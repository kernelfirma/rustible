//! Unit and integration tests for RusshConnection
//!
//! This test module provides comprehensive test coverage for the russh-based SSH
//! connection implementation. Tests are organized into:
//!
//! 1. **Unit tests** - Tests that can run without real SSH infrastructure
//!    - Connection config parsing
//!    - Auth config handling
//!    - Command result parsing
//!    - Error type conversions
//!
//! 2. **Integration tests** - Tests that require real SSH infrastructure (skip at runtime if unavailable)
//!    - Connection establishment
//!    - Command execution
//!    - File upload/download
//!    - SFTP operations
//!
//! To run unit tests only:
//! ```bash
//! cargo test --test russh_tests
//! ```
//!
//! To run all tests including integration tests (requires SSH infrastructure):
//! ```bash
//! cargo test --test russh_tests -- --include-ignored
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use tempfile::TempDir;

use rustible::connection::config::{
    ConnectionConfig, HostConfig, RetryConfig, SshConfigParser, DEFAULT_RETRIES,
    DEFAULT_RETRY_DELAY, DEFAULT_TIMEOUT,
};
#[cfg(feature = "russh")]
use rustible::connection::RusshConnection;
use rustible::connection::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

// ============================================================================
// TEST FIXTURES DIRECTORY SETUP
// ============================================================================

/// Helper to create a temporary test fixtures directory
fn setup_test_fixtures() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir for test fixtures");

    // Create standard fixture files
    let ssh_dir = temp_dir.path().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).expect("Failed to create .ssh dir");

    // Create a mock SSH config file
    let ssh_config_content = r#"
Host test-server
    HostName 192.168.1.100
    User testuser
    Port 2222
    IdentityFile ~/.ssh/test_key

Host *.internal
    User internal
    ProxyJump bastion

Host bastion
    HostName bastion.example.com
    User jump
    Port 22
"#;
    std::fs::write(ssh_dir.join("config"), ssh_config_content)
        .expect("Failed to write SSH config fixture");

    // Create mock key files (empty, just for path testing)
    std::fs::write(ssh_dir.join("test_key"), "").expect("Failed to write test key");
    std::fs::write(ssh_dir.join("test_key.pub"), "").expect("Failed to write test key pub");

    temp_dir
}

/// Check if we should skip CI-sensitive tests
fn should_skip_in_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

/// Check if real SSH infrastructure is available for integration tests
fn has_ssh_infrastructure() -> bool {
    // Check for environment variable indicating SSH test server is available
    std::env::var("RUSTIBLE_SSH_TEST_HOST").is_ok()
}

/// Get SSH test configuration from environment
fn get_ssh_test_config() -> Option<(String, u16, String)> {
    let host = std::env::var("RUSTIBLE_SSH_TEST_HOST").ok()?;
    let port = std::env::var("RUSTIBLE_SSH_TEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let user = std::env::var("RUSTIBLE_SSH_TEST_USER")
        .ok()
        .unwrap_or_else(|| "testuser".to_string());
    Some((host, port, user))
}

/// Get SSH private key path from environment
fn get_ssh_test_key() -> PathBuf {
    std::env::var("RUSTIBLE_SSH_TEST_KEY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".ssh/id_ed25519")
        })
}

/// Get SSH jump host configuration from environment
fn get_ssh_test_jump_config(prefix: &str, default_user: &str) -> Option<(String, u16, String)> {
    let host = std::env::var(format!("{}_HOST", prefix)).ok()?;
    let port = std::env::var(format!("{}_PORT", prefix))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let user = std::env::var(format!("{}_USER", prefix))
        .ok()
        .unwrap_or_else(|| default_user.to_string());
    Some((host, port, user))
}

/// Get SSH jump host private key path from environment
fn get_ssh_test_jump_key(prefix: &str) -> PathBuf {
    std::env::var(format!("{}_KEY", prefix))
        .map(PathBuf::from)
        .unwrap_or_else(|_| get_ssh_test_key())
}

// ============================================================================
// MOCK RUSSH CONNECTION FOR UNIT TESTS
// ============================================================================

/// Mock russh connection for unit testing without real SSH infrastructure
#[derive(Debug)]
pub struct MockRusshConnection {
    identifier: String,
    host: String,
    port: u16,
    user: String,
    alive: AtomicBool,
    connected: AtomicBool,
    commands_executed: RwLock<Vec<String>>,
    virtual_filesystem: RwLock<HashMap<PathBuf, Vec<u8>>>,
    command_results: RwLock<HashMap<String, CommandResult>>,
    default_result: RwLock<CommandResult>,
    execution_count: AtomicU32,
    should_fail_auth: AtomicBool,
    should_fail_connection: AtomicBool,
    should_timeout: AtomicBool,
    latency_ms: AtomicU32,
    auth_method: RwLock<Option<String>>,
}

impl MockRusshConnection {
    /// Create a new mock russh connection
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self {
            identifier: format!("russh:{}@{}:{}", user, host, port),
            host: host.to_string(),
            port,
            user: user.to_string(),
            alive: AtomicBool::new(true),
            connected: AtomicBool::new(true),
            commands_executed: RwLock::new(Vec::new()),
            virtual_filesystem: RwLock::new(HashMap::new()),
            command_results: RwLock::new(HashMap::new()),
            default_result: RwLock::new(CommandResult::success(String::new(), String::new())),
            execution_count: AtomicU32::new(0),
            should_fail_auth: AtomicBool::new(false),
            should_fail_connection: AtomicBool::new(false),
            should_timeout: AtomicBool::new(false),
            latency_ms: AtomicU32::new(0),
            auth_method: RwLock::new(None),
        }
    }

    /// Set a specific command result
    pub fn set_command_result(&self, command: &str, result: CommandResult) {
        self.command_results
            .write()
            .insert(command.to_string(), result);
    }

    /// Set the default result for commands
    pub fn set_default_result(&self, result: CommandResult) {
        *self.default_result.write() = result;
    }

    /// Configure authentication failure
    pub fn set_auth_failure(&self, should_fail: bool) {
        self.should_fail_auth.store(should_fail, Ordering::SeqCst);
    }

    /// Configure connection failure
    pub fn set_connection_failure(&self, should_fail: bool) {
        self.should_fail_connection
            .store(should_fail, Ordering::SeqCst);
    }

    /// Configure timeout behavior
    pub fn set_timeout(&self, should_timeout: bool) {
        self.should_timeout.store(should_timeout, Ordering::SeqCst);
    }

    /// Set simulated network latency
    pub fn set_latency_ms(&self, latency: u32) {
        self.latency_ms.store(latency, Ordering::SeqCst);
    }

    /// Set authentication method used
    #[allow(dead_code)]
    pub fn set_auth_method(&self, method: &str) {
        *self.auth_method.write() = Some(method.to_string());
    }

    /// Get command count
    pub fn command_count(&self) -> u32 {
        self.execution_count.load(Ordering::SeqCst)
    }

    /// Get all executed commands
    pub fn get_commands(&self) -> Vec<String> {
        self.commands_executed.read().clone()
    }

    /// Add virtual file
    #[allow(dead_code)]
    pub fn add_virtual_file(&self, path: PathBuf, content: Vec<u8>) {
        self.virtual_filesystem.write().insert(path, content);
    }

    /// Kill the connection
    pub fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
    }

    /// Simulate network delay
    async fn simulate_latency(&self) {
        let latency = self.latency_ms.load(Ordering::SeqCst);
        if latency > 0 {
            tokio::time::sleep(Duration::from_millis(latency as u64)).await;
        }
    }

    /// Get host
    #[allow(dead_code)]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Get port
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get user
    #[allow(dead_code)]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Get auth method
    #[allow(dead_code)]
    pub fn auth_method(&self) -> Option<String> {
        self.auth_method.read().clone()
    }
}

#[async_trait]
impl Connection for MockRusshConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && self.connected.load(Ordering::SeqCst)
    }

    async fn execute(
        &self,
        command: &str,
        _options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        if self.should_fail_connection.load(Ordering::SeqCst) {
            return Err(ConnectionError::ConnectionFailed(
                "Connection refused".to_string(),
            ));
        }

        if self.should_fail_auth.load(Ordering::SeqCst) {
            return Err(ConnectionError::AuthenticationFailed(
                "Authentication failed".to_string(),
            ));
        }

        if self.should_timeout.load(Ordering::SeqCst) {
            return Err(ConnectionError::Timeout(30));
        }

        self.simulate_latency().await;

        self.execution_count.fetch_add(1, Ordering::SeqCst);
        self.commands_executed.write().push(command.to_string());

        if let Some(result) = self.command_results.read().get(command) {
            return Ok(result.clone());
        }

        Ok(self.default_result.read().clone())
    }

    async fn upload(
        &self,
        src: &Path,
        dest: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if self.should_fail_connection.load(Ordering::SeqCst) {
            return Err(ConnectionError::TransferFailed(
                "Connection failed".to_string(),
            ));
        }

        self.simulate_latency().await;

        if src.exists() {
            if let Ok(content) = std::fs::read(src) {
                self.virtual_filesystem
                    .write()
                    .insert(dest.to_path_buf(), content);
            }
        }

        Ok(())
    }

    async fn upload_content(
        &self,
        content: &[u8],
        dest: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if self.should_fail_connection.load(Ordering::SeqCst) {
            return Err(ConnectionError::TransferFailed(
                "Connection failed".to_string(),
            ));
        }

        self.simulate_latency().await;
        self.virtual_filesystem
            .write()
            .insert(dest.to_path_buf(), content.to_vec());
        Ok(())
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        if self.should_fail_connection.load(Ordering::SeqCst) {
            return Err(ConnectionError::TransferFailed(
                "Connection failed".to_string(),
            ));
        }

        self.simulate_latency().await;

        if let Some(content) = self.virtual_filesystem.read().get(remote_path) {
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(local_path, content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to write local file: {}", e))
            })?;
            return Ok(());
        }

        Err(ConnectionError::TransferFailed(format!(
            "Remote file not found: {:?}",
            remote_path
        )))
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        if self.should_fail_connection.load(Ordering::SeqCst) {
            return Err(ConnectionError::TransferFailed(
                "Connection failed".to_string(),
            ));
        }

        self.simulate_latency().await;

        if let Some(content) = self.virtual_filesystem.read().get(remote_path) {
            return Ok(content.clone());
        }

        Err(ConnectionError::TransferFailed(format!(
            "Remote file not found: {:?}",
            remote_path
        )))
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        self.simulate_latency().await;
        Ok(self.virtual_filesystem.read().contains_key(path))
    }

    async fn is_directory(&self, _path: &Path) -> ConnectionResult<bool> {
        self.simulate_latency().await;
        Ok(false)
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        self.simulate_latency().await;

        if let Some(content) = self.virtual_filesystem.read().get(path) {
            Ok(FileStat {
                size: content.len() as u64,
                mode: 0o644,
                uid: 1000,
                gid: 1000,
                atime: 0,
                mtime: 0,
                is_dir: false,
                is_file: true,
                is_symlink: false,
            })
        } else {
            Err(ConnectionError::TransferFailed(format!(
                "File not found: {:?}",
                path
            )))
        }
    }

    async fn close(&self) -> ConnectionResult<()> {
        self.alive.store(false, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }
}

// ============================================================================
// UNIT TESTS - CONNECTION CONFIG PARSING
// ============================================================================

mod config_parsing {
    use super::*;

    #[test]
    fn test_connection_config_default_values() {
        let config = ConnectionConfig::default();

        assert_eq!(config.defaults.port, 22);
        assert_eq!(config.defaults.timeout, DEFAULT_TIMEOUT);
        assert_eq!(config.defaults.retries, DEFAULT_RETRIES);
        assert!(config.defaults.use_agent);
        assert!(config.defaults.verify_host_key);
    }

    #[test]
    fn test_host_config_builder() {
        let config = HostConfig::new()
            .hostname("example.com")
            .port(2222)
            .user("admin")
            .identity_file("~/.ssh/id_ed25519")
            .timeout(60);

        assert_eq!(config.hostname, Some("example.com".to_string()));
        assert_eq!(config.port, Some(2222));
        assert_eq!(config.user, Some("admin".to_string()));
        assert_eq!(config.identity_file, Some("~/.ssh/id_ed25519".to_string()));
        assert_eq!(config.connect_timeout, Some(60));
    }

    #[test]
    fn test_host_config_timeout_duration() {
        let config = HostConfig::new().timeout(120);
        assert_eq!(config.timeout_duration(), Duration::from_secs(120));

        let default_config = HostConfig::new();
        assert_eq!(
            default_config.timeout_duration(),
            Duration::from_secs(DEFAULT_TIMEOUT)
        );
    }

    #[test]
    fn test_ssh_config_parsing_basic() {
        let config = r#"
Host example
    HostName example.com
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let example = hosts.get("example").unwrap();

        assert_eq!(example.hostname, Some("example.com".to_string()));
        assert_eq!(example.port, Some(2222));
        assert_eq!(example.user, Some("admin".to_string()));
        assert!(example.identity_file.is_some());
    }

    #[test]
    fn test_ssh_config_parsing_with_wildcards() {
        let config = r#"
Host *.production.example.com
    User prodadmin
    Port 2222

Host *.staging.example.com
    User stagingadmin
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        assert!(hosts.contains_key("*.production.example.com"));
        assert!(hosts.contains_key("*.staging.example.com"));

        let prod = hosts.get("*.production.example.com").unwrap();
        assert_eq!(prod.user, Some("prodadmin".to_string()));
        assert_eq!(prod.port, Some(2222));
    }

    #[test]
    fn test_ssh_config_parsing_proxy_jump() {
        let config = r#"
Host internal
    HostName internal.private
    ProxyJump bastion.example.com
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let internal = hosts.get("internal").unwrap();

        assert_eq!(internal.proxy_jump, Some("bastion.example.com".to_string()));
    }

    #[test]
    fn test_ssh_config_parsing_compression() {
        let config = r#"
Host compressed
    Compression yes

Host uncompressed
    Compression no
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        assert!(hosts.get("compressed").unwrap().compression);
        assert!(!hosts.get("uncompressed").unwrap().compression);
    }

    #[test]
    fn test_ssh_config_parsing_forward_agent() {
        let config = r#"
Host forwarding
    ForwardAgent yes
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        assert!(hosts.get("forwarding").unwrap().forward_agent);
    }

    #[test]
    fn test_ssh_config_parsing_server_alive() {
        let config = r#"
Host keepalive
    ServerAliveInterval 60
    ServerAliveCountMax 3
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let keepalive = hosts.get("keepalive").unwrap();

        assert_eq!(keepalive.server_alive_interval, Some(60));
        assert_eq!(keepalive.server_alive_count_max, Some(3));
    }

    #[test]
    fn test_ssh_config_parsing_strict_host_key() {
        let config = r#"
Host strict
    StrictHostKeyChecking yes

Host lenient
    StrictHostKeyChecking no
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        assert_eq!(
            hosts.get("strict").unwrap().strict_host_key_checking,
            Some(true)
        );
        assert_eq!(
            hosts.get("lenient").unwrap().strict_host_key_checking,
            Some(false)
        );
    }

    #[test]
    fn test_ssh_config_parsing_multiple_hosts_same_line() {
        let config = r#"
Host server1 server2 server3
    User shareduser
    Port 22
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        assert_eq!(hosts.len(), 3);
        for server in ["server1", "server2", "server3"] {
            let host = hosts.get(server).unwrap();
            assert_eq!(host.user, Some("shareduser".to_string()));
            assert_eq!(host.port, Some(22));
        }
    }

    #[test]
    fn test_ssh_config_parsing_empty() {
        let config = "";
        let hosts = SshConfigParser::parse(config).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_ssh_config_parsing_comments_only() {
        let config = r#"
# This is a comment
# Another comment
    # Indented comment
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_connection_config_from_toml() {
        let toml = r#"
[defaults]
user = "admin"
port = 22
timeout = 60

[hosts.webserver]
hostname = "192.168.1.100"
port = 2222
user = "web"
"#;

        let config = ConnectionConfig::from_toml(toml).unwrap();

        assert_eq!(config.defaults.user, "admin");
        assert_eq!(config.defaults.timeout, 60);

        let webserver = config.get_host("webserver").unwrap();
        assert_eq!(webserver.hostname, Some("192.168.1.100".to_string()));
        assert_eq!(webserver.port, Some(2222));
    }

    #[test]
    fn test_connection_config_host_merge() {
        let mut config = ConnectionConfig::new();
        config.defaults.user = "default_user".to_string();
        config.defaults.port = 22;
        config.defaults.timeout = 30;

        config.add_host("partial", HostConfig::new().hostname("partial.example.com"));

        let merged = config.get_host_merged("partial");

        assert_eq!(merged.hostname, Some("partial.example.com".to_string()));
        assert_eq!(merged.user, Some("default_user".to_string()));
        assert_eq!(merged.port, Some(22));
        assert_eq!(merged.connect_timeout, Some(30));
    }
}

// ============================================================================
// UNIT TESTS - AUTH CONFIG HANDLING
// ============================================================================

mod auth_config {
    use super::*;

    #[test]
    fn test_auth_config_use_agent_default() {
        let config = ConnectionConfig::default();
        assert!(config.defaults.use_agent);
    }

    #[test]
    fn test_auth_config_disable_agent() {
        let mut config = ConnectionConfig::default();
        config.defaults.use_agent = false;
        assert!(!config.defaults.use_agent);
    }

    #[test]
    fn test_auth_config_identity_files() {
        let mut config = ConnectionConfig::default();
        config.defaults.identity_files =
            vec!["~/.ssh/id_ed25519".to_string(), "~/.ssh/id_rsa".to_string()];

        assert_eq!(config.defaults.identity_files.len(), 2);
        assert!(config.defaults.identity_files[0].contains("ed25519"));
    }

    #[test]
    fn test_auth_config_host_identity_file() {
        let config = HostConfig::new().identity_file("~/.ssh/custom_key");
        assert_eq!(config.identity_file, Some("~/.ssh/custom_key".to_string()));
    }

    #[test]
    fn test_auth_config_password_not_serialized() {
        let mut config = HostConfig::new();
        config.password = Some("secret".to_string());

        // Serialize and verify password is not included
        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn test_auth_config_host_overrides_default_identity() {
        let mut config = ConnectionConfig::new();
        config.defaults.identity_files = vec!["~/.ssh/default_key".to_string()];

        let host_config = HostConfig::new()
            .hostname("example.com")
            .identity_file("~/.ssh/specific_key");

        config.add_host("server", host_config);

        let server = config.get_host("server").unwrap();
        assert_eq!(
            server.identity_file,
            Some("~/.ssh/specific_key".to_string())
        );
    }
}

// ============================================================================
// UNIT TESTS - COMMAND RESULT PARSING
// ============================================================================

mod command_result_parsing {
    use super::*;

    #[test]
    fn test_command_result_success() {
        let result = CommandResult::success("output".to_string(), String::new());

        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "output");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn test_command_result_failure() {
        let result = CommandResult::failure(1, String::new(), "error".to_string());

        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert_eq!(result.stderr, "error");
    }

    #[test]
    fn test_command_result_combined_output() {
        let result = CommandResult::success("stdout".to_string(), "stderr".to_string());
        let combined = result.combined_output();

        assert!(combined.contains("stdout"));
        assert!(combined.contains("stderr"));
    }

    #[test]
    fn test_command_result_combined_output_stdout_only() {
        let result = CommandResult::success("stdout".to_string(), String::new());
        assert_eq!(result.combined_output(), "stdout");
    }

    #[test]
    fn test_command_result_combined_output_stderr_only() {
        let result = CommandResult::success(String::new(), "stderr".to_string());
        assert_eq!(result.combined_output(), "stderr");
    }

    #[tokio::test]
    async fn test_mock_command_result_capture() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_command_result(
            "echo hello",
            CommandResult::success("hello\n".to_string(), String::new()),
        );

        let result = conn.execute("echo hello", None).await.unwrap();

        assert!(result.success);
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_mock_command_result_with_exit_code() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_command_result(
            "failing_cmd",
            CommandResult::failure(127, String::new(), "command not found".to_string()),
        );

        let result = conn.execute("failing_cmd", None).await.unwrap();

        assert!(!result.success);
        assert_eq!(result.exit_code, 127);
        assert_eq!(result.stderr, "command not found");
    }
}

// ============================================================================
// UNIT TESTS - ERROR TYPE CONVERSIONS
// ============================================================================

mod error_type_conversions {
    use super::*;

    #[test]
    fn test_connection_error_connection_failed() {
        let error = ConnectionError::ConnectionFailed("Connection refused".to_string());
        let display = format!("{}", error);

        assert!(display.contains("Connection"));
        assert!(display.contains("refused"));
    }

    #[test]
    fn test_connection_error_authentication_failed() {
        let error = ConnectionError::AuthenticationFailed("Invalid credentials".to_string());
        let display = format!("{}", error);

        assert!(display.contains("Authentication"));
    }

    #[test]
    fn test_connection_error_timeout() {
        let error = ConnectionError::Timeout(30);
        let display = format!("{}", error);

        assert!(display.contains("timeout") || display.contains("30"));
    }

    #[test]
    fn test_connection_error_host_not_found() {
        let error = ConnectionError::HostNotFound("unknown.host".to_string());
        let display = format!("{}", error);

        assert!(display.contains("unknown.host") || display.contains("not found"));
    }

    #[test]
    fn test_connection_error_transfer_failed() {
        let error = ConnectionError::TransferFailed("Permission denied".to_string());
        let display = format!("{}", error);

        assert!(display.contains("Permission") || display.contains("transfer"));
    }

    #[test]
    fn test_connection_error_execution_failed() {
        let error = ConnectionError::ExecutionFailed("Command failed".to_string());
        let display = format!("{}", error);

        assert!(display.contains("failed") || display.contains("execution"));
    }

    #[test]
    fn test_connection_error_ssh_error() {
        let error = ConnectionError::SshError("SSH protocol error".to_string());
        let display = format!("{}", error);

        assert!(display.contains("SSH"));
    }

    #[test]
    fn test_connection_error_invalid_config() {
        let error = ConnectionError::InvalidConfig("Invalid port number".to_string());
        let display = format!("{}", error);

        assert!(display.contains("config") || display.contains("Invalid"));
    }

    #[test]
    fn test_connection_error_connection_closed() {
        let error = ConnectionError::ConnectionClosed;
        let display = format!("{}", error);

        assert!(display.contains("closed") || display.contains("Connection"));
    }

    #[tokio::test]
    async fn test_mock_connection_error_propagation() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn test_mock_auth_error_propagation() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_auth_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_mock_timeout_error_propagation() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_timeout(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(_))));
    }
}

// ============================================================================
// UNIT TESTS - RETRY CONFIG
// ============================================================================

mod retry_config_tests {
    use super::*;

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();

        assert_eq!(config.max_retries, DEFAULT_RETRIES);
        assert_eq!(config.retry_delay, Duration::from_secs(DEFAULT_RETRY_DELAY));
        assert!(config.exponential_backoff);
    }

    #[test]
    fn test_retry_config_exponential_backoff() {
        let config = RetryConfig::default();

        let delay0 = config.delay_for_attempt(0);
        let delay1 = config.delay_for_attempt(1);
        let delay2 = config.delay_for_attempt(2);

        assert!(delay1 > delay0);
        assert!(delay2 > delay1);
    }

    #[test]
    fn test_retry_config_max_delay_cap() {
        let config = RetryConfig::default();

        // Very high attempt number should be capped
        let delay = config.delay_for_attempt(100);

        assert_eq!(delay, config.max_delay);
    }

    #[test]
    fn test_host_config_retry_config() {
        let host_config = HostConfig::new();
        let retry_config = host_config.retry_config();

        assert_eq!(retry_config.max_retries, DEFAULT_RETRIES);
        assert!(retry_config.exponential_backoff);
    }

    #[test]
    fn test_host_config_custom_retry() {
        let mut host_config = HostConfig::new();
        host_config.retries = Some(5);
        host_config.retry_delay = Some(2);

        let retry_config = host_config.retry_config();

        assert_eq!(retry_config.max_retries, 5);
        assert_eq!(retry_config.retry_delay, Duration::from_secs(2));
    }
}

// ============================================================================
// UNIT TESTS - EXECUTE OPTIONS
// ============================================================================

mod execute_options_tests {
    use super::*;

    #[test]
    fn test_execute_options_default() {
        let options = ExecuteOptions::default();

        assert!(options.cwd.is_none());
        assert!(options.env.is_empty());
        assert!(options.timeout.is_none());
        assert!(!options.escalate);
    }

    #[test]
    fn test_execute_options_with_cwd() {
        let options = ExecuteOptions::new().with_cwd("/tmp");
        assert_eq!(options.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn test_execute_options_with_env() {
        let options = ExecuteOptions::new()
            .with_env("KEY1", "value1")
            .with_env("KEY2", "value2");

        assert_eq!(options.env.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(options.env.get("KEY2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_execute_options_with_timeout() {
        let options = ExecuteOptions::new().with_timeout(60);
        assert_eq!(options.timeout, Some(60));
    }

    #[test]
    fn test_execute_options_with_escalation() {
        let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    #[test]
    fn test_execute_options_with_escalation_default_user() {
        let options = ExecuteOptions::new().with_escalation(None);

        assert!(options.escalate);
        assert!(options.escalate_user.is_none()); // Will default to root at execution
    }
}

// ============================================================================
// UNIT TESTS - TRANSFER OPTIONS
// ============================================================================

mod transfer_options_tests {
    use super::*;

    #[test]
    fn test_transfer_options_default() {
        let options = TransferOptions::default();

        assert!(options.mode.is_none());
        assert!(options.owner.is_none());
        assert!(options.group.is_none());
        assert!(!options.create_dirs);
        assert!(!options.backup);
    }

    #[test]
    fn test_transfer_options_with_mode() {
        let options = TransferOptions::new().with_mode(0o755);
        assert_eq!(options.mode, Some(0o755));
    }

    #[test]
    fn test_transfer_options_with_owner() {
        let options = TransferOptions::new().with_owner("testuser");
        assert_eq!(options.owner, Some("testuser".to_string()));
    }

    #[test]
    fn test_transfer_options_with_group() {
        let options = TransferOptions::new().with_group("testgroup");
        assert_eq!(options.group, Some("testgroup".to_string()));
    }

    #[test]
    fn test_transfer_options_with_create_dirs() {
        let options = TransferOptions::new().with_create_dirs();
        assert!(options.create_dirs);
    }

    #[test]
    fn test_transfer_options_chained() {
        let options = TransferOptions::new()
            .with_mode(0o644)
            .with_owner("admin")
            .with_group("staff")
            .with_create_dirs();

        assert_eq!(options.mode, Some(0o644));
        assert_eq!(options.owner, Some("admin".to_string()));
        assert_eq!(options.group, Some("staff".to_string()));
        assert!(options.create_dirs);
    }
}

// ============================================================================
// UNIT TESTS - MOCK CONNECTION BEHAVIOR
// ============================================================================

mod mock_connection {
    use super::*;

    #[tokio::test]
    async fn test_mock_connection_identifier() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        assert_eq!(conn.identifier(), "russh:admin@example.com:22");
    }

    #[tokio::test]
    async fn test_mock_connection_is_alive() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        assert!(conn.is_alive().await);

        conn.kill();

        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_mock_connection_command_tracking() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        conn.execute("cmd1", None).await.unwrap();
        conn.execute("cmd2", None).await.unwrap();
        conn.execute("cmd3", None).await.unwrap();

        assert_eq!(conn.command_count(), 3);
        assert_eq!(
            conn.get_commands(),
            vec!["cmd1".to_string(), "cmd2".to_string(), "cmd3".to_string()]
        );
    }

    #[tokio::test]
    async fn test_mock_connection_virtual_filesystem() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let remote_path = PathBuf::from("/remote/file.txt");
        let content = b"test content";

        conn.upload_content(content, &remote_path, None)
            .await
            .unwrap();

        assert!(conn.path_exists(&remote_path).await.unwrap());

        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(downloaded, content);
    }

    #[tokio::test]
    async fn test_mock_connection_latency_simulation() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_latency_ms(50);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let start = std::time::Instant::now();
        conn.execute("echo test", None).await.unwrap();
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_mock_connection_close() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        assert!(conn.is_alive().await);

        conn.close().await.unwrap();

        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_mock_connection_stat() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let remote_path = PathBuf::from("/remote/file.txt");
        let content = b"test content here";

        conn.upload_content(content, &remote_path, None)
            .await
            .unwrap();

        let stat = conn.stat(&remote_path).await.unwrap();

        assert_eq!(stat.size, content.len() as u64);
        assert!(stat.is_file);
        assert!(!stat.is_dir);
    }
}

// ============================================================================
// INTEGRATION TEST STUBS (REQUIRE REAL SSH INFRASTRUCTURE)
// ============================================================================

#[cfg(feature = "russh")]
mod integration_tests {
    use super::*;

    /// Test connecting to a real SSH server
    ///
    /// Requires environment variables:
    /// - RUSTIBLE_SSH_TEST_HOST: SSH server hostname
    /// - RUSTIBLE_SSH_TEST_PORT: SSH server port (default: 22)
    /// - RUSTIBLE_SSH_TEST_USER: SSH username (default: testuser)
    #[tokio::test]
    async fn test_russh_connect() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");

        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect to SSH server");

        assert!(conn.is_alive().await);
        conn.close().await.unwrap();
    }

    /// Test executing a command on a real SSH server
    #[tokio::test]
    async fn test_russh_execute() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");

        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect");

        let result = conn.execute("echo 'Hello, World!'", None).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Hello, World!"));

        let whoami = conn.execute("whoami", None).await.unwrap();
        assert!(whoami.success);
        assert!(whoami.stdout.contains(&user));

        conn.close().await.unwrap();
    }

    /// Test uploading a file via SFTP to a real SSH server
    #[tokio::test]
    async fn test_russh_upload() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect");

        // Create a local file to upload
        let local_file = temp_dir.path().join("upload_test.txt");
        std::fs::write(&local_file, b"Upload test content").unwrap();

        let remote_path = PathBuf::from("/tmp/rustible_upload_test.txt");

        conn.upload(&local_file, &remote_path, None).await.unwrap();

        // Verify file exists on remote
        assert!(conn.path_exists(&remote_path).await.unwrap());

        // Clean up
        conn.execute("rm /tmp/rustible_upload_test.txt", None)
            .await
            .unwrap();
        conn.close().await.unwrap();
    }

    /// Test downloading a file via SFTP from a real SSH server
    #[tokio::test]
    async fn test_russh_download() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect");

        // Create a remote file to download
        conn.execute(
            "echo 'Download test content' > /tmp/rustible_download_test.txt",
            None,
        )
        .await
        .unwrap();

        let remote_path = PathBuf::from("/tmp/rustible_download_test.txt");
        let local_path = temp_dir.path().join("downloaded.txt");

        conn.download(&remote_path, &local_path).await.unwrap();

        assert!(local_path.exists());
        let content = std::fs::read_to_string(&local_path).unwrap();
        assert!(content.contains("Download test content"));

        // Clean up
        conn.execute("rm /tmp/rustible_download_test.txt", None)
            .await
            .unwrap();
        conn.close().await.unwrap();
    }

    /// Test SFTP operations on a real SSH server
    #[tokio::test]
    async fn test_russh_sftp_operations() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");

        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect");

        // Test upload_content
        let content = b"SFTP test content";
        let remote_path = PathBuf::from("/tmp/rustible_sftp_test.txt");

        conn.upload_content(content, &remote_path, None)
            .await
            .unwrap();

        // Test path_exists
        assert!(conn.path_exists(&remote_path).await.unwrap());

        // Test stat
        let stat = conn.stat(&remote_path).await.unwrap();
        assert!(stat.is_file);
        assert_eq!(stat.size, content.len() as u64);

        // Test download_content
        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(downloaded, content);

        // Test is_directory
        assert!(!conn.is_directory(&remote_path).await.unwrap());
        assert!(conn.is_directory(&PathBuf::from("/tmp")).await.unwrap());

        // Clean up
        conn.execute("rm /tmp/rustible_sftp_test.txt", None)
            .await
            .unwrap();
        conn.close().await.unwrap();
    }

    /// Test SSH connection with key authentication
    #[tokio::test]
    async fn test_russh_key_authentication() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");
        let key_path = get_ssh_test_key();

        if !key_path.exists() {
            eprintln!("Skipping: SSH key not found at {:?}", key_path);
            return;
        }

        // Configure host with identity file
        let host_config = HostConfig::new()
            .hostname(&host)
            .port(port)
            .user(&user)
            .identity_file(key_path.to_string_lossy());

        let conn = RusshConnection::connect(
            &host,
            port,
            &user,
            Some(host_config),
            &ConnectionConfig::default(),
        )
        .await
        .expect("Failed to connect to SSH server with key auth");

        assert!(conn.is_alive().await);

        let result = conn.execute("whoami", None).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.trim().contains(&user));

        conn.close().await.unwrap();
    }

    /// Test SSH connection with password authentication
    #[tokio::test]
    async fn test_russh_password_authentication() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        // TODO: Test password authentication
        eprintln!("Would test password authentication");
    }

    /// Test SSH connection through a proxy/jump host
    ///
    /// Requires environment variables:
    /// - RUSTIBLE_SSH_TEST_JUMP_HOST: SSH jump host hostname
    /// - RUSTIBLE_SSH_TEST_JUMP_PORT: SSH jump host port (default: 22)
    /// - RUSTIBLE_SSH_TEST_JUMP_USER: SSH jump host username (default: same as target)
    /// - RUSTIBLE_SSH_TEST_JUMP_KEY: SSH jump host private key path (optional)
    #[tokio::test]
    async fn test_russh_proxy_jump() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");
        let (jump_host, jump_port, jump_user) =
            match get_ssh_test_jump_config("RUSTIBLE_SSH_TEST_JUMP", &user) {
                Some(config) => config,
                None => {
                    eprintln!("Skipping: No jump host configured");
                    return;
                }
            };

        let target_key = get_ssh_test_key();
        let jump_key = get_ssh_test_jump_key("RUSTIBLE_SSH_TEST_JUMP");

        let mut config = ConnectionConfig::default();

        let mut jump_config = HostConfig::new()
            .hostname(&jump_host)
            .port(jump_port)
            .user(&jump_user);
        if jump_key.exists() {
            jump_config = jump_config.identity_file(jump_key.to_string_lossy().to_string());
        }
        config.add_host("jump1", jump_config);

        let mut target_config = HostConfig::new().hostname(&host).port(port).user(&user);
        if target_key.exists() {
            target_config = target_config.identity_file(target_key.to_string_lossy().to_string());
        }
        target_config.proxy_jump = Some("jump1".to_string());
        config.add_host(&host, target_config);

        let conn = RusshConnection::connect(&host, port, &user, None, &config)
            .await
            .expect("Failed to connect via jump host");

        let result = conn.execute("whoami", None).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.trim().contains(&user));

        conn.close().await.unwrap();
    }

    /// Test SSH connection through a multi-hop proxy/jump chain
    ///
    /// Requires environment variables:
    /// - RUSTIBLE_SSH_TEST_JUMP_HOST: First jump host hostname
    /// - RUSTIBLE_SSH_TEST_JUMP_PORT: First jump host port (default: 22)
    /// - RUSTIBLE_SSH_TEST_JUMP_USER: First jump host username (default: same as target)
    /// - RUSTIBLE_SSH_TEST_JUMP_KEY: First jump host private key path (optional)
    /// - RUSTIBLE_SSH_TEST_JUMP2_HOST: Second jump host hostname
    /// - RUSTIBLE_SSH_TEST_JUMP2_PORT: Second jump host port (default: 22)
    /// - RUSTIBLE_SSH_TEST_JUMP2_USER: Second jump host username (default: same as target)
    /// - RUSTIBLE_SSH_TEST_JUMP2_KEY: Second jump host private key path (optional)
    #[tokio::test]
    async fn test_russh_proxy_jump_multi() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");
        let (jump_host, jump_port, jump_user) =
            match get_ssh_test_jump_config("RUSTIBLE_SSH_TEST_JUMP", &user) {
                Some(config) => config,
                None => {
                    eprintln!("Skipping: No jump host configured");
                    return;
                }
            };
        let (jump2_host, jump2_port, jump2_user) =
            match get_ssh_test_jump_config("RUSTIBLE_SSH_TEST_JUMP2", &user) {
                Some(config) => config,
                None => {
                    eprintln!("Skipping: No second jump host configured");
                    return;
                }
            };

        let target_key = get_ssh_test_key();
        let jump_key = get_ssh_test_jump_key("RUSTIBLE_SSH_TEST_JUMP");
        let jump2_key = get_ssh_test_jump_key("RUSTIBLE_SSH_TEST_JUMP2");

        let mut config = ConnectionConfig::default();

        let mut jump_config = HostConfig::new()
            .hostname(&jump_host)
            .port(jump_port)
            .user(&jump_user);
        if jump_key.exists() {
            jump_config = jump_config.identity_file(jump_key.to_string_lossy().to_string());
        }
        config.add_host("jump1", jump_config);

        let mut jump2_config = HostConfig::new()
            .hostname(&jump2_host)
            .port(jump2_port)
            .user(&jump2_user);
        if jump2_key.exists() {
            jump2_config = jump2_config.identity_file(jump2_key.to_string_lossy().to_string());
        }
        config.add_host("jump2", jump2_config);

        let mut target_config = HostConfig::new().hostname(&host).port(port).user(&user);
        if target_key.exists() {
            target_config = target_config.identity_file(target_key.to_string_lossy().to_string());
        }
        target_config.proxy_jump = Some("jump1,jump2".to_string());
        config.add_host(&host, target_config);

        let conn = RusshConnection::connect(&host, port, &user, None, &config)
            .await
            .expect("Failed to connect via multi-hop jump hosts");

        let result = conn.execute("whoami", None).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.trim().contains(&user));

        conn.close().await.unwrap();
    }

    /// Test connection retry behavior
    #[tokio::test]
    async fn test_russh_connection_retry() {
        // TODO: Test connection retry with real infrastructure
        eprintln!("Would test connection retry");
    }

    /// Test concurrent command execution
    #[tokio::test]
    async fn test_russh_concurrent_execution() {
        if !has_ssh_infrastructure() {
            eprintln!("Skipping: No SSH infrastructure available");
            return;
        }

        let (host, port, user) = get_ssh_test_config().expect("SSH test config required");

        eprintln!("Connecting to {}:{} as {}", host, port, user);
        let conn = RusshConnection::connect(&host, port, &user, None, &ConnectionConfig::default())
            .await
            .expect("Failed to connect");

        // Wrap in Arc for shared access in tasks
        let conn = Arc::new(conn);

        // 1. Test standard concurrent execution via Arc sharing
        eprintln!("Testing standard concurrent execution (tokio::spawn)...");
        let start = std::time::Instant::now();
        let mut handles = vec![];

        // Run 5 concurrent commands that sleep for 1 second each
        // If sequential: ~5s
        // If parallel: ~1s + overhead
        for i in 0..5 {
            let c = conn.clone();
            handles.push(tokio::spawn(async move {
                c.execute(&format!("sleep 1 && echo done_{}", i), None)
                    .await
            }));
        }

        let mut success_count = 0;
        for handle in handles {
            let res = handle
                .await
                .expect("Task join failed")
                .expect("Command execution failed");

            if res.success {
                success_count += 1;
            } else {
                eprintln!("Command failed: {}", res.stderr);
            }
        }
        assert_eq!(success_count, 5, "Not all concurrent commands succeeded");

        let elapsed = start.elapsed();
        eprintln!("Standard concurrent execution took {:?}", elapsed);

        // Assert it ran in parallel (allowing overhead, but clearly < 3s)
        assert!(
            elapsed.as_secs() < 3,
            "Execution took {:?} which implies sequential execution (expected < 3s)",
            elapsed
        );
        // Also assert it took at least 1s (sanity check that it actually slept)
        assert!(
            elapsed.as_millis() >= 1000,
            "Execution was too fast, sleep didn't work: {:?}",
            elapsed
        );

        // 2. Test execute_batch (channel multiplexing)
        eprintln!("Testing batch execution (channel multiplexing)...");
        let commands = vec![
            "sleep 1 && echo batch1",
            "sleep 1 && echo batch2",
            "sleep 1 && echo batch3",
        ];

        let start = std::time::Instant::now();
        let results = conn.execute_batch(&commands, None).await;
        let elapsed = start.elapsed();

        eprintln!("Batch execution took {:?}", elapsed);

        assert_eq!(results.len(), 3);
        for (i, res) in results.into_iter().enumerate() {
            let r = res.expect("Batch command failed");
            assert!(
                r.success,
                "Batch command {} failed: {}",
                i,
                r.combined_output()
            );
        }

        // Should also be parallel
        assert!(
            elapsed.as_secs() < 3,
            "Batch execution took {:?} which implies sequential execution (expected < 3s)",
            elapsed
        );
        assert!(
            elapsed.as_millis() >= 1000,
            "Batch execution was too fast: {:?}",
            elapsed
        );

        conn.close().await.unwrap();
    }
}

// ============================================================================
// UNIT TESTS THAT RUN IN CI
// ============================================================================

mod ci_safe_tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_workflow_complete() {
        let temp_dir = setup_test_fixtures();
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // 1. Check connection
        assert!(conn.is_alive().await);

        // 2. Execute commands
        conn.set_command_result(
            "whoami",
            CommandResult::success("admin".to_string(), String::new()),
        );
        let whoami = conn.execute("whoami", None).await.unwrap();
        assert_eq!(whoami.stdout, "admin");

        // 3. Upload a file
        let remote_file = PathBuf::from("/remote/config.txt");
        conn.upload_content(b"key=value", &remote_file, None)
            .await
            .unwrap();

        // 4. Verify file exists
        assert!(conn.path_exists(&remote_file).await.unwrap());

        // 5. Get file stats
        let stat = conn.stat(&remote_file).await.unwrap();
        assert!(stat.is_file);

        // 6. Download file
        let local_file = temp_dir.path().join("config.txt");
        conn.download(&remote_file, &local_file).await.unwrap();
        assert!(local_file.exists());

        // 7. Close connection
        conn.close().await.unwrap();
        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_mock_concurrent_operations() {
        let conn = Arc::new(MockRusshConnection::new("example.com", 22, "admin"));
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let mut handles = vec![];

        for i in 0..20 {
            let conn_clone = conn.clone();
            let handle =
                tokio::spawn(async move { conn_clone.execute(&format!("cmd_{}", i), None).await });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 20);
    }

    #[test]
    fn test_fixtures_setup() {
        let temp_dir = setup_test_fixtures();

        let ssh_dir = temp_dir.path().join(".ssh");
        assert!(ssh_dir.exists());
        assert!(ssh_dir.join("config").exists());
        assert!(ssh_dir.join("test_key").exists());
    }

    #[test]
    fn test_ci_skip_detection() {
        // This test always passes but documents the skip condition
        let _in_ci = should_skip_in_ci();
        // CI detection works regardless of environment
    }

    #[test]
    fn test_ssh_infrastructure_detection() {
        let has_infra = has_ssh_infrastructure();
        let config = get_ssh_test_config();

        // If we have infrastructure, we should have config
        if has_infra {
            assert!(config.is_some());
        }
    }
}
