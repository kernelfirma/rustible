//! Real SSH Integration Tests (ssh2-backend)
//!
//! These tests require actual SSH targets to be available. They validate:
//! - SSH connection establishment
//! - Command execution via SSH
//! - File transfers via SFTP
//! - Connection pooling behavior
//! - Privilege escalation
//!
//! NOTE: This test file requires the ssh2-backend feature. Run with:
//! ```bash
//! export RUSTIBLE_TEST_SSH_ENABLED=1
//! export RUSTIBLE_TEST_SSH_USER=testuser
//! cargo test --test real_ssh_tests --features ssh2-backend -- --test-threads=1
//! ```
//!
//! Or use the test infrastructure:
//! ```bash
//! cd tests/infrastructure
//! ./provision.sh deploy
//! cargo test --test real_ssh_tests --features ssh2-backend
//! ```

#![cfg(feature = "ssh2-backend")]

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustible::connection::{Connection, ExecuteOptions, SshConnectionBuilder};
use tempfile::TempDir;
use tokio::sync::Semaphore;

mod common;

/// Configuration for real SSH tests
struct SshTestConfig {
    enabled: bool,
    user: String,
    key_path: PathBuf,
    hosts: Vec<String>,
    #[allow(dead_code)]
    inventory_path: Option<PathBuf>,
}

impl SshTestConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_TEST_SSH_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let user = env::var("RUSTIBLE_TEST_SSH_USER").unwrap_or_else(|_| "testuser".to_string());

        let key_path = env::var("RUSTIBLE_TEST_SSH_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".ssh/id_ed25519")
            });

        let hosts = env::var("RUSTIBLE_TEST_SSH_HOSTS")
            .map(|h| h.split(',').map(String::from).collect())
            .unwrap_or_else(|_| {
                // Default to infrastructure test hosts
                vec![
                    "192.168.178.141".to_string(),
                    "192.168.178.142".to_string(),
                    "192.168.178.143".to_string(),
                    "192.168.178.144".to_string(),
                    "192.168.178.145".to_string(),
                ]
            });

        let inventory_path = env::var("RUSTIBLE_TEST_INVENTORY").map(PathBuf::from).ok();

        Self {
            enabled,
            user,
            key_path,
            hosts,
            inventory_path,
        }
    }

    fn skip_if_disabled(&self) -> bool {
        if !self.enabled {
            eprintln!("Skipping real SSH tests (RUSTIBLE_TEST_SSH_ENABLED not set)");
            true
        } else {
            false
        }
    }

    fn first_host(&self) -> Option<&str> {
        self.hosts.first().map(|s| s.as_str())
    }
}

// =============================================================================
// Connection Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_connect_with_key_authentication() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    // Use rustible's SSH connection builder
    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .timeout(Duration::from_secs(30))
        .connect()
        .await
        .expect("Failed to connect via SSH");

    assert!(connection.is_alive().await);

    connection
        .close()
        .await
        .expect("Failed to close connection");
}

#[tokio::test]
async fn test_ssh_connect_with_password_authentication() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Password auth test - only if password is provided
    let password = match env::var("RUSTIBLE_TEST_SSH_PASSWORD") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Skipping password auth test (RUSTIBLE_TEST_SSH_PASSWORD not set)");
            return;
        }
    };

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .password(password)
        .timeout(Duration::from_secs(30))
        .connect()
        .await
        .expect("Failed to connect via SSH with password");

    assert!(connection.is_alive().await);
    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_connection_timeout() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Try to connect to a non-routable IP with short timeout
    let start = Instant::now();
    let result = SshConnectionBuilder::new("10.255.255.1") // Non-routable
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .timeout(Duration::from_secs(2)) // Short timeout
        .connect()
        .await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Should have failed to connect");
    assert!(
        elapsed < Duration::from_secs(10),
        "Timeout should have triggered within reasonable time"
    );
}

// =============================================================================
// Command Execution Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_command_execution_simple() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test simple echo command
    let result = connection.execute("echo 'hello world'", None).await;
    assert!(result.is_ok(), "Command execution failed: {:?}", result);

    let output = result.unwrap();
    assert!(output.success, "Command should succeed");
    assert!(
        output.stdout.contains("hello world"),
        "Output should contain 'hello world', got: {}",
        output.stdout
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_command_execution_with_environment() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    let options = ExecuteOptions::new()
        .with_env("TEST_VAR", "test_value")
        .with_env("ANOTHER_VAR", "123");

    let result = connection
        .execute("echo $TEST_VAR-$ANOTHER_VAR", Some(options))
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert!(
        output.stdout.contains("test_value-123"),
        "Environment variables should be set"
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_command_execution_with_working_directory() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    let options = ExecuteOptions::new().with_cwd("/tmp");

    let result = connection.execute("pwd", Some(options)).await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert!(
        output.stdout.trim() == "/tmp",
        "Working directory should be /tmp, got: {}",
        output.stdout.trim()
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_command_exit_codes() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test exit code 0
    let result = connection.execute("exit 0", None).await.unwrap();
    assert!(result.success);
    assert_eq!(result.exit_code, 0);

    // Test exit code 1
    let result = connection.execute("exit 1", None).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, 1);

    // Test exit code 42
    let result = connection.execute("exit 42", None).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, 42);

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_command_stderr_capture() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Command that writes to stderr
    let result = connection
        .execute("echo 'stdout message'; echo 'stderr message' >&2", None)
        .await
        .unwrap();

    assert!(result.stdout.contains("stdout message"));
    assert!(result.stderr.contains("stderr message"));

    connection.close().await.ok();
}

// =============================================================================
// File Transfer Tests (SFTP)
// =============================================================================

#[tokio::test]
async fn test_ssh_file_upload_sftp() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create local test file
    let local_file = temp_dir.path().join("test_upload.txt");
    let test_content = "Hello from SFTP upload test!";
    std::fs::write(&local_file, test_content).expect("Failed to write local file");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Upload file
    let remote_path = PathBuf::from("/tmp/rustible_test_upload.txt");
    let result = connection.upload(&local_file, &remote_path, None).await;
    assert!(result.is_ok(), "Upload failed: {:?}", result);

    // Verify file exists and has correct content
    let verify = connection
        .execute("cat /tmp/rustible_test_upload.txt", None)
        .await
        .unwrap();
    assert!(verify.success);
    assert_eq!(verify.stdout.trim(), test_content);

    // Cleanup
    connection
        .execute("rm -f /tmp/rustible_test_upload.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_file_download_sftp() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Create remote file
    let test_content = "Hello from SFTP download test!";
    connection
        .execute(
            &format!("echo '{}' > /tmp/rustible_test_download.txt", test_content),
            None,
        )
        .await
        .expect("Failed to create remote file");

    // Download file
    let remote_path = PathBuf::from("/tmp/rustible_test_download.txt");
    let local_path = temp_dir.path().join("downloaded.txt");
    let result = connection.download(&remote_path, &local_path).await;
    assert!(result.is_ok(), "Download failed: {:?}", result);

    // Verify content
    let downloaded_content = std::fs::read_to_string(&local_path).expect("Failed to read file");
    assert!(
        downloaded_content.contains(test_content),
        "Downloaded content doesn't match"
    );

    // Cleanup
    connection
        .execute("rm -f /tmp/rustible_test_download.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_large_file_transfer() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create a 10MB test file
    let local_file = temp_dir.path().join("large_file.bin");
    let large_content: Vec<u8> = (0..10 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&local_file, &large_content).expect("Failed to write large file");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Upload large file
    let remote_path = PathBuf::from("/tmp/rustible_large_test.bin");
    let start = Instant::now();
    let result = connection.upload(&local_file, &remote_path, None).await;
    let upload_time = start.elapsed();

    assert!(result.is_ok(), "Large file upload failed: {:?}", result);
    println!("10MB upload completed in {:?}", upload_time);

    // Verify size
    let verify = connection
        .execute("stat -c %s /tmp/rustible_large_test.bin", None)
        .await
        .unwrap();
    assert!(verify.success);
    let remote_size: usize = verify.stdout.trim().parse().unwrap_or(0);
    assert_eq!(
        remote_size,
        large_content.len(),
        "File size mismatch after upload"
    );

    // Download and verify
    let downloaded_path = temp_dir.path().join("downloaded_large.bin");
    let start = Instant::now();
    let result = connection.download(&remote_path, &downloaded_path).await;
    let download_time = start.elapsed();

    assert!(result.is_ok(), "Large file download failed: {:?}", result);
    println!("10MB download completed in {:?}", download_time);

    let downloaded = std::fs::read(&downloaded_path).expect("Failed to read downloaded file");
    assert_eq!(
        downloaded.len(),
        large_content.len(),
        "Downloaded file size mismatch"
    );
    assert_eq!(downloaded, large_content, "Downloaded content mismatch");

    // Cleanup
    connection
        .execute("rm -f /tmp/rustible_large_test.bin", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_upload_content_directly() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Upload content directly without local file
    let content = b"Direct content upload test";
    let remote_path = PathBuf::from("/tmp/rustible_direct_upload.txt");
    let result = connection.upload_content(content, &remote_path, None).await;
    assert!(result.is_ok(), "Direct upload failed: {:?}", result);

    // Verify
    let verify = connection
        .execute("cat /tmp/rustible_direct_upload.txt", None)
        .await
        .unwrap();
    assert!(verify.success);
    assert_eq!(verify.stdout.trim(), std::str::from_utf8(content).unwrap());

    // Cleanup
    connection
        .execute("rm -f /tmp/rustible_direct_upload.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

// =============================================================================
// Connection Pooling Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_connection_reuse() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    // Create first connection
    let conn1 = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to get connection");

    let id1 = conn1.identifier().to_string();

    // Execute command
    let result = conn1.execute("echo test1", None).await.unwrap();
    assert!(result.success);

    // Verify connection is still alive after command
    assert!(conn1.is_alive().await, "Connection should still be alive");

    // Execute another command on the same connection
    let result = conn1.execute("echo test2", None).await.unwrap();
    assert!(result.success);

    // Verify we're using the same connection
    assert_eq!(conn1.identifier(), &id1, "Should be the same connection");

    // Cleanup
    conn1.close().await.ok();
}

#[tokio::test]
async fn test_ssh_connection_pool_concurrent() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if config.hosts.len() < 3 {
        eprintln!("Skipping concurrent pool test (need at least 3 hosts)");
        return;
    }

    let semaphore = Arc::new(Semaphore::new(5));

    let mut handles = vec![];

    for (i, host) in config.hosts.iter().take(3).enumerate() {
        let sem = Arc::clone(&semaphore);
        let host = host.clone();
        let user = config.user.clone();
        let key_path = config.key_path.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            let connection = SshConnectionBuilder::new(&host)
                .port(22)
                .user(&user)
                .private_key(&key_path)
                .connect()
                .await
                .expect("Failed to connect");

            // Execute multiple commands
            for j in 0..3 {
                let result = connection
                    .execute(&format!("echo 'host {} command {}'", i, j), None)
                    .await
                    .unwrap();
                assert!(result.success);
            }

            connection.close().await.ok();
        }));
    }

    for handle in handles {
        handle.await.expect("Task panicked");
    }
}

// =============================================================================
// Privilege Escalation Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_privilege_escalation_sudo() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test sudo without password (NOPASSWD configured in test infrastructure)
    let result = connection.execute("sudo whoami", None).await.unwrap();

    assert!(result.success, "sudo whoami failed: {}", result.stderr);
    assert_eq!(
        result.stdout.trim(),
        "root",
        "Should be running as root after sudo"
    );

    // Test sudo with a command that requires root
    let result = connection
        .execute("sudo cat /etc/shadow | head -1", None)
        .await
        .unwrap();
    assert!(
        result.success,
        "Reading shadow file failed: {}",
        result.stderr
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_become_user() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test becoming a different user (requires sudo access)
    let result = connection
        .execute("sudo -u nobody whoami", None)
        .await
        .unwrap();

    assert!(result.success, "sudo -u failed: {}", result.stderr);
    assert_eq!(
        result.stdout.trim(),
        "nobody",
        "Should be running as nobody"
    );

    connection.close().await.ok();
}

// =============================================================================
// Robustness Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_connection_recovery_after_disconnect() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    // First connection
    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    let result = connection.execute("echo first", None).await.unwrap();
    assert!(result.success);

    // Close connection
    connection.close().await.expect("Failed to close");

    // Connection should no longer be alive
    assert!(!connection.is_alive().await);

    // Create new connection
    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to reconnect");

    let result = connection.execute("echo second", None).await.unwrap();
    assert!(result.success);
    assert!(result.stdout.contains("second"));

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_long_running_command() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .timeout(Duration::from_secs(120))
        .connect()
        .await
        .expect("Failed to connect");

    // Run a command that takes a few seconds
    let start = Instant::now();
    let result = connection.execute("sleep 3 && echo done", None).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert!(output.stdout.contains("done"));
    assert!(
        elapsed >= Duration::from_secs(3),
        "Command should have taken at least 3 seconds"
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_concurrent_commands_single_connection() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = Arc::new(
        SshConnectionBuilder::new(host)
            .port(22)
            .user(&config.user)
            .private_key(&config.key_path)
            .connect()
            .await
            .expect("Failed to connect"),
    );

    // Note: Most SSH libraries don't support true concurrent commands on a single
    // connection. This test verifies that sequential rapid commands work correctly.
    let mut handles = vec![];

    for i in 0..5 {
        let conn = Arc::clone(&connection);
        handles.push(tokio::spawn(async move {
            let result = conn
                .execute(&format!("echo 'command {}'", i), None)
                .await
                .expect("Command failed");
            assert!(result.success);
            assert!(result.stdout.contains(&format!("command {}", i)));
            i
        }));
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.await.expect("Task panicked"));
    }

    // All commands should complete
    assert_eq!(results.len(), 5);

    if let Ok(conn) = Arc::try_unwrap(connection) {
        conn.close().await.ok();
    }
}

// =============================================================================
// Path and File System Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_path_exists() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test existing path
    let exists = connection
        .path_exists(&PathBuf::from("/etc/passwd"))
        .await
        .unwrap();
    assert!(exists, "/etc/passwd should exist");

    // Test non-existing path
    let exists = connection
        .path_exists(&PathBuf::from("/nonexistent/path/that/does/not/exist"))
        .await
        .unwrap();
    assert!(!exists, "Non-existent path should not exist");

    // Test directory
    let exists = connection
        .path_exists(&PathBuf::from("/tmp"))
        .await
        .unwrap();
    assert!(exists, "/tmp should exist");

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_is_directory() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Test directory
    let is_dir = connection
        .is_directory(&PathBuf::from("/tmp"))
        .await
        .unwrap();
    assert!(is_dir, "/tmp should be a directory");

    // Test file
    let is_dir = connection
        .is_directory(&PathBuf::from("/etc/passwd"))
        .await
        .unwrap();
    assert!(!is_dir, "/etc/passwd should not be a directory");

    connection.close().await.ok();
}

#[tokio::test]
async fn test_ssh_file_stat() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.first_host().expect("No test hosts configured");

    let connection = SshConnectionBuilder::new(host)
        .port(22)
        .user(&config.user)
        .private_key(&config.key_path)
        .connect()
        .await
        .expect("Failed to connect");

    // Create test file
    connection
        .execute("echo 'test content' > /tmp/rustible_stat_test.txt", None)
        .await
        .expect("Failed to create test file");

    // Get stat
    let stat = connection
        .stat(&PathBuf::from("/tmp/rustible_stat_test.txt"))
        .await
        .expect("Failed to stat file");

    assert!(stat.is_file, "Should be a file");
    assert!(!stat.is_dir, "Should not be a directory");
    assert!(stat.size > 0, "File should have content");
    assert!(stat.mode > 0, "Should have permissions");

    // Cleanup
    connection
        .execute("rm -f /tmp/rustible_stat_test.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

// =============================================================================
// Multi-Host Tests
// =============================================================================

#[tokio::test]
async fn test_ssh_multiple_hosts_parallel() {
    let config = SshTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if config.hosts.len() < 2 {
        eprintln!("Skipping multi-host test (need at least 2 hosts)");
        return;
    }

    let mut handles = vec![];

    for host in config.hosts.iter().take(5) {
        let host = host.clone();
        let user = config.user.clone();
        let key_path = config.key_path.clone();

        handles.push(tokio::spawn(async move {
            let connection = SshConnectionBuilder::new(&host)
                .port(22)
                .user(&user)
                .private_key(&key_path)
                .connect()
                .await
                .expect("Failed to connect");

            // Get hostname
            let result = connection.execute("hostname", None).await.unwrap();
            assert!(result.success);

            let hostname = result.stdout.trim().to_string();
            connection.close().await.ok();
            (host, hostname)
        }));
    }

    let mut results = vec![];
    for handle in handles {
        let (host, hostname) = handle.await.expect("Task panicked");
        println!("Connected to {} (hostname: {})", host, hostname);
        results.push((host, hostname));
    }

    assert!(!results.is_empty(), "Should have connected to some hosts");
}
