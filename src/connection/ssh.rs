//! SSH connection module
//!
//! This module provides SSH connectivity using the ssh2 crate.
//! It supports key-based authentication, password authentication,
//! SSH agent support, and SFTP for file transfers.

use async_trait::async_trait;
use parking_lot::Mutex;
use ssh2::{Session, Sftp};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tracing::{debug, trace};

use crate::security::BecomeValidator;

use super::config::{ConnectionConfig, HostConfig};
use super::ssh_common;
use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

struct KeyboardInteractivePassword {
    password: String,
}

impl ssh2::KeyboardInteractivePrompt for KeyboardInteractivePassword {
    fn prompt<'a>(
        &mut self,
        _username: &str,
        _instructions: &str,
        prompts: &[ssh2::Prompt<'a>],
    ) -> Vec<String> {
        prompts
            .iter()
            .map(|_| self.password.clone())
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthAttempt {
    Agent,
    PublicKey,
    Password,
    KeyboardInteractive,
}

fn supports_auth_method(methods: &str, method: &str) -> bool {
    methods
        .split(',')
        .map(|entry| entry.trim())
        .any(|entry| entry == method)
}

/// SSH connection implementation using ssh2 crate
pub struct SshConnection {
    /// Session identifier
    identifier: String,
    /// SSH session (wrapped in Arc<Mutex> for thread safety)
    session: Arc<Mutex<Session>>,
    /// Host configuration
    host_config: HostConfig,
    /// Whether the connection is established
    connected: Arc<Mutex<bool>>,
}

impl SshConnection {
    /// Connect to a remote host via SSH
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<Self> {
        let ssh_common::ResolvedConnectionParams {
            host_config,
            retry_config,
            host: actual_host,
            port: actual_port,
            user: actual_user,
            timeout,
            identifier,
        } = ssh_common::resolve_connection_params(host, port, user, host_config, global_config);

        debug!(
            host = %actual_host,
            port = %actual_port,
            user = %actual_user,
            "Connecting via SSH"
        );

        // Clone values for the blocking task
        let host_owned = actual_host.clone();
        let port_owned = actual_port;
        let user_owned = actual_user.clone();
        let config_owned = host_config.clone();
        let global_config_owned = global_config.clone();
        let timeout_owned = timeout;
        let retry_config_owned = retry_config.clone();

        // Run connection in a blocking task since ssh2 is synchronous
        let session = task::spawn_blocking(move || {
            ssh_common::connect_with_retry_blocking(&retry_config_owned, "SSH connection", || {
                Self::do_connect(
                    &host_owned,
                    port_owned,
                    &user_owned,
                    &config_owned,
                    &global_config_owned,
                    timeout_owned,
                )
            })
        })
        .await
        .map_err(|e| ConnectionError::ConnectionFailed(format!("Task join error: {}", e)))??;

        Ok(Self {
            identifier,
            session: Arc::new(Mutex::new(session)),
            host_config,
            connected: Arc::new(Mutex::new(true)),
        })
    }

    /// Perform the actual connection
    fn do_connect(
        host: &str,
        port: u16,
        user: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
        timeout: Duration,
    ) -> ConnectionResult<Session> {
        // Create TCP connection
        let addr = format!("{}:{}", host, port);
        let tcp = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| {
                ConnectionError::ConnectionFailed(format!("Invalid address {}: {}", addr, e))
            })?,
            timeout,
        )
        .map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to connect to {}: {}", addr, e))
        })?;

        // Set TCP options
        tcp.set_read_timeout(Some(timeout)).ok();
        tcp.set_write_timeout(Some(timeout)).ok();
        tcp.set_nodelay(true).ok();

        // Create SSH session
        let mut session = Session::new().map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to create SSH session: {}", e))
        })?;

        session.set_tcp_stream(tcp);
        session.set_timeout(timeout.as_millis() as u32);

        // Enable compression if configured
        if host_config.compression {
            session.set_compress(true);
        }

        // Perform SSH handshake
        session.handshake().map_err(|e| {
            ConnectionError::ConnectionFailed(format!("SSH handshake failed: {}", e))
        })?;

        // Authenticate
        Self::authenticate(&session, user, host_config, global_config)?;

        debug!("SSH connection established successfully");
        Ok(session)
    }

    /// Perform SSH authentication
    fn authenticate(
        session: &Session,
        user: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<()> {
        // Get available authentication methods
        let methods = session.auth_methods(user).map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to get auth methods: {}", e))
        })?;

        debug!(methods = %methods, "Available authentication methods");

        if supports_auth_method(&methods, "keyboard-interactive") && host_config.password.is_none()
        {
            debug!("Keyboard-interactive auth available but no password provided");
        }

        for attempt in Self::build_auth_attempts(&methods, host_config, global_config) {
            match attempt {
                AuthAttempt::Agent => {
                    if Self::try_agent_auth(session, user).is_ok() {
                        debug!("Authenticated using SSH agent");
                        return Ok(());
                    }
                }
                AuthAttempt::PublicKey => {
                    for key_path in
                        ssh_common::identity_file_candidates(host_config, global_config)
                    {
                        if Self::try_key_auth(
                            session,
                            user,
                            &key_path,
                            host_config.password.as_deref(),
                        )
                        .is_ok()
                        {
                            debug!(key = %key_path.display(), "Authenticated using key");
                            return Ok(());
                        }
                    }
                }
                AuthAttempt::Password => {
                    if let Some(password) = &host_config.password {
                        session.userauth_password(user, password).map_err(|e| {
                            ConnectionError::AuthenticationFailed(format!(
                                "Password authentication failed: {}",
                                e
                            ))
                        })?;

                        if session.authenticated() {
                            debug!("Authenticated using password");
                            return Ok(());
                        }
                    }
                }
                AuthAttempt::KeyboardInteractive => {
                    if let Some(password) = &host_config.password {
                        if Self::try_keyboard_interactive_auth(session, user, password).is_ok() {
                            debug!("Authenticated using keyboard-interactive");
                            return Ok(());
                        }
                    }
                }
            }
        }

        Err(ConnectionError::AuthenticationFailed(
            "All authentication methods failed".to_string(),
        ))
    }

    fn build_auth_attempts(
        methods: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
    ) -> Vec<AuthAttempt> {
        let mut attempts = Vec::new();

        if global_config.defaults.use_agent && supports_auth_method(methods, "publickey") {
            attempts.push(AuthAttempt::Agent);
        }

        if supports_auth_method(methods, "publickey") {
            attempts.push(AuthAttempt::PublicKey);
        }

        if supports_auth_method(methods, "password") && host_config.password.is_some() {
            attempts.push(AuthAttempt::Password);
        }

        if supports_auth_method(methods, "keyboard-interactive") && host_config.password.is_some() {
            attempts.push(AuthAttempt::KeyboardInteractive);
        }

        attempts
    }

    /// Try SSH agent authentication
    fn try_agent_auth(session: &Session, user: &str) -> ConnectionResult<()> {
        let mut agent = session.agent().map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to connect to SSH agent: {}", e))
        })?;

        agent.connect().map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to connect to SSH agent: {}", e))
        })?;

        agent.list_identities().map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to list agent identities: {}", e))
        })?;

        for identity in agent.identities().unwrap_or_default() {
            if agent.userauth(user, &identity).is_ok() && session.authenticated() {
                return Ok(());
            }
        }

        Err(ConnectionError::AuthenticationFailed(
            "No suitable agent identity found".to_string(),
        ))
    }

    /// Try key-based authentication
    fn try_key_auth(
        session: &Session,
        user: &str,
        key_path: &Path,
        passphrase: Option<&str>,
    ) -> ConnectionResult<()> {
        if !key_path.exists() {
            return Err(ConnectionError::AuthenticationFailed(format!(
                "Key file not found: {}",
                key_path.display()
            )));
        }

        // Try with passphrase
        session
            .userauth_pubkey_file(user, None, key_path, passphrase)
            .map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Key authentication failed for {}: {}",
                    key_path.display(),
                    e
                ))
            })?;

        if session.authenticated() {
            Ok(())
        } else {
            Err(ConnectionError::AuthenticationFailed(
                "Key authentication failed".to_string(),
            ))
        }
    }

    /// Try keyboard-interactive authentication
    fn try_keyboard_interactive_auth(
        session: &Session,
        user: &str,
        password: &str,
    ) -> ConnectionResult<()> {
        let mut prompter = KeyboardInteractivePassword {
            password: password.to_string(),
        };

        session
            .userauth_keyboard_interactive(user, &mut prompter)
            .map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Keyboard-interactive authentication failed: {}",
                    e
                ))
            })?;

        if session.authenticated() {
            Ok(())
        } else {
            Err(ConnectionError::AuthenticationFailed(
                "Keyboard-interactive authentication failed".to_string(),
            ))
        }
    }

    /// Execute a command on the remote host (synchronous)
    fn exec_sync(
        session: &Session,
        command: &str,
        options: &ExecuteOptions,
    ) -> ConnectionResult<CommandResult> {
        let mut channel = session.channel_session().map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to open channel: {}", e))
        })?;

        // Build the full command with options
        let full_command = Self::build_command(command, options)?;

        trace!(command = %full_command, "Executing remote command");

        // Set environment variables
        for (key, value) in &options.env {
            let _ = channel.setenv(key, value);
        }

        // Execute the command
        channel.exec(&full_command).map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to execute command: {}", e))
        })?;

        // Handle escalation password if needed
        if options.escalate && options.escalate_password.is_some() {
            let password = options.escalate_password.as_ref().unwrap();
            channel
                .write_all(format!("{}\n", password).as_bytes())
                .map_err(|e| {
                    ConnectionError::ExecutionFailed(format!("Failed to write password: {}", e))
                })?;
        }

        // Read stdout
        let mut stdout = String::new();
        channel.read_to_string(&mut stdout).map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to read stdout: {}", e))
        })?;

        // Read stderr
        let mut stderr = String::new();
        channel.stderr().read_to_string(&mut stderr).map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to read stderr: {}", e))
        })?;

        // Wait for exit and get exit status
        channel.wait_close().ok();
        let exit_code = channel.exit_status().unwrap_or(-1);

        trace!(exit_code = %exit_code, "Command completed");

        if exit_code == 0 {
            Ok(CommandResult::success(stdout, stderr))
        } else {
            Ok(CommandResult::failure(exit_code, stdout, stderr))
        }
    }

    /// Build command string with options
    fn build_command(command: &str, options: &ExecuteOptions) -> ConnectionResult<String> {
        let mut parts = Vec::new();

        // Add working directory
        if let Some(cwd) = &options.cwd {
            parts.push(format!("cd {} && ", cwd));
        }

        // Handle privilege escalation
        if options.escalate {
            let escalate_method = options.escalate_method.as_deref().unwrap_or("sudo");
            let escalate_user = options.escalate_user.as_deref().unwrap_or("root");

            BecomeValidator::new().validate_username(escalate_user).map_err(|e| {
                ConnectionError::InvalidConfig(format!(
                    "Invalid escalation user '{}': {}",
                    escalate_user, e
                ))
            })?;

            match escalate_method {
                "sudo" => {
                    if options.escalate_password.is_some() {
                        parts.push(format!("sudo -S -u {} -- ", escalate_user));
                    } else {
                        parts.push(format!("sudo -u {} -- ", escalate_user));
                    }
                }
                "su" => {
                    parts.push(format!("su - {} -c ", escalate_user));
                }
                "doas" => {
                    parts.push(format!("doas -u {} ", escalate_user));
                }
                _ => {
                    parts.push(format!("sudo -u {} -- ", escalate_user));
                }
            }
        }

        parts.push(command.to_string());
        Ok(parts.concat())
    }

    /// Get SFTP session
    fn get_sftp(session: &Session) -> ConnectionResult<Sftp> {
        session.sftp().map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to open SFTP channel: {}", e))
        })
    }
}

#[async_trait]
impl Connection for SshConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        let session = self.session.clone();
        let connected = self.connected.clone();

        task::spawn_blocking(move || {
            if !*connected.lock() {
                return false;
            }

            let session = session.lock();
            // Try to send a keepalive to check if connection is alive
            session.keepalive_send().is_ok()
        })
        .await
        .unwrap_or(false)
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();
        let session = self.session.clone();
        let command = command.to_string();

        // Check for timeout
        let timeout = options.timeout;

        let result = task::spawn_blocking(move || {
            let session = session.lock();
            Self::exec_sync(&session, &command, &options)
        });

        if let Some(timeout_secs) = timeout {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), result).await {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => Err(ConnectionError::ExecutionFailed(format!(
                    "Task join error: {}",
                    e
                ))),
                Err(_) => Err(ConnectionError::Timeout(timeout_secs)),
            }
        } else {
            result
                .await
                .map_err(|e| ConnectionError::ExecutionFailed(format!("Task join error: {}", e)))?
        }
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();
        let session = self.session.clone();
        let local_path = local_path.to_path_buf();
        let remote_path = remote_path.to_path_buf();

        debug!(
            local = %local_path.display(),
            remote = %remote_path.display(),
            "Uploading file via SFTP"
        );

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            // Create parent directories if needed
            if options.create_dirs {
                if let Some(parent) = remote_path.parent() {
                    Self::create_remote_dirs(&sftp, parent)?;
                }
            }

            // Read local file
            let content = std::fs::read(&local_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to read local file {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

            // Write to remote file
            let mode = options.mode.unwrap_or(0o644);
            // Use open_mode to set permissions atomically at creation time
            // This prevents the race condition where file is created with 644 and then chmodded
            let mut remote_file = sftp.open_mode(
                &remote_path,
                ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::TRUNCATE,
                mode as i32,
                ssh2::OpenType::File,
            ).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            remote_file.write_all(&content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to write to remote file: {}", e))
            })?;

            // Close file and sftp session
            drop(remote_file);
            drop(sftp);

            // Set owner/group if specified
            if options.owner.is_some() || options.group.is_some() {
                let ownership = match (&options.owner, &options.group) {
                    (Some(o), Some(g)) => format!("{}:{}", o, g),
                    (Some(o), None) => o.to_string(),
                    (None, Some(g)) => format!(":{}", g),
                    (None, None) => return Ok(()),
                };

                let chown_cmd = format!("chown {} {}", ownership, remote_path.display());
                let mut channel = session.channel_session().map_err(|e| {
                    ConnectionError::TransferFailed(format!("Failed to open channel: {}", e))
                })?;
                channel.exec(&chown_cmd).ok();
                channel.wait_close().ok();
            }

            Ok(())
        })
        .await
        .map_err(|e| ConnectionError::TransferFailed(format!("Task join error: {}", e)))?
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();
        let session = self.session.clone();
        let content = content.to_vec();
        let remote_path = remote_path.to_path_buf();

        debug!(
            remote = %remote_path.display(),
            size = %content.len(),
            "Uploading content via SFTP"
        );

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            // Create parent directories if needed
            if options.create_dirs {
                if let Some(parent) = remote_path.parent() {
                    Self::create_remote_dirs(&sftp, parent)?;
                }
            }

            // Write to remote file
            let mode = options.mode.unwrap_or(0o644);
            // Use open_mode to set permissions atomically at creation time
            let mut remote_file = sftp.open_mode(
                &remote_path,
                ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::TRUNCATE,
                mode as i32,
                ssh2::OpenType::File,
            ).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            remote_file.write_all(&content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to write to remote file: {}", e))
            })?;

            drop(remote_file);
            drop(sftp);

            // Set owner/group if specified
            if options.owner.is_some() || options.group.is_some() {
                let ownership = match (&options.owner, &options.group) {
                    (Some(o), Some(g)) => format!("{}:{}", o, g),
                    (Some(o), None) => o.to_string(),
                    (None, Some(g)) => format!(":{}", g),
                    (None, None) => return Ok(()),
                };

                let chown_cmd = format!("chown {} {}", ownership, remote_path.display());
                let mut channel = session.channel_session().map_err(|e| {
                    ConnectionError::TransferFailed(format!("Failed to open channel: {}", e))
                })?;
                channel.exec(&chown_cmd).ok();
                channel.wait_close().ok();
            }

            Ok(())
        })
        .await
        .map_err(|e| ConnectionError::TransferFailed(format!("Task join error: {}", e)))?
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        let session = self.session.clone();
        let remote_path = remote_path.to_path_buf();
        let local_path = local_path.to_path_buf();

        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            "Downloading file via SFTP"
        );

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            // Open remote file
            let mut remote_file = sftp.open(&remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to open remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            // Read content
            let mut content = Vec::new();
            remote_file.read_to_end(&mut content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to read remote file: {}", e))
            })?;

            // Create parent directories for local file
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ConnectionError::TransferFailed(format!(
                        "Failed to create local directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }

            // Write local file
            std::fs::write(&local_path, &content).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to write local file {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

            Ok(())
        })
        .await
        .map_err(|e| ConnectionError::TransferFailed(format!("Task join error: {}", e)))?
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        let session = self.session.clone();
        let remote_path = remote_path.to_path_buf();

        debug!(remote = %remote_path.display(), "Downloading content via SFTP");

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            // Open remote file
            let mut remote_file = sftp.open(&remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to open remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            // Read content
            let mut content = Vec::new();
            remote_file.read_to_end(&mut content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to read remote file: {}", e))
            })?;

            Ok(content)
        })
        .await
        .map_err(|e| ConnectionError::TransferFailed(format!("Task join error: {}", e)))?
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        let session = self.session.clone();
        let path = path.to_path_buf();

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;
            Ok(sftp.stat(&path).is_ok())
        })
        .await
        .map_err(|e| ConnectionError::ExecutionFailed(format!("Task join error: {}", e)))?
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        let session = self.session.clone();
        let path = path.to_path_buf();

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            match sftp.stat(&path) {
                Ok(stat) => Ok(stat.is_dir()),
                Err(_) => Ok(false),
            }
        })
        .await
        .map_err(|e| ConnectionError::ExecutionFailed(format!("Task join error: {}", e)))?
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        let session = self.session.clone();
        let path = path.to_path_buf();

        task::spawn_blocking(move || {
            let session = session.lock();
            let sftp = Self::get_sftp(&session)?;

            let stat = sftp.stat(&path).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to stat {}: {}", path.display(), e))
            })?;

            Ok(FileStat {
                size: stat.size.unwrap_or(0),
                mode: stat.perm.unwrap_or(0),
                uid: stat.uid.unwrap_or(0),
                gid: stat.gid.unwrap_or(0),
                atime: stat.atime.map(|t| t as i64).unwrap_or(0),
                mtime: stat.mtime.map(|t| t as i64).unwrap_or(0),
                is_dir: stat.is_dir(),
                is_file: stat.is_file(),
                is_symlink: false, // ssh2 stat doesn't distinguish symlinks
            })
        })
        .await
        .map_err(|e| ConnectionError::TransferFailed(format!("Task join error: {}", e)))?
    }

    async fn close(&self) -> ConnectionResult<()> {
        let session = self.session.clone();
        let connected = self.connected.clone();

        task::spawn_blocking(move || {
            let mut connected_guard = connected.lock();
            if *connected_guard {
                let session = session.lock();
                session.disconnect(None, "Connection closed", None).ok();
                *connected_guard = false;
            }
            Ok(())
        })
        .await
        .map_err(|e| ConnectionError::ConnectionFailed(format!("Task join error: {}", e)))?
    }
}

impl SshConnection {
    /// Create remote directories recursively
    fn create_remote_dirs(sftp: &Sftp, path: &Path) -> ConnectionResult<()> {
        // Split path into components and create each directory
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);

            // Skip root
            if current.to_string_lossy() == "/" {
                continue;
            }

            // Try to create directory (ignore error if it already exists)
            if sftp.stat(&current).is_err() {
                sftp.mkdir(&current, 0o755).ok();
            }
        }

        Ok(())
    }
}

impl std::fmt::Debug for SshConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshConnection")
            .field("identifier", &self.identifier)
            .field("connected", &*self.connected.lock())
            .finish()
    }
}

/// Builder for SSH connections
pub struct SshConnectionBuilder {
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    private_key: Option<PathBuf>,
    timeout: Option<Duration>,
    compression: bool,
}

impl SshConnectionBuilder {
    /// Create a new SSH connection builder
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 22,
            user: std::env::var("USER").unwrap_or_else(|_| "root".to_string()),
            password: None,
            private_key: None,
            timeout: Some(Duration::from_secs(30)),
            compression: false,
        }
    }

    /// Set the port
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the username
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the password
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the private key path
    pub fn private_key(mut self, path: impl Into<PathBuf>) -> Self {
        self.private_key = Some(path.into());
        self
    }

    /// Set the connection timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Enable compression
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Build and connect
    pub async fn connect(self) -> ConnectionResult<SshConnection> {
        let host_config = HostConfig {
            hostname: Some(self.host.clone()),
            port: Some(self.port),
            user: Some(self.user.clone()),
            password: self.password,
            identity_file: self.private_key.map(|p| p.to_string_lossy().to_string()),
            connect_timeout: self.timeout.map(|d| d.as_secs()),
            compression: self.compression,
            ..Default::default()
        };

        let config = ConnectionConfig::default();
        SshConnection::connect(
            &self.host,
            self.port,
            &self.user,
            Some(host_config),
            &config,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh2::KeyboardInteractivePrompt;
    use std::borrow::Cow;

    #[test]
    fn test_build_command_basic() {
        let options = ExecuteOptions::default();
        let cmd = SshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_build_command_with_cwd() {
        let options = ExecuteOptions::new().with_cwd("/tmp");
        let cmd = SshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "cd /tmp && echo hello");
    }

    #[test]
    fn test_build_command_with_escalation() {
        let options = ExecuteOptions::new().with_escalation(Some("admin".to_string()));
        let cmd = SshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "sudo -u admin -- echo hello");
    }

    #[test]
    fn test_build_command_with_cwd_and_escalation() {
        let options = ExecuteOptions::new()
            .with_cwd("/var/log")
            .with_escalation(None);
        let cmd = SshConnection::build_command("cat syslog", &options).unwrap();
        assert_eq!(cmd, "cd /var/log && sudo -u root -- cat syslog");
    }

    #[test]
    fn test_build_command_rejects_invalid_user() {
        let options =
            ExecuteOptions::new().with_escalation(Some("root; rm -rf /".to_string()));
        let result = SshConnection::build_command("echo hello", &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_ssh_connection_builder() {
        let builder = SshConnectionBuilder::new("example.com")
            .port(2222)
            .user("admin")
            .compression(true);

        assert_eq!(builder.host, "example.com");
        assert_eq!(builder.port, 2222);
        assert_eq!(builder.user, "admin");
        assert!(builder.compression);
    }

    #[test]
    fn test_keyboard_interactive_prompts_repeat_password() {
        let mut prompter = KeyboardInteractivePassword {
            password: "secret".to_string(),
        };
        let prompts = vec![
            ssh2::Prompt {
                text: Cow::Borrowed("Password: "),
                echo: false,
            },
            ssh2::Prompt {
                text: Cow::Borrowed("OTP: "),
                echo: false,
            },
        ];

        let responses = prompter.prompt("user", "instructions", &prompts);
        assert_eq!(
            responses,
            vec!["secret".to_string(), "secret".to_string()]
        );
    }

    #[test]
    fn test_build_auth_attempts_includes_keyboard_interactive() {
        let mut host_config = HostConfig::default();
        host_config.password = Some("pw".to_string());
        let global_config = ConnectionConfig::default();

        let attempts = SshConnection::build_auth_attempts(
            "publickey,keyboard-interactive",
            &host_config,
            &global_config,
        );

        assert_eq!(
            attempts,
            vec![
                AuthAttempt::Agent,
                AuthAttempt::PublicKey,
                AuthAttempt::KeyboardInteractive
            ]
        );
    }

    #[test]
    fn test_build_auth_attempts_skip_keyboard_interactive_without_password() {
        let host_config = HostConfig::default();
        let global_config = ConnectionConfig::default();

        let attempts = SshConnection::build_auth_attempts(
            "publickey,keyboard-interactive",
            &host_config,
            &global_config,
        );

        assert_eq!(attempts, vec![AuthAttempt::Agent, AuthAttempt::PublicKey]);
    }
}
