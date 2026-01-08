//! Comprehensive tests for the Russh connection implementation
//!
//! This test module covers the russh-based SSH connection implementation.
//! Since the russh connection is not yet fully implemented, these tests serve
//! to validate the API, test mocked behavior, and prepare for the full implementation.
//!
//! Test coverage includes:
//! - Connection establishment (mocked and integration)
//! - Command execution
//! - File upload/download via SFTP
//! - Key authentication
//! - Password authentication
//! - Connection pooling
//! - Error handling
//! - Concurrent operations
//!
//! To run these tests:
//! ```bash
//! cargo test --test russh_connection_tests --features russh
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use tempfile::TempDir;

use rustible::connection::config::ConnectionConfig;
use rustible::connection::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    HostConfig, TransferOptions,
};

#[cfg(feature = "russh")]
use rustible::connection::russh::RusshConnectionBuilder;

// ============================================================================
// Mock Russh Connection for Testing
// ============================================================================

/// Mock russh connection that simulates SSH behavior for testing
/// This allows us to test the API and behavior without requiring actual SSH infrastructure
#[derive(Debug)]
pub struct MockRusshConnection {
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
    // Russh-specific features
    #[allow(dead_code)]
    async_native: AtomicBool,
    use_key_auth: AtomicBool,
    use_password_auth: AtomicBool,
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
            files_transferred: RwLock::new(Vec::new()),
            virtual_filesystem: RwLock::new(HashMap::new()),
            command_results: RwLock::new(HashMap::new()),
            default_result: RwLock::new(CommandResult::success(String::new(), String::new())),
            execution_count: AtomicU32::new(0),
            should_fail_auth: AtomicBool::new(false),
            should_fail_connection: AtomicBool::new(false),
            should_timeout: AtomicBool::new(false),
            latency_ms: AtomicU32::new(0),
            async_native: AtomicBool::new(true),
            use_key_auth: AtomicBool::new(false),
            use_password_auth: AtomicBool::new(false),
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

    /// Enable key authentication
    pub fn set_key_auth(&self, enabled: bool) {
        self.use_key_auth.store(enabled, Ordering::SeqCst);
    }

    /// Enable password authentication
    pub fn set_password_auth(&self, enabled: bool) {
        self.use_password_auth.store(enabled, Ordering::SeqCst);
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

    /// Check if using key authentication
    pub fn is_using_key_auth(&self) -> bool {
        self.use_key_auth.load(Ordering::SeqCst)
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
// Connection Establishment Tests
// ============================================================================

mod connection_establishment {
    use super::*;

    #[tokio::test]
    async fn test_mock_russh_connection_basic() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        assert_eq!(conn.identifier(), "russh:admin@example.com:22");
        assert!(conn.is_alive().await);
        assert_eq!(conn.command_count(), 0);
    }

    #[tokio::test]
    async fn test_russh_connection_identifier_format() {
        let conn = MockRusshConnection::new("192.168.1.100", 2222, "deploy");

        assert!(conn.identifier().contains("russh:"));
        assert!(conn.identifier().contains("deploy@192.168.1.100:2222"));
    }

    #[cfg(feature = "russh")]
    #[tokio::test]
    async fn test_russh_connection_builder_api() {
        // Test the builder pattern API compiles and chains correctly
        let _builder = RusshConnectionBuilder::new("example.com")
            .port(2222)
            .user("admin")
            .compression(true)
            .timeout(60);

        // Builder pattern verified by successful method chaining
    }

    #[cfg(feature = "russh")]
    #[test]
    fn test_russh_connection_builder_defaults() {
        let builder = RusshConnectionBuilder::new("example.com");

        assert_eq!(builder.host, "example.com");
        assert_eq!(builder.port, 22);
        assert!(!builder.compression);
    }

    #[tokio::test]
    async fn test_connection_establishment_failure() {
        let conn = MockRusshConnection::new("unreachable.example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn test_connection_state_tracking() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Initially connected
        assert!(conn.is_alive().await);

        // Kill connection
        conn.kill();

        // Should be dead
        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_connection_identifier_uniqueness() {
        let conn1 = MockRusshConnection::new("host1.com", 22, "user1");
        let conn2 = MockRusshConnection::new("host2.com", 22, "user2");
        let conn3 = MockRusshConnection::new("host1.com", 2222, "user1");

        assert_ne!(conn1.identifier(), conn2.identifier());
        assert_ne!(conn1.identifier(), conn3.identifier());
    }
}

// ============================================================================
// Command Execution Tests
// ============================================================================

mod command_execution {
    use super::*;

    #[tokio::test]
    async fn test_command_execution_simple() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("hello".to_string(), String::new()));

        let result = conn.execute("echo hello", None).await.unwrap();

        assert!(result.success);
        assert_eq!(conn.command_count(), 1);
        assert_eq!(conn.get_commands(), vec!["echo hello".to_string()]);
    }

    #[tokio::test]
    async fn test_command_execution_with_stderr() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_command_result(
            "test_cmd",
            CommandResult::success("stdout".to_string(), "stderr".to_string()),
        );

        let result = conn.execute("test_cmd", None).await.unwrap();

        assert_eq!(result.stdout, "stdout");
        assert_eq!(result.stderr, "stderr");
    }

    #[tokio::test]
    async fn test_command_execution_failure() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_command_result(
            "failing_cmd",
            CommandResult::failure(1, String::new(), "error".to_string()),
        );

        let result = conn.execute("failing_cmd", None).await.unwrap();

        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_command_execution_with_options() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let options = ExecuteOptions::new()
            .with_cwd("/tmp")
            .with_env("TEST_VAR", "test_value");

        let result = conn.execute("pwd", Some(options)).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_multiple_commands_on_same_connection() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        for i in 0..5 {
            let result = conn.execute(&format!("cmd_{}", i), None).await;
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 5);
        let commands = conn.get_commands();
        assert_eq!(commands.len(), 5);
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_timeout(true);

        let result = conn.execute("long_running_cmd", None).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(_))));
    }

    #[tokio::test]
    async fn test_concurrent_command_execution() {
        let conn = Arc::new(MockRusshConnection::new("example.com", 22, "admin"));
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let mut handles = vec![];

        for i in 0..10 {
            let conn_clone = conn.clone();
            let handle =
                tokio::spawn(async move { conn_clone.execute(&format!("cmd_{}", i), None).await });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        assert_eq!(conn.command_count(), 10);
    }

    #[tokio::test]
    async fn test_command_execution_with_latency() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_latency_ms(10);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let start = std::time::Instant::now();
        conn.execute("echo test", None).await.unwrap();
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(10));
    }
}

// ============================================================================
// File Upload Tests
// ============================================================================

mod file_upload {
    use super::*;

    #[tokio::test]
    async fn test_file_upload_basic() {
        let temp_dir = TempDir::new().unwrap();
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let local_file = temp_dir.path().join("test.txt");
        std::fs::write(&local_file, b"test content").unwrap();

        let remote_path = PathBuf::from("/remote/test.txt");
        let result = conn.upload(&local_file, &remote_path, None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_upload_content_directly() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let content = b"direct upload content";
        let remote_path = PathBuf::from("/remote/direct.txt");

        let result = conn.upload_content(content, &remote_path, None).await;

        assert!(result.is_ok());
        assert!(conn.path_exists(&remote_path).await.unwrap());
    }

    #[tokio::test]
    async fn test_upload_large_file() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Create 1MB content
        let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        let remote_path = PathBuf::from("/remote/large.bin");

        let result = conn
            .upload_content(&large_content, &remote_path, None)
            .await;

        assert!(result.is_ok());

        // Verify size
        let stat = conn.stat(&remote_path).await.unwrap();
        assert_eq!(stat.size, large_content.len() as u64);
    }

    #[tokio::test]
    async fn test_upload_binary_content() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Binary content with all byte values
        let binary: Vec<u8> = (0..=255).collect();
        let remote_path = PathBuf::from("/remote/binary.bin");

        conn.upload_content(&binary, &remote_path, None)
            .await
            .unwrap();

        // Verify
        let downloaded = conn.download_content(&remote_path).await.unwrap();
        assert_eq!(downloaded, binary);
    }

    #[tokio::test]
    async fn test_upload_failure_on_connection_error() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn
            .upload_content(b"test", Path::new("/remote/file"), None)
            .await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }

    #[tokio::test]
    async fn test_upload_with_transfer_options() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let options = TransferOptions::new()
            .with_mode(0o755)
            .with_owner("testuser")
            .with_group("testgroup");

        let result = conn
            .upload_content(b"content", Path::new("/remote/script.sh"), Some(options))
            .await;

        assert!(result.is_ok());
    }
}

// ============================================================================
// File Download Tests
// ============================================================================

mod file_download {
    use super::*;

    #[tokio::test]
    async fn test_file_download_basic() {
        let temp_dir = TempDir::new().unwrap();
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let remote_path = PathBuf::from("/remote/test.txt");
        let content = b"remote content";
        conn.add_virtual_file(remote_path.clone(), content.to_vec());

        let local_path = temp_dir.path().join("downloaded.txt");
        let result = conn.download(&remote_path, &local_path).await;

        assert!(result.is_ok());
        assert!(local_path.exists());

        let downloaded = std::fs::read(&local_path).unwrap();
        assert_eq!(downloaded, content);
    }

    #[tokio::test]
    async fn test_download_content_directly() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let remote_path = PathBuf::from("/remote/data.txt");
        let content = b"file content";
        conn.add_virtual_file(remote_path.clone(), content.to_vec());

        let result = conn.download_content(&remote_path).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content);
    }

    #[tokio::test]
    async fn test_download_nonexistent_file() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let result = conn
            .download_content(Path::new("/nonexistent/file.txt"))
            .await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }

    #[tokio::test]
    async fn test_download_large_file() {
        let temp_dir = TempDir::new().unwrap();
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // 5MB file
        let large_content: Vec<u8> = (0..5 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
        let remote_path = PathBuf::from("/remote/large.bin");
        conn.add_virtual_file(remote_path.clone(), large_content.clone());

        let local_path = temp_dir.path().join("large_downloaded.bin");
        let result = conn.download(&remote_path, &local_path).await;

        assert!(result.is_ok());

        let downloaded = std::fs::read(&local_path).unwrap();
        assert_eq!(downloaded.len(), large_content.len());
        assert_eq!(downloaded, large_content);
    }

    #[tokio::test]
    async fn test_download_failure_on_connection_error() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.download_content(Path::new("/remote/file.txt")).await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }
}

// ============================================================================
// Key Authentication Tests
// ============================================================================

mod key_authentication {
    use super::*;

    #[tokio::test]
    async fn test_key_authentication_enabled() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_key_auth(true);

        assert!(conn.is_using_key_auth());
    }

    #[tokio::test]
    async fn test_key_authentication_failure() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_key_auth(true);
        conn.set_auth_failure(true);

        let result = conn.execute("test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[cfg(feature = "russh")]
    #[tokio::test]
    async fn test_builder_with_private_key() {
        let _builder = RusshConnectionBuilder::new("example.com")
            .user("admin")
            .private_key("~/.ssh/id_ed25519");

        // Builder should accept key path
    }

    #[tokio::test]
    async fn test_key_auth_fallback_to_password() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Initially try key auth (fails)
        conn.set_key_auth(true);
        conn.set_auth_failure(true);
        let result = conn.execute("test", None).await;
        assert!(result.is_err());

        // Fall back to password auth
        conn.set_key_auth(false);
        conn.set_password_auth(true);
        conn.set_auth_failure(false);
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let result = conn.execute("test", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multiple_key_attempts() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Simulate trying multiple keys (mock would need enhancement for this)
        conn.set_key_auth(true);

        // For now, just verify auth can be set
        assert!(conn.is_using_key_auth());
    }
}

// ============================================================================
// Password Authentication Tests (if needed as fallback)
// ============================================================================

mod password_authentication {
    use super::*;

    #[cfg(feature = "russh")]
    #[tokio::test]
    async fn test_builder_with_password() {
        let _builder = RusshConnectionBuilder::new("example.com")
            .user("admin")
            .password("secret");

        // Builder should accept password
    }

    #[tokio::test]
    async fn test_password_authentication_failure() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_password_auth(true);
        conn.set_auth_failure(true);

        let result = conn.execute("test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }
}

// ============================================================================
// File Stat Tests
// ============================================================================

mod file_stat {
    use super::*;

    #[tokio::test]
    async fn test_path_exists() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let path = PathBuf::from("/remote/existing.txt");
        conn.add_virtual_file(path.clone(), b"content".to_vec());

        assert!(conn.path_exists(&path).await.unwrap());
        assert!(!conn.path_exists(Path::new("/nonexistent")).await.unwrap());
    }

    #[tokio::test]
    async fn test_file_stat() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let path = PathBuf::from("/remote/file.txt");
        let content = b"test content for stat";
        conn.add_virtual_file(path.clone(), content.to_vec());

        let stat = conn.stat(&path).await.unwrap();

        assert_eq!(stat.size, content.len() as u64);
        assert!(stat.is_file);
        assert!(!stat.is_dir);
    }

    #[tokio::test]
    async fn test_stat_nonexistent_file() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        let result = conn.stat(Path::new("/nonexistent")).await;

        assert!(matches!(result, Err(ConnectionError::TransferFailed(_))));
    }

    #[tokio::test]
    async fn test_is_directory() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");

        // Current mock implementation returns false for all paths
        let result = conn.is_directory(Path::new("/tmp")).await.unwrap();
        assert!(!result); // Mock behavior
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

mod error_handling {
    use super::*;

    #[tokio::test]
    async fn test_connection_refused() {
        let conn = MockRusshConnection::new("unreachable.example.com", 22, "admin");
        conn.set_connection_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(result, Err(ConnectionError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn test_authentication_failed() {
        let conn = MockRusshConnection::new("example.com", 22, "wronguser");
        conn.set_auth_failure(true);

        let result = conn.execute("echo test", None).await;

        assert!(matches!(
            result,
            Err(ConnectionError::AuthenticationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_timeout_error() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_timeout(true);

        let result = conn.execute("sleep 100", None).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(_))));
    }

    #[tokio::test]
    async fn test_network_interruption() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // Execute successfully first
        let result1 = conn.execute("echo test1", None).await;
        assert!(result1.is_ok());

        // Simulate network interruption
        conn.kill();

        // Connection should be dead
        assert!(!conn.is_alive().await);
    }

    #[tokio::test]
    async fn test_recovery_after_error() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
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
}

// ============================================================================
// Integration-Style Tests
// ============================================================================

mod integration {
    use super::*;

    #[tokio::test]
    async fn test_full_workflow() {
        let temp_dir = TempDir::new().unwrap();
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
    async fn test_parallel_operations() {
        let conn = Arc::new(MockRusshConnection::new("example.com", 22, "admin"));
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
}

// ============================================================================
// Performance and Async Tests
// ============================================================================

mod performance {
    use super::*;

    #[tokio::test]
    async fn test_async_native_performance() {
        let conn = Arc::new(MockRusshConnection::new("example.com", 22, "admin"));
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        let start = std::time::Instant::now();

        // Execute multiple commands concurrently (async-native)
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

        let elapsed = start.elapsed();
        println!("Async execution of 20 commands: {:?}", elapsed);

        assert_eq!(conn.command_count(), 20);
    }

    #[tokio::test]
    async fn test_connection_reuse() {
        let conn = MockRusshConnection::new("example.com", 22, "admin");
        conn.set_default_result(CommandResult::success("ok".to_string(), String::new()));

        // Execute multiple commands on same connection
        for i in 0..50 {
            let result = conn.execute(&format!("echo 'command {}'", i), None).await;
            assert!(result.is_ok());
        }

        assert!(conn.is_alive().await, "Connection should still be alive");
        assert_eq!(conn.command_count(), 50);
    }
}

// ============================================================================
// Configuration Tests
// ============================================================================

mod configuration {
    use super::*;

    #[test]
    fn test_host_config_for_russh() {
        let config = HostConfig::new()
            .hostname("example.com")
            .port(2222)
            .user("admin")
            .identity_file("~/.ssh/id_ed25519")
            .compression(true)
            .timeout(60);

        assert_eq!(config.hostname, Some("example.com".to_string()));
        assert_eq!(config.port, Some(2222));
        assert_eq!(config.user, Some("admin".to_string()));
        assert!(config.compression);
    }

    #[test]
    fn test_connection_config_defaults() {
        let config = ConnectionConfig::default();

        assert!(!config.defaults.user.is_empty());
        assert!(config.defaults.verify_host_key);
    }

    #[cfg(feature = "russh")]
    #[test]
    fn test_russh_builder_configuration() {
        let builder = RusshConnectionBuilder::new("example.com")
            .port(2222)
            .user("deploy")
            .private_key("~/.ssh/deploy_key")
            .timeout(120)
            .compression(true);

        assert_eq!(builder.host, "example.com");
        assert_eq!(builder.port, 2222);
        assert_eq!(builder.user, "deploy");
        assert!(builder.compression);
    }
}
