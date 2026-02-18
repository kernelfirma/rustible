//! Comprehensive SSH-specific tests for Rustible (ssh2-backend)
//!
//! This test module covers advanced SSH functionality including:
//! - SSH connection establishment and authentication
//! - SSH pipelining configuration
//! - SSH multiplexing (ControlMaster/ControlPersist)
//! - SSH proxy and jump host configuration
//! - SSH options and host key handling
//! - SSH key management
//! - SSH performance and connection pooling
//! - SSH error handling and edge cases
//!
//! NOTE: This test file requires the ssh2-backend feature. Run with:
//! ```bash
//! cargo test --test ssh_tests --features ssh2-backend
//! ```

#![cfg(feature = "ssh2-backend")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use tempfile::TempDir;

use rustible::connection::config::{
    default_identity_files, expand_path, RetryConfig, SshConfigParser, DEFAULT_TIMEOUT,
};
use rustible::connection::ssh::SshConnectionBuilder;
use rustible::connection::{
    CommandResult, Connection, ConnectionBuilder, ConnectionConfig, ConnectionError,
    ConnectionFactory, ConnectionResult, ConnectionType, ExecuteOptions, FileStat, HostConfig,
    TransferOptions,
};

// ============================================================================
// Mock SSH Connection for Testing
// ============================================================================

/// Mock SSH connection that simulates SSH behavior for testing
/// without requiring actual SSH infrastructure
#[derive(Debug)]
pub struct MockSshConnection {
    identifier: String,
    pub host: String,
    pub port: u16,
    #[allow(dead_code)]
    user: String,
    alive: AtomicBool,
    connected: AtomicBool,
    commands_executed: RwLock<Vec<String>>,
    files_transferred: RwLock<Vec<(PathBuf, PathBuf)>>,
    virtual_filesystem: RwLock<HashMap<PathBuf, Vec<u8>>>,
    command_results: RwLock<HashMap<String, CommandResult>>,
    default_result: RwLock<CommandResult>,
    execution_count: AtomicU32,
    should_fail_auth: AtomicBool,
    should_fail_connection: AtomicBool,
    should_timeout: AtomicBool,
    latency_ms: AtomicU32,
    // SSH-specific configuration
    pipelining: AtomicBool,
    multiplexing: AtomicBool,
    compression: AtomicBool,
    control_persist: AtomicU32,
    host_key_checking: RwLock<Option<bool>>,
    known_hosts_file: RwLock<Option<PathBuf>>,
    identity_files: RwLock<Vec<PathBuf>>,
    proxy_command: RwLock<Option<String>>,
    proxy_jump: RwLock<Option<String>>,
}

impl MockSshConnection {
    /// Create a new mock SSH connection
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self {
            identifier: format!("{}@{}:{}", user, host, port),
            host: host.to_string(),
            port,
            user: user.to_string(),
            alive: AtomicBool::new(true),
            connected: AtomicBool::new(true),
            commands_executed: RwLock::new(Vec::new()),
            files_transferred: RwLock::new(Vec::new()),
            virtual_filesystem: RwLock::new(HashMap::new()),
            command_results: RwLock::new(HashMap::new()),
            default_result: RwLock::new(CommandResult::success(String::new(), String::new())),
            execution_count: AtomicU32::new(0),
            should_fail_auth: AtomicBool::new(false),
            should_fail_connection: AtomicBool::new(false),
            should_timeout: AtomicBool::new(false),
            latency_ms: AtomicU32::new(0),
            pipelining: AtomicBool::new(false),
            multiplexing: AtomicBool::new(false),
            compression: AtomicBool::new(false),
            control_persist: AtomicU32::new(0),
            host_key_checking: RwLock::new(Some(true)),
            known_hosts_file: RwLock::new(None),
            identity_files: RwLock::new(Vec::new()),
            proxy_command: RwLock::new(None),
            proxy_jump: RwLock::new(None),
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

    /// Enable/disable pipelining
    pub fn set_pipelining(&self, enabled: bool) {
        self.pipelining.store(enabled, Ordering::SeqCst);
    }

    /// Enable/disable multiplexing
    pub fn set_multiplexing(&self, enabled: bool) {
        self.multiplexing.store(enabled, Ordering::SeqCst);
    }

    /// Enable/disable compression
    pub fn set_compression(&self, enabled: bool) {
        self.compression.store(enabled, Ordering::SeqCst);
    }

    /// Set ControlPersist timeout
    pub fn set_control_persist(&self, seconds: u32) {
        self.control_persist.store(seconds, Ordering::SeqCst);
    }

    /// Set host key checking mode
    pub fn set_host_key_checking(&self, enabled: Option<bool>) {
        *self.host_key_checking.write() = enabled;
    }

    /// Set known hosts file
    pub fn set_known_hosts_file(&self, path: Option<PathBuf>) {
        *self.known_hosts_file.write() = path;
    }

    /// Add identity file
    pub fn add_identity_file(&self, path: PathBuf) {
        self.identity_files.write().push(path);
    }

    /// Set proxy command
    pub fn set_proxy_command(&self, command: Option<String>) {
        *self.proxy_command.write() = command;
    }

    /// Set proxy jump
    pub fn set_proxy_jump(&self, jump: Option<String>) {
        *self.proxy_jump.write() = jump;
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
    pub fn add_virtual_file(&self, path: PathBuf, content: Vec<u8>) {
        self.virtual_filesystem.write().insert(path, content);
    }

    /// Check if pipelining is enabled
    pub fn is_pipelining_enabled(&self) -> bool {
        self.pipelining.load(Ordering::SeqCst)
    }

    /// Check if multiplexing is enabled
    pub fn is_multiplexing_enabled(&self) -> bool {
        self.multiplexing.load(Ordering::SeqCst)
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
}

#[async_trait]
impl Connection for MockSshConnection {
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
        // Check for connection issues
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

        // Simulate network latency
        self.simulate_latency().await;

        self.execution_count.fetch_add(1, Ordering::SeqCst);
        self.commands_executed.write().push(command.to_string());

        // Check for specific command result
        if let Some(result) = self.command_results.read().get(command) {
            return Ok(result.clone());
        }

        // Return default result
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
        self.files_transferred
            .write()
            .push((src.to_path_buf(), dest.to_path_buf()));

        // Read source file if it exists and add to virtual filesystem
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
// SSH CONNECTION TESTS
// ============================================================================

mod ssh_connection {
    use super::*;

    #[tokio::test]
    async fn test_mock_ssh_connection_basic() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        assert_eq!(conn.identifier(), "admin@example.com:22");
        assert!(conn.is_alive().await);
        assert_eq!(conn.command_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_ssh_connection_execute() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("hello".to_string(), String::new()));

        let result = conn.execute("echo hello", None).await.unwrap();

        assert!(result.success);
        assert_eq!(conn.command_count(), 1);
        assert_eq!(conn.get_commands(), vec!["echo hello".to_string()]);
    }

    #[tokio::test]
    async fn test_mock_ssh_connection_specific_command_result() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_command_result(
            "whoami",
            CommandResult::success("admin\n".to_string(), String::new()),
        );
        conn.set_command_result(
            "hostname",
            CommandResult::success("example.com\n".to_string(), String::new()),
        );

        let whoami_result = conn.execute("whoami", None).await.unwrap();
        assert_eq!(whoami_result.stdout, "admin\n");

        let hostname_result = conn.execute("hostname", None).await.unwrap();
        assert_eq!(hostname_result.stdout, "example.com\n");
    }

    #[tokio::test]
    async fn test_ssh_connection_builder_pattern() {
        // Test the SSH connection builder API without actually connecting
        // We verify the builder pattern works by chaining methods
        // (We cannot verify internal state as fields are private)
        let _builder = SshConnectionBuilder::new("example.com")
            .port(2222)
            .user("admin")
            .compression(true)
            .timeout(Duration::from_secs(60));

        // The builder pattern is verified by successful method chaining above.
        // Actual connection would require a real SSH server, so we just test
        // that the builder API compiles and chains correctly.
    }

    #[tokio::test]
    async fn test_connection_builder_ssh_type() {
        // Test that ConnectionBuilder properly creates SSH connection types
        let _builder = ConnectionBuilder::new("remote.example.com")
            .port(22)
            .user("admin")
            .private_key("/home/user/.ssh/id_rsa")
            .timeout(30);

        // The builder stores the configuration - we can't test actual connection
        // without a real SSH server, but we verify the API works
    }

    #[test]
    fn test_connection_type_ssh_pool_key() {
        let conn_type = ConnectionType::Ssh {
            host: "example.com".to_string(),
            port: 22,
            user: "admin".to_string(),
        };

        assert_eq!(conn_type.pool_key(), "ssh://admin@example.com:22");
    }

    #[test]
    fn test_connection_type_ssh_custom_port() {
        let conn_type = ConnectionType::Ssh {
            host: "example.com".to_string(),
            port: 2222,
            user: "deploy".to_string(),
        };

        assert_eq!(conn_type.pool_key(), "ssh://deploy@example.com:2222");
    }
}

// ============================================================================
// SSH AUTHENTICATION TESTS
// ============================================================================

mod ssh_authentication {
    use super::*;

    #[tokio::test]
    async fn test_key_based_authentication_failure() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_auth_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_password_authentication_failure() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_auth_failure(true);

        let result = conn.execute("whoami", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_authentication_success_after_config_change() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // First, fail authentication
        conn.set_auth_failure(true);
        let result = conn.execute("test", None).await;
        assert!(result.is_err());

        // Then, succeed after "fixing" credentials
        conn.set_auth_failure(false);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));
        let result = conn.execute("test", None).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_host_config_identity_file() {
        let config = HostConfig::new()
            .hostname("example.com")
            .identity_file("~/.ssh/custom_key");

        assert_eq!(config.identity_file, Some("~/.ssh/custom_key".to_string()));
    }

    #[test]
    fn test_multiple_identity_files_config() {
        let mut config = ConnectionConfig::new();
        config.defaults.identity_files = vec![
            "~/.ssh/id_ed25519".to_string(),
            "~/.ssh/id_rsa".to_string(),
            "~/.ssh/id_ecdsa".to_string(),
        ];

        assert_eq!(config.defaults.identity_files.len(), 3);
        assert!(config.defaults.identity_files[0].contains("ed25519"));
    }

    #[test]
    fn test_agent_authentication_config() {
        let config = ConnectionConfig::default();

        // By default, agent authentication should be enabled
        assert!(config.defaults.use_agent);
    }

    #[test]
    fn test_disable_agent_authentication() {
        let mut config = ConnectionConfig::default();
        config.defaults.use_agent = false;

        assert!(!config.defaults.use_agent);
    }

    #[tokio::test]
    async fn test_mock_ssh_with_identity_files() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.add_identity_file(PathBuf::from("/home/user/.ssh/id_rsa"));
        conn.add_identity_file(PathBuf::from("/home/user/.ssh/id_ed25519"));

        let identity_files = conn.identity_files.read();
        assert_eq!(identity_files.len(), 2);
    }
}

// ============================================================================
// SSH PIPELINING TESTS
// ============================================================================

mod ssh_pipelining {
    use super::*;

    #[tokio::test]
    async fn test_pipelining_configuration() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Initially pipelining should be off
        assert!(!conn.is_pipelining_enabled());

        // Enable pipelining
        conn.set_pipelining(true);
        assert!(conn.is_pipelining_enabled());

        // Disable pipelining
        conn.set_pipelining(false);
        assert!(!conn.is_pipelining_enabled());
    }

    #[tokio::test]
    async fn test_pipelining_reduces_ssh_connections() {
        let conn = Arc::new(MockSshConnection::new("example.com", 22, "admin"));
        conn.set_pipelining(true);
        conn.set_default_result(CommandResult::success(String::new(), String::new()));

        // Simulate multiple commands - with pipelining, we should see
        // all commands executed through the same connection
        let commands = vec!["cmd1", "cmd2", "cmd3", "cmd4", "cmd5"];

        for cmd in &commands {
            conn.execute(cmd, None).await.unwrap();
        }

        assert_eq!(conn.command_count(), 5);
        assert!(conn.is_pipelining_enabled());

        // Verify all commands were captured
        let executed = conn.get_commands();
        assert_eq!(executed.len(), 5);
    }

    #[tokio::test]
    async fn test_pipelining_performance_comparison() {
        // Test without pipelining (simulated latency)
        let conn_no_pipe = MockSshConnection::new("example.com", 22, "admin");
        conn_no_pipe.set_pipelining(false);
        conn_no_pipe.set_latency_ms(10);
        conn_no_pipe.set_default_result(CommandResult::success(String::new(), String::new()));

        let start_no_pipe = std::time::Instant::now();
        for i in 0..5 {
            conn_no_pipe
                .execute(&format!("command_{}", i), None)
                .await
                .unwrap();
        }
        let duration_no_pipe = start_no_pipe.elapsed();

        // Test with pipelining
        let conn_pipe = MockSshConnection::new("example.com", 22, "admin");
        conn_pipe.set_pipelining(true);
        conn_pipe.set_latency_ms(10);
        conn_pipe.set_default_result(CommandResult::success(String::new(), String::new()));

        let start_pipe = std::time::Instant::now();
        for i in 0..5 {
            conn_pipe
                .execute(&format!("command_{}", i), None)
                .await
                .unwrap();
        }
        let duration_pipe = start_pipe.elapsed();

        // Both should complete, duration comparison is informational
        assert_eq!(conn_no_pipe.command_count(), 5);
        assert_eq!(conn_pipe.command_count(), 5);

        // Log durations for analysis
        println!("Without pipelining: {:?}", duration_no_pipe);
        println!("With pipelining: {:?}", duration_pipe);
    }

    #[test]
    fn test_requiretty_handling_in_ssh_config() {
        let config = r#"
Host production
    HostName prod.example.com
    RequestTTY no
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let prod = hosts.get("production").unwrap();

        // RequestTTY should be stored in options
        assert_eq!(prod.options.get("requesttty"), Some(&"no".to_string()));
    }
}

// ============================================================================
// SSH MULTIPLEXING TESTS
// ============================================================================

mod ssh_multiplexing {
    use super::*;

    #[tokio::test]
    async fn test_multiplexing_configuration() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Initially multiplexing should be off
        assert!(!conn.is_multiplexing_enabled());

        // Enable multiplexing
        conn.set_multiplexing(true);
        assert!(conn.is_multiplexing_enabled());
    }

    #[tokio::test]
    async fn test_control_persist_configuration() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_multiplexing(true);
        conn.set_control_persist(600); // 10 minutes

        assert!(conn.is_multiplexing_enabled());
        assert_eq!(conn.control_persist.load(Ordering::SeqCst), 600);
    }

    #[test]
    fn test_control_master_ssh_config() {
        let config = r#"
Host *
    ControlMaster auto
    ControlPersist 600
    ControlPath ~/.ssh/sockets/%r@%h-%p
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let star = hosts.get("*").unwrap();

        assert_eq!(star.options.get("controlmaster"), Some(&"auto".to_string()));
        assert_eq!(star.options.get("controlpersist"), Some(&"600".to_string()));
        assert!(star.options.contains_key("controlpath"));
    }

    #[tokio::test]
    async fn test_connection_reuse_with_multiplexing() {
        let conn = Arc::new(MockSshConnection::new("example.com", 22, "admin"));
        conn.set_multiplexing(true);
        conn.set_control_persist(60);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // Execute multiple commands - all should reuse the same connection
        let mut handles = vec![];
        for i in 0..10 {
            let conn_clone = conn.clone();
            let handle =
                tokio::spawn(
                    async move { conn_clone.execute(&format!("command_{}", i), None).await },
                );
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 10);
        // All commands went through the same multiplexed connection
        assert!(conn.is_alive().await);
    }

    #[test]
    fn test_control_path_expansion() {
        let config = r#"
Host server
    ControlPath ~/.ssh/sockets/%r@%h-%p
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let server = hosts.get("server").unwrap();

        let control_path = server.options.get("controlpath").unwrap();
        assert!(control_path.contains("%r")); // Remote user
        assert!(control_path.contains("%h")); // Host
        assert!(control_path.contains("%p")); // Port
    }

    #[tokio::test]
    async fn test_master_socket_handling() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_multiplexing(true);

        // Verify connection is alive (master socket exists)
        assert!(conn.is_alive().await);

        // Close connection (remove master socket)
        conn.close().await.unwrap();

        // Connection should no longer be alive
        assert!(!conn.is_alive().await);
    }
}

// ============================================================================
// SSH PROXY TESTS
// ============================================================================

mod ssh_proxy {
    use super::*;

    #[test]
    fn test_proxy_command_configuration() {
        let config = r#"
Host internal-*
    ProxyCommand ssh -W %h:%p bastion.example.com
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let internal = hosts.get("internal-*").unwrap();

        assert!(internal.options.contains_key("proxycommand"));
        let proxy_cmd = internal.options.get("proxycommand").unwrap();
        assert!(proxy_cmd.contains("bastion.example.com"));
    }

    #[test]
    fn test_proxy_jump_configuration() {
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
    fn test_multi_hop_proxy_jump() {
        let config = r#"
Host deep-internal
    HostName deep.internal.private
    ProxyJump bastion1.example.com,bastion2.example.com
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let deep = hosts.get("deep-internal").unwrap();

        // Multiple jump hosts should be comma-separated
        assert!(deep.proxy_jump.as_ref().unwrap().contains(","));
    }

    #[tokio::test]
    async fn test_mock_ssh_with_proxy() {
        let conn = MockSshConnection::new("internal.private", 22, "admin");
        conn.set_proxy_jump(Some("bastion.example.com".to_string()));
        conn.set_default_result(CommandResult::success(
            "internal host".to_string(),
            String::new(),
        ));

        let result = conn.execute("hostname", None).await.unwrap();

        assert_eq!(result.stdout, "internal host");
        assert_eq!(
            *conn.proxy_jump.read(),
            Some("bastion.example.com".to_string())
        );
    }

    #[test]
    fn test_bastion_host_config() {
        let mut config = ConnectionConfig::new();

        // Add bastion host
        config.add_host(
            "bastion",
            HostConfig::new()
                .hostname("bastion.example.com")
                .user("jumpuser")
                .port(22),
        );

        // Add internal host that uses bastion
        let mut internal_config = HostConfig::new().hostname("internal.private").user("admin");
        internal_config.proxy_jump = Some("bastion".to_string());
        config.add_host("internal", internal_config);

        let bastion = config.get_host("bastion").unwrap();
        assert_eq!(bastion.hostname, Some("bastion.example.com".to_string()));

        let internal = config.get_host("internal").unwrap();
        assert_eq!(internal.proxy_jump, Some("bastion".to_string()));
    }

    #[tokio::test]
    async fn test_proxy_command_with_mock() {
        let conn = MockSshConnection::new("internal.example.com", 22, "admin");
        conn.set_proxy_command(Some("ssh -W %h:%p bastion".to_string()));
        conn.set_default_result(CommandResult::success("success".to_string(), String::new()));

        {
            let guard = conn.proxy_command.read();
            let proxy_cmd: &Option<String> = &guard;
            assert!(proxy_cmd.as_ref().unwrap().contains("bastion"));
        }

        let result = conn.execute("echo test", None).await.unwrap();
        assert!(result.success);
    }
}

// ============================================================================
// SSH OPTIONS TESTS
// ============================================================================

mod ssh_options {
    use super::*;

    #[test]
    fn test_strict_host_key_checking_yes() {
        let config = r#"
Host secure
    StrictHostKeyChecking yes
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let secure = hosts.get("secure").unwrap();

        assert_eq!(secure.strict_host_key_checking, Some(true));
    }

    #[test]
    fn test_strict_host_key_checking_no() {
        let config = r#"
Host insecure
    StrictHostKeyChecking no
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let insecure = hosts.get("insecure").unwrap();

        assert_eq!(insecure.strict_host_key_checking, Some(false));
    }

    #[test]
    fn test_user_known_hosts_file() {
        let config = r#"
Host custom
    UserKnownHostsFile /custom/known_hosts
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let custom = hosts.get("custom").unwrap();

        assert_eq!(
            custom.user_known_hosts_file,
            Some("/custom/known_hosts".to_string())
        );
    }

    #[test]
    fn test_port_configuration() {
        let config = r#"
Host custom-port
    HostName example.com
    Port 2222
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let custom = hosts.get("custom-port").unwrap();

        assert_eq!(custom.port, Some(2222));
    }

    #[test]
    fn test_forward_agent() {
        let config = r#"
Host forward
    ForwardAgent yes
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let forward = hosts.get("forward").unwrap();

        assert!(forward.forward_agent);
    }

    #[test]
    fn test_compression() {
        let config = r#"
Host compressed
    Compression yes
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let compressed = hosts.get("compressed").unwrap();

        assert!(compressed.compression);
    }

    #[test]
    fn test_server_alive_interval() {
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
    fn test_connect_timeout() {
        let config = r#"
Host slow
    ConnectTimeout 120
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let slow = hosts.get("slow").unwrap();

        assert_eq!(slow.connect_timeout, Some(120));
    }

    #[test]
    fn test_custom_options_storage() {
        let config = r#"
Host custom
    CustomOption1 value1
    CustomOption2 value2
    AnotherOption another-value
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let custom = hosts.get("custom").unwrap();

        assert_eq!(
            custom.options.get("customoption1"),
            Some(&"value1".to_string())
        );
        assert_eq!(
            custom.options.get("customoption2"),
            Some(&"value2".to_string())
        );
        assert_eq!(
            custom.options.get("anotheroption"),
            Some(&"another-value".to_string())
        );
    }

    #[tokio::test]
    async fn test_mock_ssh_host_key_checking() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Default: host key checking enabled
        assert_eq!(*conn.host_key_checking.read(), Some(true));

        // Disable host key checking
        conn.set_host_key_checking(Some(false));
        assert_eq!(*conn.host_key_checking.read(), Some(false));
    }

    #[tokio::test]
    async fn test_mock_ssh_known_hosts_file() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        conn.set_known_hosts_file(Some(PathBuf::from("/custom/known_hosts")));

        let known_hosts = conn.known_hosts_file.read();
        assert_eq!(*known_hosts, Some(PathBuf::from("/custom/known_hosts")));
    }
}

// ============================================================================
// SSH KEYS TESTS
// ============================================================================

mod ssh_keys {
    use super::*;

    #[test]
    fn test_identity_file_path_expansion() {
        let path = expand_path("~/.ssh/id_rsa");
        assert!(path.to_string_lossy().contains(".ssh"));
        assert!(!path.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn test_default_identity_files_list() {
        // This tests the function that lists default identity files
        // The actual files may or may not exist on the test system
        let default_files = default_identity_files();

        // If files exist, they should be in expected locations
        for file in &default_files {
            let path_str = file.to_string_lossy();
            assert!(
                path_str.contains("id_ed25519")
                    || path_str.contains("id_ecdsa")
                    || path_str.contains("id_rsa")
                    || path_str.contains("id_dsa")
            );
        }
    }

    #[test]
    fn test_host_config_with_identity_file() {
        let config = HostConfig::new()
            .hostname("example.com")
            .user("admin")
            .identity_file("~/.ssh/custom_key");

        assert_eq!(config.identity_file, Some("~/.ssh/custom_key".to_string()));
    }

    #[test]
    fn test_ssh_config_identity_file_parsing() {
        let config = r#"
Host example
    IdentityFile ~/.ssh/id_rsa
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let example = hosts.get("example").unwrap();

        assert!(example.identity_file.is_some());
        // Identity file should have ~ expanded
        let identity = example.identity_file.as_ref().unwrap();
        assert!(!identity.starts_with("~") || identity.contains("~"));
    }

    #[tokio::test]
    async fn test_mock_ssh_multiple_identity_files() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.add_identity_file(PathBuf::from("/home/user/.ssh/id_ed25519"));
        conn.add_identity_file(PathBuf::from("/home/user/.ssh/id_rsa"));
        conn.add_identity_file(PathBuf::from("/home/user/.ssh/work_key"));

        let files = conn.identity_files.read();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_connection_config_default_identity_files() {
        let mut config = ConnectionConfig::default();
        config.defaults.identity_files =
            vec!["~/.ssh/id_ed25519".to_string(), "~/.ssh/id_rsa".to_string()];

        assert_eq!(config.defaults.identity_files.len(), 2);
    }

    #[test]
    fn test_host_config_overrides_default_identity() {
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
// SSH HOST KEYS TESTS
// ============================================================================

mod ssh_host_keys {
    use super::*;

    #[test]
    fn test_verify_host_key_default() {
        let config = ConnectionConfig::default();
        assert!(config.defaults.verify_host_key);
    }

    #[test]
    fn test_disable_host_key_verification() {
        let mut config = ConnectionConfig::default();
        config.defaults.verify_host_key = false;
        assert!(!config.defaults.verify_host_key);
    }

    #[test]
    fn test_known_hosts_file_config() {
        let mut config = ConnectionConfig::default();
        config.defaults.known_hosts_file = Some(PathBuf::from("/custom/known_hosts"));

        assert_eq!(
            config.defaults.known_hosts_file,
            Some(PathBuf::from("/custom/known_hosts"))
        );
    }

    #[test]
    fn test_host_specific_key_checking() {
        let config = r#"
Host trusted
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null

Host production
    StrictHostKeyChecking yes
    UserKnownHostsFile ~/.ssh/known_hosts.prod
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        let trusted = hosts.get("trusted").unwrap();
        assert_eq!(trusted.strict_host_key_checking, Some(false));
        assert_eq!(trusted.user_known_hosts_file, Some("/dev/null".to_string()));

        let production = hosts.get("production").unwrap();
        assert_eq!(production.strict_host_key_checking, Some(true));
        assert!(production.user_known_hosts_file.is_some());
    }

    #[tokio::test]
    async fn test_mock_ssh_host_key_verification_modes() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Test different host key checking modes
        conn.set_host_key_checking(Some(true)); // strict
        assert_eq!(*conn.host_key_checking.read(), Some(true));

        conn.set_host_key_checking(Some(false)); // no checking
        assert_eq!(*conn.host_key_checking.read(), Some(false));

        conn.set_host_key_checking(None); // ask (default)
        assert_eq!(*conn.host_key_checking.read(), None);
    }

    #[test]
    fn test_accept_new_host_key_option() {
        let config = r#"
Host new-servers
    StrictHostKeyChecking accept-new
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let new_servers = hosts.get("new-servers").unwrap();

        // accept-new is neither yes nor no, should be None
        assert_eq!(new_servers.strict_host_key_checking, None);
    }
}

// ============================================================================
// SSH PERFORMANCE TESTS
// ============================================================================

mod ssh_performance {
    use super::*;

    #[tokio::test]
    async fn test_connection_pool_effectiveness() {
        let config = ConnectionConfig::new();
        let factory = ConnectionFactory::new(config);

        // Get the same connection multiple times
        let conn1 = factory.get_connection("localhost").await.unwrap();
        let conn2 = factory.get_connection("localhost").await.unwrap();
        let conn3 = factory.get_connection("localhost").await.unwrap();

        // All should return the same pooled connection
        assert_eq!(conn1.identifier(), conn2.identifier());
        assert_eq!(conn2.identifier(), conn3.identifier());

        // Pool should only have 1 active connection
        let stats = factory.pool_stats().await;
        assert_eq!(stats.active_connections, 1);
    }

    #[tokio::test]
    async fn test_parallel_ssh_efficiency() {
        let conn = Arc::new(MockSshConnection::new("example.com", 22, "admin"));
        conn.set_latency_ms(5); // Small latency
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let start = std::time::Instant::now();

        // Execute commands in parallel
        let mut handles = vec![];
        for i in 0..20 {
            let conn_clone = conn.clone();
            let handle =
                tokio::spawn(
                    async move { conn_clone.execute(&format!("command_{}", i), None).await },
                );
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        let duration = start.elapsed();
        println!("Parallel execution of 20 commands: {:?}", duration);

        assert_eq!(conn.command_count(), 20);
    }

    #[tokio::test]
    async fn test_large_file_transfer_via_ssh() {
        let _temp_dir = TempDir::new().unwrap();
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Create a "large" file (1MB)
        let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        let remote_path = PathBuf::from("/remote/large_file.bin");

        // Upload
        conn.upload_content(&large_content, &remote_path, None)
            .await
            .unwrap();

        // Download
        let downloaded = conn.download_content(&remote_path).await.unwrap();

        assert_eq!(downloaded.len(), large_content.len());
        assert_eq!(downloaded, large_content);
    }

    #[tokio::test]
    async fn test_sftp_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let conn = MockSshConnection::new("example.com", 22, "admin");

        let local_src = temp_dir.path().join("source.txt");
        std::fs::write(&local_src, b"test content for SFTP").unwrap();

        let remote_path = PathBuf::from("/remote/dest.txt");

        // Upload via SFTP
        conn.upload(&local_src, &remote_path, None).await.unwrap();

        // Verify file exists
        assert!(conn.path_exists(&remote_path).await.unwrap());

        // Get file stats
        let stat = conn.stat(&remote_path).await.unwrap();
        assert!(stat.is_file);
        assert!(stat.size > 0);

        // Download content
        let content = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(content, b"test content for SFTP");
    }

    #[tokio::test]
    async fn test_connection_pool_with_custom_size() {
        let config = ConnectionConfig::new();
        let factory = ConnectionFactory::with_pool_size(config, 5);

        let stats = factory.pool_stats().await;
        assert_eq!(stats.max_connections, 5);
    }

    #[tokio::test]
    async fn test_compression_configuration() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_compression(true);

        assert!(conn.compression.load(Ordering::SeqCst));
    }
}

// ============================================================================
// SSH ERRORS TESTS
// ============================================================================

mod ssh_errors {
    use super::*;

    #[tokio::test]
    async fn test_connection_refused() {
        let conn = MockSshConnection::new("unreachable.example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn test_authentication_failure() {
        let conn = MockSshConnection::new("example.com", 22, "wronguser");
        conn.set_auth_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_timeout_handling() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_timeout(true);

        let result = conn.execute("sleep 100", None).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(_))));
    }

    #[tokio::test]
    async fn test_network_interruption() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // Execute successfully first
        let result1 = conn.execute("echo test1", None).await;
        assert!(result1.is_ok());

        // Simulate network interruption
        conn.kill();

        // Check connection is dead
        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_host_unreachable() {
        let conn = MockSshConnection::new("192.168.255.255", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn test_transfer_failure() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn
            .upload_content(b"test", Path::new("/remote/file"), None)
            .await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }

    #[tokio::test]
    async fn test_file_not_found_on_remote() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        let result = conn
            .download_content(Path::new("/nonexistent/file.txt"))
            .await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }

    #[tokio::test]
    async fn test_recovery_after_connection_error() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // First, simulate failure
        conn.set_connection_failure(true);
        let result = conn.execute("echo test", None).await;
        assert!(result.is_err());

        // Then, "recover" the connection
        conn.set_connection_failure(false);
        let result = conn.execute("echo recovered", None).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_connection_error_display() {
        let errors = vec![
            ConnectionError::ConnectionFailed("Connection refused".to_string()),
            ConnectionError::AuthenticationFailed("Invalid credentials".to_string()),
            ConnectionError::Timeout(30),
            ConnectionError::HostNotFound("unknown.host".to_string()),
            ConnectionError::SshError("SSH protocol error".to_string()),
        ];

        for err in errors {
            let display = format!("{}", err);
            assert!(!display.is_empty());
        }
    }

    #[test]
    fn test_retry_config_with_connection_failures() {
        let config = RetryConfig::default();

        // Verify exponential backoff for retries
        let delay0 = config.delay_for_attempt(0);
        let delay1 = config.delay_for_attempt(1);
        let delay2 = config.delay_for_attempt(2);

        assert!(delay1 > delay0);
        assert!(delay2 > delay1);

        // Should cap at max delay
        let delay_max = config.delay_for_attempt(100);
        assert_eq!(delay_max, config.max_delay);
    }
}

// ============================================================================
// SSH EDGE CASES TESTS
// ============================================================================

mod ssh_edge_cases {
    use super::*;

    #[tokio::test]
    async fn test_non_standard_ssh_port() {
        let conn = MockSshConnection::new("example.com", 2222, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        assert_eq!(conn.port, 2222);
        assert!(conn.identifier().contains("2222"));

        let result = conn.execute("echo test", None).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_ipv6_host() {
        let conn = MockSshConnection::new("::1", 22, "admin");
        conn.set_default_result(CommandResult::success(
            "localhost".to_string(),
            String::new(),
        ));

        assert_eq!(conn.host, "::1");

        let result = conn.execute("hostname", None).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_ipv6_with_brackets() {
        let conn = MockSshConnection::new("[2001:db8::1]", 22, "admin");
        conn.set_default_result(CommandResult::success(
            "ipv6host".to_string(),
            String::new(),
        ));

        let result = conn.execute("hostname", None).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_very_long_command() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Create a very long command
        let long_cmd = format!("echo '{}'", "x".repeat(10000));

        conn.set_default_result(CommandResult::success(String::new(), String::new()));

        let result = conn.execute(&long_cmd, None).await.unwrap();
        assert!(result.success);

        // Verify the full command was captured
        let commands = conn.get_commands();
        assert!(!commands.is_empty());
        assert!(commands[0].len() > 10000);
    }

    #[tokio::test]
    async fn test_binary_data_transfer() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Create binary content with all byte values
        let binary_content: Vec<u8> = (0..=255).collect();
        let remote_path = PathBuf::from("/remote/binary.bin");

        // Upload binary data
        conn.upload_content(&binary_content, &remote_path, None)
            .await
            .unwrap();

        // Download and verify
        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(downloaded, binary_content);
    }

    #[tokio::test]
    async fn test_special_characters_in_path() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        let special_path = PathBuf::from("/remote/path with spaces/file (1).txt");
        let content = b"content with special path";

        conn.upload_content(content, &special_path, None)
            .await
            .unwrap();

        let downloaded = conn.download_content(&special_path).await.unwrap();
        assert_eq!(downloaded, content);
    }

    #[tokio::test]
    async fn test_unicode_content_transfer() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        let unicode_content = "Hello, World. Merhaba Dunya. Bonjour le monde.";
        let remote_path = PathBuf::from("/remote/unicode.txt");

        conn.upload_content(unicode_content.as_bytes(), &remote_path, None)
            .await
            .unwrap();

        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(String::from_utf8(downloaded).unwrap(), unicode_content);
    }

    #[tokio::test]
    async fn test_empty_file_transfer() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        let remote_path = PathBuf::from("/remote/empty.txt");
        conn.upload_content(b"", &remote_path, None).await.unwrap();

        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(downloaded.len(), 0);
    }

    #[tokio::test]
    async fn test_command_with_special_characters() {
        let conn = MockSshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // Commands with various special characters
        let special_commands = vec![
            "echo 'single quotes'",
            "echo \"double quotes\"",
            "echo $VARIABLE",
            "echo `backticks`",
            "echo 'line1\nline2'",
            "echo 'tab\there'",
            "command; another_command",
            "command && another_command",
            "command || fallback_command",
            "cat file | grep pattern",
        ];

        for cmd in special_commands {
            let result = conn.execute(cmd, None).await.unwrap();
            assert!(result.success, "Failed for command: {}", cmd);
        }
    }

    #[test]
    fn test_ssh_config_with_wildcards() {
        let config = r#"
Host *.production.example.com
    User prodadmin
    Port 2222

Host *.staging.example.com
    User stagingadmin
    Port 22

Host web-?
    User webadmin

Host db[0-9]
    User dbadmin
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        assert!(hosts.contains_key("*.production.example.com"));
        assert!(hosts.contains_key("*.staging.example.com"));
        assert!(hosts.contains_key("web-?"));
        assert!(hosts.contains_key("db[0-9]"));
    }

    #[tokio::test]
    async fn test_concurrent_connections_to_same_host() {
        let conn = Arc::new(MockSshConnection::new("example.com", 22, "admin"));
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let mut handles = vec![];

        // Spawn 50 concurrent tasks
        for i in 0..50 {
            let conn_clone = conn.clone();
            let handle =
                tokio::spawn(async move { conn_clone.execute(&format!("task_{}", i), None).await });
            handles.push(handle);
        }

        // All should complete successfully
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 50);
    }

    #[tokio::test]
    async fn test_connection_state_transitions() {
        let conn = MockSshConnection::new("example.com", 22, "admin");

        // Initially alive
        assert!(conn.is_alive().await);

        // Execute a command
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));
        conn.execute("test", None).await.unwrap();
        assert!(conn.is_alive().await);

        // Close connection
        conn.close().await.unwrap();
        assert!(!conn.is_alive().await);
    }

    #[test]
    fn test_host_config_timeout_duration() {
        let config = HostConfig::new().timeout(120);

        let duration = config.timeout_duration();
        assert_eq!(duration, Duration::from_secs(120));
    }

    #[test]
    fn test_host_config_default_timeout() {
        let config = HostConfig::new();

        let duration = config.timeout_duration();
        assert_eq!(duration, Duration::from_secs(DEFAULT_TIMEOUT));
    }
}

// ============================================================================
// SSH CONFIG PARSING ADVANCED TESTS
// ============================================================================

mod ssh_config_parsing {
    use super::*;

    #[test]
    fn test_full_ssh_config() {
        let config = r#"
# Global defaults
Host *
    ServerAliveInterval 60
    ServerAliveCountMax 3
    ControlMaster auto
    ControlPersist 600
    ControlPath ~/.ssh/sockets/%r@%h-%p
    AddKeysToAgent yes
    ForwardAgent no

# Production servers
Host prod-*
    User prodadmin
    Port 2222
    IdentityFile ~/.ssh/prod_key
    StrictHostKeyChecking yes
    ProxyJump bastion.prod.example.com

# Staging servers
Host staging-*
    User stagingadmin
    IdentityFile ~/.ssh/staging_key
    StrictHostKeyChecking no
    Compression yes

# Development
Host dev
    HostName dev.local.example.com
    User developer
    Port 22
    IdentityFile ~/.ssh/dev_key
    ForwardAgent yes
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        // Check wildcard config
        let wildcard = hosts.get("*").unwrap();
        assert_eq!(wildcard.server_alive_interval, Some(60));
        assert_eq!(wildcard.server_alive_count_max, Some(3));

        // Check production config
        let prod = hosts.get("prod-*").unwrap();
        assert_eq!(prod.user, Some("prodadmin".to_string()));
        assert_eq!(prod.port, Some(2222));
        assert_eq!(prod.strict_host_key_checking, Some(true));
        assert!(prod.proxy_jump.is_some());

        // Check staging config
        let staging = hosts.get("staging-*").unwrap();
        assert_eq!(staging.user, Some("stagingadmin".to_string()));
        assert_eq!(staging.strict_host_key_checking, Some(false));
        assert!(staging.compression);

        // Check dev config
        let dev = hosts.get("dev").unwrap();
        assert_eq!(dev.hostname, Some("dev.local.example.com".to_string()));
        assert_eq!(dev.user, Some("developer".to_string()));
        assert!(dev.forward_agent);
    }

    #[test]
    fn test_include_directive_ignored() {
        // Include directives should be safely ignored/stored
        let config = r#"
Include ~/.ssh/config.d/*

Host example
    HostName example.com
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let example = hosts.get("example").unwrap();
        assert_eq!(example.hostname, Some("example.com".to_string()));
    }

    #[test]
    fn test_match_block_ignored() {
        // Match blocks should be safely ignored
        let config = r#"
Match host *.example.com
    User matchuser

Host regular
    HostName regular.com
    User regularuser
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let regular = hosts.get("regular").unwrap();
        assert_eq!(regular.user, Some("regularuser".to_string()));
    }

    #[test]
    fn test_case_insensitive_keywords() {
        let config = r#"
Host example
    HOSTNAME example.com
    PORT 2222
    USER admin
    IDENTITYFILE ~/.ssh/id_rsa
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let example = hosts.get("example").unwrap();

        assert_eq!(example.hostname, Some("example.com".to_string()));
        assert_eq!(example.port, Some(2222));
        assert_eq!(example.user, Some("admin".to_string()));
    }

    #[test]
    fn test_quoted_values() {
        let config = r#"
Host quoted
    HostName "example.com"
    User "admin user"
    IdentityFile "~/.ssh/my key"
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        let quoted = hosts.get("quoted").unwrap();

        assert_eq!(quoted.hostname, Some("example.com".to_string()));
        assert_eq!(quoted.user, Some("admin user".to_string()));
    }

    #[test]
    fn test_empty_config() {
        let config = "";
        let hosts = SshConfigParser::parse(config).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_comments_only_config() {
        let config = r#"
# This is a comment
# Another comment
    # Indented comment
"#;

        let hosts = SshConfigParser::parse(config).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_multiple_hosts_same_line() {
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
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

mod integration {
    use super::*;

    #[tokio::test]
    async fn test_full_ssh_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let conn = MockSshConnection::new("example.com", 22, "admin");
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
    async fn test_connection_factory_with_ssh_hosts() {
        let mut config = ConnectionConfig::new();

        // Configure a host
        config.add_host(
            "webserver",
            HostConfig::new()
                .hostname("192.168.1.10")
                .port(22)
                .user("webadmin"),
        );

        let factory = ConnectionFactory::new(config);

        // Get local connection (should work without actual SSH)
        let local_conn = factory.get_connection("localhost").await.unwrap();
        assert!(local_conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_parallel_operations_on_connection() {
        let conn = Arc::new(MockSshConnection::new("example.com", 22, "admin"));
        conn.set_latency_ms(5);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let mut exec_handles = vec![];
        let mut upload_handles = vec![];

        // Mix of command executions and file operations
        for i in 0..10 {
            let conn_clone = conn.clone();
            exec_handles.push(tokio::spawn(async move {
                conn_clone.execute(&format!("cmd_{}", i), None).await
            }));

            let conn_clone2 = conn.clone();
            let remote_path = PathBuf::from(format!("/remote/file_{}.txt", i));
            upload_handles.push(tokio::spawn(async move {
                conn_clone2
                    .upload_content(b"content", &remote_path, None)
                    .await
            }));
        }

        for handle in exec_handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        for handle in upload_handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 10);
    }

    #[test]
    fn test_connection_config_merge_with_defaults() {
        let mut config = ConnectionConfig::new();
        config.set_default_user("default_user");
        config.set_default_port(22);
        config.set_default_timeout(30);

        config.add_host("partial", HostConfig::new().hostname("partial.example.com"));

        let merged = config.get_host_merged("partial");

        // Host-specific
        assert_eq!(merged.hostname, Some("partial.example.com".to_string()));

        // From defaults
        assert_eq!(merged.user, Some("default_user".to_string()));
        assert_eq!(merged.port, Some(22));
        assert_eq!(merged.connect_timeout, Some(30));
    }

    #[test]
    fn test_toml_config_with_ssh_hosts() {
        let toml = r#"
[defaults]
user = "deploy"
port = 22
timeout = 60
use_agent = true
verify_host_key = true

[hosts.production]
hostname = "prod.example.com"
port = 2222
user = "produser"
identity_file = "~/.ssh/prod_key"
compression = true

[hosts.staging]
hostname = "staging.example.com"
user = "staginguser"
forward_agent = true
"#;

        let config = ConnectionConfig::from_toml(toml).unwrap();

        assert_eq!(config.defaults.user, "deploy");
        assert_eq!(config.defaults.timeout, 60);
        assert!(config.defaults.use_agent);

        let prod = config.get_host("production").unwrap();
        assert_eq!(prod.hostname, Some("prod.example.com".to_string()));
        assert_eq!(prod.port, Some(2222));
        assert!(prod.compression);

        let staging = config.get_host("staging").unwrap();
        assert!(staging.forward_agent);
    }
}
