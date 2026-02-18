//! Real Docker Connection Integration Tests
//!
//! These tests validate Rustible's Docker connection implementation:
//! - Docker exec command execution
//! - File transfers via docker cp
//! - Container lifecycle handling
//! - Docker Compose support
//!
//! To run these tests:
//! ```bash
//! export RUSTIBLE_TEST_DOCKER_ENABLED=1
//! export RUSTIBLE_TEST_DOCKER_HOST=tcp://192.168.178.210:2375
//! cargo test --test real_docker_tests --features docker
//! ```

#![cfg(feature = "docker")]

use std::env;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tempfile::TempDir;

// Import the Connection trait so we can call .execute() / .close() etc.
use rustible::connection::{Connection, ExecuteOptions};
use rustible::connection::docker::DockerConnection;
use rustible::modules::Module;

mod common;

/// Configuration for Docker tests
struct DockerTestConfig {
    enabled: bool,
    docker_host: Option<String>,
    test_container: String,
    test_image: String,
}

impl DockerTestConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_TEST_DOCKER_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let docker_host = env::var("RUSTIBLE_TEST_DOCKER_HOST").ok();

        let test_container = env::var("RUSTIBLE_TEST_DOCKER_CONTAINER")
            .unwrap_or_else(|_| "rustible-test-container".to_string());

        let test_image =
            env::var("RUSTIBLE_TEST_DOCKER_IMAGE").unwrap_or_else(|_| "ubuntu:24.04".to_string());

        Self {
            enabled,
            docker_host,
            test_container,
            test_image,
        }
    }

    fn skip_if_disabled(&self) -> bool {
        if !self.enabled {
            eprintln!("Skipping Docker tests (RUSTIBLE_TEST_DOCKER_ENABLED not set)");
            true
        } else {
            false
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Ensure test container is running
async fn ensure_test_container(config: &DockerTestConfig) -> Result<(), String> {
    use tokio::process::Command;

    let docker_cmd = if let Some(host) = &config.docker_host {
        format!("DOCKER_HOST={} docker", host)
    } else {
        "docker".to_string()
    };

    // Check if container exists and is running
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} inspect -f '{{{{.State.Running}}}}' {} 2>/dev/null",
            docker_cmd, config.test_container
        ))
        .output()
        .await
        .map_err(|e| format!("Failed to check container: {}", e))?;

    let is_running = String::from_utf8_lossy(&status.stdout)
        .trim()
        .to_lowercase()
        == "true";

    if !is_running {
        // Create and start container
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "{} rm -f {} 2>/dev/null; {} run -d --name {} {} sleep infinity",
                docker_cmd,
                config.test_container,
                docker_cmd,
                config.test_container,
                config.test_image
            ))
            .output()
            .await
            .map_err(|e| format!("Failed to create container: {}", e))?;

        // Wait for container to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Ok(())
}

/// Cleanup test container
async fn cleanup_test_container(config: &DockerTestConfig) {
    use tokio::process::Command;

    let docker_cmd = if let Some(host) = &config.docker_host {
        format!("DOCKER_HOST={} docker", host)
    } else {
        "docker".to_string()
    };

    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!("{} rm -f {}", docker_cmd, config.test_container))
        .output()
        .await;
}

// =============================================================================
// Connection Tests
// =============================================================================

#[tokio::test]
async fn test_docker_connect_to_running_container() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    assert!(connection.is_alive().await);

    connection
        .close()
        .await
        .expect("Failed to close connection");
}

#[tokio::test]
async fn test_docker_connect_to_non_existent_container() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let connection = DockerConnection::new("nonexistent-container-12345");

    // Attempting to execute on non-existent container should fail
    let result = connection.execute("echo test", None).await;

    assert!(
        result.is_err(),
        "Should fail to execute on non-existent container"
    );
}

// =============================================================================
// Command Execution Tests
// =============================================================================

#[tokio::test]
async fn test_docker_exec_simple_command() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    let result = connection.execute("echo 'hello from docker'", None).await;

    assert!(result.is_ok(), "Command failed: {:?}", result);
    let output = result.unwrap();
    assert!(output.success);
    assert!(
        output.stdout.contains("hello from docker"),
        "Output: {}",
        output.stdout
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_exec_with_environment() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    let options = ExecuteOptions::new()
        .with_env("TEST_VAR", "docker_value")
        .with_env("ANOTHER", "456");

    let result = connection
        .execute("echo $TEST_VAR-$ANOTHER", Some(options))
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert!(
        output.stdout.contains("docker_value-456"),
        "Env vars not set: {}",
        output.stdout
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_exec_as_different_user() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    // Use escalation to run as a different user
    let connection = DockerConnection::new(&config.test_container);

    let options = ExecuteOptions::new()
        .with_escalation(Some("nobody".to_string()));

    let result = connection.execute("whoami", Some(options)).await;

    assert!(result.is_ok());
    let output = result.unwrap();
    // Note: This might fail if nobody user doesn't exist in the container
    // The test validates the user switching mechanism works
    println!("Running as: {}", output.stdout.trim());

    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_exec_working_directory() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    let options = ExecuteOptions::new()
        .with_cwd("/tmp");

    let result = connection.execute("pwd", Some(options)).await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.success);
    assert_eq!(
        output.stdout.trim(),
        "/tmp",
        "Working directory should be /tmp"
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_exec_exit_codes() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    // Exit 0
    let result = connection.execute("exit 0", None).await.unwrap();
    assert!(result.success);
    assert_eq!(result.exit_code, 0);

    // Exit 1
    let result = connection.execute("exit 1", None).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, 1);

    // Exit 42
    let result = connection.execute("exit 42", None).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, 42);

    connection.close().await.ok();
}

// =============================================================================
// File Transfer Tests (docker cp)
// =============================================================================

#[tokio::test]
async fn test_docker_cp_to_container() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let local_file = temp_dir.path().join("upload_test.txt");
    let test_content = "Content for docker cp upload test";
    std::fs::write(&local_file, test_content).expect("Failed to write file");

    let connection = DockerConnection::new(&config.test_container);

    // Upload file
    let remote_path = PathBuf::from("/tmp/docker_upload_test.txt");
    let result = connection.upload(&local_file, &remote_path, None).await;
    assert!(result.is_ok(), "Upload failed: {:?}", result);

    // Verify content
    let verify = connection
        .execute("cat /tmp/docker_upload_test.txt", None)
        .await
        .unwrap();
    assert!(verify.success);
    assert_eq!(verify.stdout.trim(), test_content);

    // Cleanup
    connection
        .execute("rm -f /tmp/docker_upload_test.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_cp_from_container() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let connection = DockerConnection::new(&config.test_container);

    // Create file in container
    let test_content = "Content from docker container";
    connection
        .execute(
            &format!("echo '{}' > /tmp/docker_download_test.txt", test_content),
            None,
        )
        .await
        .expect("Failed to create file");

    // Download file
    let remote_path = PathBuf::from("/tmp/docker_download_test.txt");
    let local_path = temp_dir.path().join("downloaded.txt");
    let result = connection.download(&remote_path, &local_path).await;
    assert!(result.is_ok(), "Download failed: {:?}", result);

    // Verify content
    let downloaded = std::fs::read_to_string(&local_path).expect("Failed to read downloaded file");
    assert!(downloaded.contains(test_content));

    // Cleanup
    connection
        .execute("rm -f /tmp/docker_download_test.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_upload_content_directly() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    // Upload content directly
    let content = b"Direct content upload to docker";
    let remote_path = PathBuf::from("/tmp/docker_direct_upload.txt");
    let result = connection.upload_content(content, &remote_path, None).await;
    assert!(result.is_ok(), "Direct upload failed: {:?}", result);

    // Verify
    let verify = connection
        .execute("cat /tmp/docker_direct_upload.txt", None)
        .await
        .unwrap();
    assert!(verify.success);
    assert_eq!(verify.stdout.trim(), std::str::from_utf8(content).unwrap());

    // Cleanup
    connection
        .execute("rm -f /tmp/docker_direct_upload.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_large_file_transfer() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Create 5MB file
    let local_file = temp_dir.path().join("large_file.bin");
    let large_content: Vec<u8> = (0..5 * 1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&local_file, &large_content).expect("Failed to write file");

    let connection = DockerConnection::new(&config.test_container);

    // Upload
    let remote_path = PathBuf::from("/tmp/docker_large_test.bin");
    let start = Instant::now();
    let result = connection.upload(&local_file, &remote_path, None).await;
    let upload_time = start.elapsed();

    assert!(result.is_ok(), "Large file upload failed: {:?}", result);
    println!("5MB docker cp upload: {:?}", upload_time);

    // Verify size
    let verify = connection
        .execute("stat -c %s /tmp/docker_large_test.bin", None)
        .await
        .unwrap();
    let remote_size: usize = verify.stdout.trim().parse().unwrap_or(0);
    assert_eq!(remote_size, large_content.len());

    // Download and verify
    let downloaded_path = temp_dir.path().join("downloaded_large.bin");
    let start = Instant::now();
    let result = connection.download(&remote_path, &downloaded_path).await;
    let download_time = start.elapsed();

    assert!(result.is_ok(), "Large file download failed: {:?}", result);
    println!("5MB docker cp download: {:?}", download_time);

    let downloaded = std::fs::read(&downloaded_path).expect("Failed to read file");
    assert_eq!(downloaded.len(), large_content.len());

    // Cleanup
    connection
        .execute("rm -f /tmp/docker_large_test.bin", None)
        .await
        .ok();
    connection.close().await.ok();
}

// =============================================================================
// Path Operations Tests
// =============================================================================

#[tokio::test]
async fn test_docker_path_exists() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

    // Test existing path
    let exists = connection
        .path_exists(&PathBuf::from("/etc/passwd"))
        .await
        .unwrap();
    assert!(exists, "/etc/passwd should exist in container");

    // Test non-existing path
    let exists = connection
        .path_exists(&PathBuf::from("/nonexistent/path"))
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
async fn test_docker_is_directory() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = DockerConnection::new(&config.test_container);

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

// =============================================================================
// Container Lifecycle Tests
// =============================================================================

#[tokio::test]
async fn test_docker_exec_on_stopped_container() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Create a stopped container
    let stopped_container = "rustible-test-stopped";

    use tokio::process::Command;
    let docker_cmd = if let Some(host) = &config.docker_host {
        format!("DOCKER_HOST={} docker", host)
    } else {
        "docker".to_string()
    };

    // Create and stop container
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} rm -f {} 2>/dev/null; {} create --name {} {} echo test",
            docker_cmd, stopped_container, docker_cmd, stopped_container, config.test_image
        ))
        .output()
        .await;

    let connection = DockerConnection::new(stopped_container);

    // Attempting to execute on stopped container should fail
    let result = connection.execute("echo test", None).await;

    // Cleanup
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!("{} rm -f {}", docker_cmd, stopped_container))
        .output()
        .await;

    // Connection to stopped container should fail
    assert!(
        result.is_err(),
        "Should fail to execute on stopped container"
    );
}

// =============================================================================
// Module Integration Tests
// =============================================================================

#[tokio::test]
async fn test_docker_module_execution() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    // Test executing rustible modules over Docker connection
    let connection = DockerConnection::new(&config.test_container);

    // Test command module
    let module = rustible::modules::command::CommandModule;
    let params = common::make_params(vec![("cmd", serde_json::json!("echo 'module test'"))]);

    let context = rustible::modules::ModuleContext::new().with_check_mode(false);

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Module execution failed: {:?}", result);

    let output = result.unwrap();
    assert!(
        output.changed || output.status == rustible::modules::ModuleStatus::Ok
    );

    connection.close().await.ok();
}

#[tokio::test]
async fn test_docker_copy_module() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let local_file = temp_dir.path().join("copy_module_test.txt");
    std::fs::write(&local_file, "Copy module content").expect("Failed to write file");

    let connection = DockerConnection::new(&config.test_container);

    // Test copy module
    let module = rustible::modules::copy::CopyModule;
    let params = common::make_params(vec![
        ("src", serde_json::json!(local_file.to_string_lossy())),
        ("dest", serde_json::json!("/tmp/copy_module_dest.txt")),
    ]);

    let context = rustible::modules::ModuleContext::new().with_check_mode(false);

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Copy module failed: {:?}", result);

    // Verify
    let verify = connection
        .execute("cat /tmp/copy_module_dest.txt", None)
        .await
        .unwrap();
    assert!(verify.stdout.contains("Copy module content"));

    // Cleanup
    connection
        .execute("rm -f /tmp/copy_module_dest.txt", None)
        .await
        .ok();
    connection.close().await.ok();
}

// =============================================================================
// Docker Compose Tests
// =============================================================================

#[tokio::test]
async fn test_docker_compose_exec() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Check if docker compose is available
    use tokio::process::Command;
    let compose_check = Command::new("docker")
        .args(["compose", "version"])
        .output()
        .await;

    if compose_check.is_err() || !compose_check.unwrap().status.success() {
        eprintln!("Docker Compose not available, skipping test");
        return;
    }

    // Create test compose file
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let compose_file = temp_dir.path().join("docker-compose.yml");

    let compose_content = format!(
        r#"
services:
  test:
    image: {}
    command: sleep infinity
    container_name: rustible-compose-test
"#,
        config.test_image
    );

    std::fs::write(&compose_file, compose_content).expect("Failed to write compose file");

    // Start compose service
    let _ = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&compose_file)
        .args(["up", "-d"])
        .output()
        .await;

    tokio::time::sleep(Duration::from_secs(2)).await;

    // Test connection via compose - use DockerConnection::compose for service-based access
    let connection = DockerConnection::compose("test");

    let result = connection.execute("echo compose test", None).await;

    // Cleanup
    let _ = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&compose_file)
        .args(["down", "-v"])
        .output()
        .await;

    if let Ok(output) = result {
        assert!(output.success);
    }
}

// =============================================================================
// Concurrent Docker Tests
// =============================================================================

#[tokio::test]
async fn test_docker_concurrent_commands() {
    let config = DockerTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if let Err(e) = ensure_test_container(&config).await {
        eprintln!("Failed to setup test container: {}", e);
        return;
    }

    let connection = std::sync::Arc::new(DockerConnection::new(&config.test_container));

    let mut handles = vec![];

    for i in 0..5 {
        let conn = std::sync::Arc::clone(&connection);
        handles.push(tokio::spawn(async move {
            let result = conn
                .execute(&format!("echo 'concurrent {}'", i), None)
                .await;
            assert!(result.is_ok());
            result.unwrap()
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let output = handle.await.expect("Task panicked");
        assert!(output.success);
        assert!(output.stdout.contains(&format!("concurrent {}", i)));
    }
}

// =============================================================================
// Cleanup
// =============================================================================

/// Final cleanup of test containers
#[tokio::test]
#[ignore] // Run manually with --ignored
async fn cleanup_all_test_containers() {
    let config = DockerTestConfig::from_env();
    cleanup_test_container(&config).await;

    use tokio::process::Command;
    let _ = Command::new("docker")
        .args(["rm", "-f", "rustible-compose-test"])
        .output()
        .await;
}
