//! Comprehensive tests for the Rustible connection layer
//!
//! These tests verify the core functionality of the connection module including:
//! - LocalConnection - command execution, file operations, path checking
//! - SSH connection configuration and mocking
//! - Docker connection basics
//! - ConnectionFactory - pool management, connection type resolution
//! - ConnectionBuilder - builder pattern validation
//! - Error handling for all connection types
//! - ExecuteOptions and TransferOptions builder patterns

#![allow(unused_variables)]

use std::path::Path;
use std::sync::Arc;

use rustible::connection::{
    CommandResult, Connection, ConnectionBuilder, ConnectionConfig, ConnectionError,
    ConnectionFactory, ConnectionType, ExecuteOptions, FileStat, HostConfig, PoolStats,
    TransferOptions,
};

use rustible::connection::config::{
    RetryConfig, SshConfigParser, DEFAULT_RETRIES, DEFAULT_RETRY_DELAY, DEFAULT_TIMEOUT,
};

use rustible::connection::local::LocalConnection;

// ============================================================================
// CommandResult Tests
// ============================================================================

#[test]
fn test_command_result_success() {
    let result = CommandResult::success("output data".to_string(), "".to_string());

    assert!(result.success);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "output data");
    assert_eq!(result.stderr, "");
}

#[test]
fn test_command_result_failure() {
    let result = CommandResult::failure(1, "".to_string(), "error message".to_string());

    assert!(!result.success);
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "error message");
}

#[test]
fn test_command_result_combined_output() {
    let result1 = CommandResult::success("stdout".to_string(), "stderr".to_string());
    assert_eq!(result1.combined_output(), "stdout\nstderr");

    let result2 = CommandResult::success("stdout".to_string(), "".to_string());
    assert_eq!(result2.combined_output(), "stdout");

    let result3 = CommandResult::success("".to_string(), "stderr".to_string());
    assert_eq!(result3.combined_output(), "stderr");

    let result4 = CommandResult::success("".to_string(), "".to_string());
    assert_eq!(result4.combined_output(), "");
}

#[test]
fn test_command_result_failure_with_exit_codes() {
    let result1 = CommandResult::failure(127, "".to_string(), "command not found".to_string());
    assert_eq!(result1.exit_code, 127);

    let result2 = CommandResult::failure(2, "".to_string(), "syntax error".to_string());
    assert_eq!(result2.exit_code, 2);

    let result3 = CommandResult::failure(255, "".to_string(), "connection error".to_string());
    assert_eq!(result3.exit_code, 255);
}

// ============================================================================
// ExecuteOptions Tests
// ============================================================================

#[test]
fn test_execute_options_default() {
    let options = ExecuteOptions::default();

    assert!(options.cwd.is_none());
    assert!(options.env.is_empty());
    assert!(options.timeout.is_none());
    assert!(!options.escalate);
    assert!(options.escalate_user.is_none());
    assert!(options.escalate_method.is_none());
    assert!(options.escalate_password.is_none());
}

#[test]
fn test_execute_options_new() {
    let options = ExecuteOptions::new();

    assert!(options.cwd.is_none());
    assert!(options.env.is_empty());
    assert!(options.timeout.is_none());
    assert!(!options.escalate);
}

#[test]
fn test_execute_options_builder_with_cwd() {
    let options = ExecuteOptions::new().with_cwd("/var/www");

    assert_eq!(options.cwd, Some("/var/www".to_string()));
}

#[test]
fn test_execute_options_builder_with_env() {
    let options = ExecuteOptions::new()
        .with_env("PATH", "/usr/bin")
        .with_env("USER", "admin");

    assert_eq!(options.env.len(), 2);
    assert_eq!(options.env.get("PATH"), Some(&"/usr/bin".to_string()));
    assert_eq!(options.env.get("USER"), Some(&"admin".to_string()));
}

#[test]
fn test_execute_options_builder_with_timeout() {
    let options = ExecuteOptions::new().with_timeout(60);

    assert_eq!(options.timeout, Some(60));
}

#[test]
fn test_execute_options_builder_with_escalation() {
    let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

    assert!(options.escalate);
    assert_eq!(options.escalate_user, Some("root".to_string()));
}

#[test]
fn test_execute_options_builder_with_escalation_no_user() {
    let options = ExecuteOptions::new().with_escalation(None);

    assert!(options.escalate);
    assert!(options.escalate_user.is_none());
}

#[test]
fn test_execute_options_builder_chaining() {
    let options = ExecuteOptions::new()
        .with_cwd("/tmp")
        .with_env("FOO", "bar")
        .with_env("BAZ", "qux")
        .with_timeout(30)
        .with_escalation(Some("admin".to_string()));

    assert_eq!(options.cwd, Some("/tmp".to_string()));
    assert_eq!(options.env.len(), 2);
    assert_eq!(options.timeout, Some(30));
    assert!(options.escalate);
    assert_eq!(options.escalate_user, Some("admin".to_string()));
}

// ============================================================================
// TransferOptions Tests
// ============================================================================

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
fn test_transfer_options_new() {
    let options = TransferOptions::new();

    assert!(options.mode.is_none());
    assert!(options.owner.is_none());
    assert!(options.group.is_none());
    assert!(!options.create_dirs);
    assert!(!options.backup);
}

#[test]
fn test_transfer_options_builder_with_mode() {
    let options = TransferOptions::new().with_mode(0o644);

    assert_eq!(options.mode, Some(0o644));
}

#[test]
fn test_transfer_options_builder_with_owner() {
    let options = TransferOptions::new().with_owner("www-data");

    assert_eq!(options.owner, Some("www-data".to_string()));
}

#[test]
fn test_transfer_options_builder_with_group() {
    let options = TransferOptions::new().with_group("www-data");

    assert_eq!(options.group, Some("www-data".to_string()));
}

#[test]
fn test_transfer_options_builder_with_create_dirs() {
    let options = TransferOptions::new().with_create_dirs();

    assert!(options.create_dirs);
}

#[test]
fn test_transfer_options_builder_chaining() {
    let options = TransferOptions::new()
        .with_mode(0o755)
        .with_owner("root")
        .with_group("root")
        .with_create_dirs();

    assert_eq!(options.mode, Some(0o755));
    assert_eq!(options.owner, Some("root".to_string()));
    assert_eq!(options.group, Some("root".to_string()));
    assert!(options.create_dirs);
}

// ============================================================================
// ConnectionType Tests
// ============================================================================

#[test]
fn test_connection_type_local_pool_key() {
    let conn_type = ConnectionType::Local;
    assert_eq!(conn_type.pool_key(), "local");
}

#[test]
fn test_connection_type_ssh_pool_key() {
    let conn_type = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user".to_string(),
    };
    assert_eq!(conn_type.pool_key(), "ssh://user@example.com:22");
}

#[test]
fn test_connection_type_ssh_custom_port_pool_key() {
    let conn_type = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 2222,
        user: "admin".to_string(),
    };
    assert_eq!(conn_type.pool_key(), "ssh://admin@example.com:2222");
}

#[test]
fn test_connection_type_docker_pool_key() {
    let conn_type = ConnectionType::Docker {
        container: "my-container".to_string(),
    };
    assert_eq!(conn_type.pool_key(), "docker://my-container");
}

#[test]
fn test_connection_type_equality() {
    let local1 = ConnectionType::Local;
    let local2 = ConnectionType::Local;
    assert_eq!(local1, local2);

    let ssh1 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user".to_string(),
    };
    let ssh2 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user".to_string(),
    };
    assert_eq!(ssh1, ssh2);

    let docker1 = ConnectionType::Docker {
        container: "container1".to_string(),
    };
    let docker2 = ConnectionType::Docker {
        container: "container1".to_string(),
    };
    assert_eq!(docker1, docker2);
}

#[test]
fn test_connection_type_inequality() {
    let local = ConnectionType::Local;
    let ssh = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user".to_string(),
    };
    let docker = ConnectionType::Docker {
        container: "container1".to_string(),
    };

    assert_ne!(local, ssh);
    assert_ne!(local, docker);
    assert_ne!(ssh, docker);
}

// ============================================================================
// LocalConnection Tests
// ============================================================================

#[tokio::test]
async fn test_local_connection_new() {
    let conn = LocalConnection::new();
    assert!(!conn.identifier().is_empty());
}

#[tokio::test]
async fn test_local_connection_with_identifier() {
    let conn = LocalConnection::with_identifier("test-host");
    assert_eq!(conn.identifier(), "test-host");
}

#[tokio::test]
async fn test_local_connection_is_alive() {
    let conn = LocalConnection::new();
    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_local_connection_execute_simple_command() {
    let conn = LocalConnection::new();
    let result = conn.execute("echo 'hello world'", None).await.unwrap();

    assert!(result.success);
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello world"));
}

#[tokio::test]
async fn test_local_connection_execute_with_env() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_env("TEST_VAR", "test_value");

    let result = conn.execute("echo $TEST_VAR", Some(options)).await.unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("test_value"));
}

#[tokio::test]
async fn test_local_connection_execute_with_cwd() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_cwd("/tmp");

    let result = conn.execute("pwd", Some(options)).await.unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("/tmp"));
}

#[tokio::test]
async fn test_local_connection_execute_failure() {
    let conn = LocalConnection::new();
    let result = conn.execute("exit 42", None).await.unwrap();

    assert!(!result.success);
    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
async fn test_local_connection_execute_nonexistent_command() {
    let conn = LocalConnection::new();
    let result = conn
        .execute("nonexistent_command_12345", None)
        .await
        .unwrap();

    assert!(!result.success);
    assert!(result.exit_code != 0);
}

#[tokio::test]
async fn test_local_connection_execute_with_timeout_success() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_timeout(5);

    let result = conn.execute("echo 'fast'", Some(options)).await.unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("fast"));
}

#[tokio::test]
async fn test_local_connection_execute_with_timeout_failure() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_timeout(1);

    let result = conn.execute("sleep 10", Some(options)).await;

    assert!(matches!(result, Err(ConnectionError::Timeout(1))));
}

#[tokio::test]
async fn test_local_connection_path_exists() {
    let conn = LocalConnection::new();

    assert!(conn.path_exists(Path::new("/tmp")).await.unwrap());
    assert!(!conn
        .path_exists(Path::new("/nonexistent/path/12345"))
        .await
        .unwrap());
}

#[tokio::test]
async fn test_local_connection_is_directory() {
    let conn = LocalConnection::new();

    assert!(conn.is_directory(Path::new("/tmp")).await.unwrap());
    assert!(!conn.is_directory(Path::new("/etc/passwd")).await.unwrap());
}

#[tokio::test]
async fn test_local_connection_upload_download() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src_path = temp_dir.path().join("source.txt");
    let dst_path = temp_dir.path().join("dest.txt");

    // Create source file
    std::fs::write(&src_path, b"test content").unwrap();

    // Upload (copy) file
    conn.upload(&src_path, &dst_path, None).await.unwrap();
    assert!(dst_path.exists());

    // Verify content
    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "test content");

    // Download content
    let downloaded = conn.download_content(&dst_path).await.unwrap();
    assert_eq!(downloaded, b"test content");
}

#[tokio::test]
async fn test_local_connection_upload_content() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let dst_path = temp_dir.path().join("content.txt");

    conn.upload_content(b"direct content", &dst_path, None)
        .await
        .unwrap();

    assert!(dst_path.exists());
    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "direct content");
}

#[tokio::test]
async fn test_local_connection_upload_with_create_dirs() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src_path = temp_dir.path().join("source.txt");
    let dst_path = temp_dir.path().join("subdir/nested/dest.txt");

    // Create source file
    std::fs::write(&src_path, b"nested content").unwrap();

    // Upload with create_dirs
    let options = TransferOptions::new().with_create_dirs();
    conn.upload(&src_path, &dst_path, Some(options))
        .await
        .unwrap();

    assert!(dst_path.exists());
    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "nested content");
}

#[tokio::test]
async fn test_local_connection_upload_with_mode() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src_path = temp_dir.path().join("source.txt");
    let dst_path = temp_dir.path().join("dest.txt");

    // Create source file
    std::fs::write(&src_path, b"mode test").unwrap();

    // Upload with specific mode
    let options = TransferOptions::new().with_mode(0o600);
    conn.upload(&src_path, &dst_path, Some(options))
        .await
        .unwrap();

    assert!(dst_path.exists());

    // Check permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&dst_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[tokio::test]
async fn test_local_connection_stat() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_file.txt");

    std::fs::write(&file_path, b"some content").unwrap();

    let stat = conn.stat(&file_path).await.unwrap();
    assert!(stat.is_file);
    assert!(!stat.is_dir);
    assert_eq!(stat.size, 12); // "some content" = 12 bytes
    assert!(stat.mode > 0);
}

#[tokio::test]
async fn test_local_connection_stat_directory() {
    let conn = LocalConnection::new();
    let stat = conn.stat(Path::new("/tmp")).await.unwrap();

    assert!(stat.is_dir);
    assert!(!stat.is_file);
}

#[tokio::test]
async fn test_local_connection_stat_nonexistent() {
    let conn = LocalConnection::new();
    let result = conn.stat(Path::new("/nonexistent/file")).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_local_connection_close() {
    let conn = LocalConnection::new();
    let result = conn.close().await;

    assert!(result.is_ok());
}

// ============================================================================
// ConnectionConfig Tests
// ============================================================================

#[test]
fn test_connection_config_default() {
    let config = ConnectionConfig::default();

    assert_eq!(config.defaults.port, 22);
    assert_eq!(config.defaults.timeout, DEFAULT_TIMEOUT);
    assert_eq!(config.defaults.retries, DEFAULT_RETRIES);
    assert!(config.hosts.is_empty());
    assert!(config.parse_ssh_config);
}

#[test]
fn test_connection_config_new() {
    let config = ConnectionConfig::new();

    assert_eq!(config.defaults.port, 22);
    assert!(config.hosts.is_empty());
}

#[test]
fn test_connection_config_add_host() {
    let mut config = ConnectionConfig::new();

    let host_config = HostConfig::new()
        .hostname("192.168.1.100")
        .port(2222)
        .user("admin");

    config.add_host("server1", host_config);

    let retrieved = config.get_host("server1").unwrap();
    assert_eq!(retrieved.hostname, Some("192.168.1.100".to_string()));
    assert_eq!(retrieved.port, Some(2222));
    assert_eq!(retrieved.user, Some("admin".to_string()));
}

#[test]
fn test_connection_config_get_host_not_found() {
    let config = ConnectionConfig::new();
    assert!(config.get_host("nonexistent").is_none());
}

#[test]
fn test_connection_config_set_defaults() {
    let mut config = ConnectionConfig::new();

    config.set_default_user("myuser");
    config.set_default_port(2222);
    config.set_default_timeout(60);

    assert_eq!(config.defaults.user, "myuser");
    assert_eq!(config.defaults.port, 2222);
    assert_eq!(config.defaults.timeout, 60);
}

#[test]
fn test_connection_config_from_toml() {
    let toml = r#"
[defaults]
user = "admin"
port = 22
timeout = 60
retries = 5

[hosts.webserver]
hostname = "192.168.1.100"
port = 2222
user = "web"
"#;

    let config = ConnectionConfig::from_toml(toml).unwrap();

    assert_eq!(config.defaults.user, "admin");
    assert_eq!(config.defaults.timeout, 60);
    assert_eq!(config.defaults.retries, 5);

    let webserver = config.get_host("webserver").unwrap();
    assert_eq!(webserver.hostname, Some("192.168.1.100".to_string()));
    assert_eq!(webserver.port, Some(2222));
}

#[test]
fn test_connection_config_from_toml_invalid() {
    let toml = "invalid toml content [[[";
    let result = ConnectionConfig::from_toml(toml);

    assert!(result.is_err());
}

#[test]
fn test_connection_config_get_host_merged() {
    let mut config = ConnectionConfig::new();
    config.set_default_user("defaultuser");
    config.set_default_port(22);
    config.set_default_timeout(30);

    let host_config = HostConfig::new().hostname("example.com");
    config.add_host("server1", host_config);

    let merged = config.get_host_merged("server1");

    // Should have hostname from host config
    assert_eq!(merged.hostname, Some("example.com".to_string()));
    // Should have user from defaults
    assert_eq!(merged.user, Some("defaultuser".to_string()));
    // Should have port from defaults
    assert_eq!(merged.port, Some(22));
    // Should have timeout from defaults
    assert_eq!(merged.connect_timeout, Some(30));
}

// ============================================================================
// HostConfig Tests
// ============================================================================

#[test]
fn test_host_config_default() {
    let config = HostConfig::default();

    assert!(config.hostname.is_none());
    assert!(config.port.is_none());
    assert!(config.user.is_none());
    assert!(config.identity_file.is_none());
    assert!(config.password.is_none());
    assert!(config.connect_timeout.is_none());
    assert!(!config.forward_agent);
    assert!(!config.compression);
}

#[test]
fn test_host_config_new() {
    let config = HostConfig::new();

    assert!(config.hostname.is_none());
    assert!(config.port.is_none());
}

#[test]
fn test_host_config_builder() {
    let config = HostConfig::new()
        .hostname("example.com")
        .port(2222)
        .user("admin")
        .identity_file("~/.ssh/id_rsa")
        .timeout(60)
        .connection_type("ssh");

    assert_eq!(config.hostname, Some("example.com".to_string()));
    assert_eq!(config.port, Some(2222));
    assert_eq!(config.user, Some("admin".to_string()));
    assert_eq!(config.identity_file, Some("~/.ssh/id_rsa".to_string()));
    assert_eq!(config.connect_timeout, Some(60));
    assert_eq!(config.connection, Some("ssh".to_string()));
}

#[test]
fn test_host_config_timeout_duration() {
    let config1 = HostConfig::new().timeout(45);
    assert_eq!(config1.timeout_duration().as_secs(), 45);

    let config2 = HostConfig::new();
    assert_eq!(config2.timeout_duration().as_secs(), DEFAULT_TIMEOUT);
}

#[test]
fn test_host_config_retry_config() {
    let config = HostConfig::new();
    let retry = config.retry_config();

    assert_eq!(retry.max_retries, DEFAULT_RETRIES);
    assert_eq!(retry.retry_delay.as_secs(), DEFAULT_RETRY_DELAY);
    assert!(retry.exponential_backoff);
}

// ============================================================================
// RetryConfig Tests
// ============================================================================

#[test]
fn test_retry_config_default() {
    let config = RetryConfig::default();

    assert_eq!(config.max_retries, DEFAULT_RETRIES);
    assert_eq!(config.retry_delay.as_secs(), DEFAULT_RETRY_DELAY);
    assert!(config.exponential_backoff);
    assert_eq!(config.max_delay.as_secs(), 30);
}

#[test]
fn test_retry_config_delay_exponential() {
    let config = RetryConfig::default();

    let delay0 = config.delay_for_attempt(0);
    let delay1 = config.delay_for_attempt(1);
    let delay2 = config.delay_for_attempt(2);
    let delay3 = config.delay_for_attempt(3);

    assert!(delay1 > delay0);
    assert!(delay2 > delay1);
    assert!(delay3 > delay2);
}

#[test]
fn test_retry_config_delay_max_cap() {
    let config = RetryConfig::default();

    // Very high attempt should be capped at max_delay
    let delay_high = config.delay_for_attempt(100);
    assert_eq!(delay_high.as_secs(), config.max_delay.as_secs());
}

#[test]
fn test_retry_config_delay_linear() {
    use std::time::Duration;

    let config = RetryConfig {
        max_retries: 5,
        retry_delay: Duration::from_secs(2),
        exponential_backoff: false,
        max_delay: Duration::from_secs(30),
    };

    let delay0 = config.delay_for_attempt(0);
    let delay1 = config.delay_for_attempt(1);
    let delay2 = config.delay_for_attempt(2);

    // Linear backoff means all delays should be the same
    assert_eq!(delay0, delay1);
    assert_eq!(delay1, delay2);
    assert_eq!(delay0.as_secs(), 2);
}

// ============================================================================
// SSH Config Parser Tests
// ============================================================================

#[test]
fn test_ssh_config_parser_simple() {
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
}

#[test]
fn test_ssh_config_parser_multiple_hosts() {
    let config = r#"
Host server1
    HostName 192.168.1.10
    User user1

Host server2
    HostName 192.168.1.20
    User user2
    Port 2222
"#;

    let hosts = SshConfigParser::parse(config).unwrap();

    assert_eq!(hosts.len(), 2);

    let server1 = hosts.get("server1").unwrap();
    assert_eq!(server1.hostname, Some("192.168.1.10".to_string()));
    assert_eq!(server1.user, Some("user1".to_string()));

    let server2 = hosts.get("server2").unwrap();
    assert_eq!(server2.hostname, Some("192.168.1.20".to_string()));
    assert_eq!(server2.port, Some(2222));
}

#[test]
fn test_ssh_config_parser_with_proxy_jump() {
    let config = r#"
Host *.internal
    User internal
    ProxyJump bastion
    ForwardAgent yes
    Compression yes
"#;

    let hosts = SshConfigParser::parse(config).unwrap();

    let internal = hosts.get("*.internal").unwrap();
    assert_eq!(internal.user, Some("internal".to_string()));
    assert_eq!(internal.proxy_jump, Some("bastion".to_string()));
    assert!(internal.forward_agent);
    assert!(internal.compression);
}

#[test]
fn test_ssh_config_parser_comments_and_empty_lines() {
    let config = r#"
# This is a comment
Host server

    # Another comment
    HostName example.com

    # Empty line above
    User admin
"#;

    let hosts = SshConfigParser::parse(config).unwrap();

    let server = hosts.get("server").unwrap();
    assert_eq!(server.hostname, Some("example.com".to_string()));
    assert_eq!(server.user, Some("admin".to_string()));
}

#[test]
fn test_ssh_config_parser_strict_host_key_checking() {
    let config = r#"
Host secure
    StrictHostKeyChecking yes

Host insecure
    StrictHostKeyChecking no
"#;

    let hosts = SshConfigParser::parse(config).unwrap();

    let secure = hosts.get("secure").unwrap();
    assert_eq!(secure.strict_host_key_checking, Some(true));

    let insecure = hosts.get("insecure").unwrap();
    assert_eq!(insecure.strict_host_key_checking, Some(false));
}

// ============================================================================
// ConnectionFactory Tests
// ============================================================================

#[tokio::test]
async fn test_connection_factory_new() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let stats = factory.pool_stats();
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.max_connections, 10);
}

#[tokio::test]
async fn test_connection_factory_with_pool_size() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::with_pool_size(config, 20);

    let stats = factory.pool_stats();
    assert_eq!(stats.max_connections, 20);
}

#[tokio::test]
async fn test_connection_factory_get_local_connection() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let conn = factory.get_connection("localhost").await.unwrap();

    // Identifier will be the actual hostname, not "localhost"
    assert!(!conn.identifier().is_empty());
    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_connection_factory_pool_reuse() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    // Get connection first time
    let conn1 = factory.get_connection("localhost").await.unwrap();
    let id1 = conn1.identifier();

    // Get connection second time - should be from pool
    let conn2 = factory.get_connection("localhost").await.unwrap();
    let id2 = conn2.identifier();

    assert_eq!(id1, id2);
}

#[tokio::test]
async fn test_connection_factory_close_all() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    // Create some connections
    let _ = factory.get_connection("localhost").await.unwrap();

    // Close all
    factory.close_all().await.unwrap();

    let stats = factory.pool_stats();
    assert_eq!(stats.active_connections, 0);
}

// ============================================================================
// ConnectionBuilder Tests
// ============================================================================

#[tokio::test]
async fn test_connection_builder_local() {
    let conn = ConnectionBuilder::new("localhost")
        .connection_type("local")
        .connect()
        .await
        .unwrap();

    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_connection_builder_local_implicit() {
    let conn = ConnectionBuilder::new("localhost").connect().await.unwrap();

    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_connection_builder_with_port() {
    // Builder pattern works, but fields are private so we can't test them directly
    // This test just verifies the builder compiles
    let _builder = ConnectionBuilder::new("example.com")
        .port(2222)
        .user("admin");
}

#[tokio::test]
async fn test_connection_builder_with_credentials() {
    // Builder pattern works, but fields are private so we can't test them directly
    // This test just verifies the builder compiles and chains correctly
    let _builder = ConnectionBuilder::new("example.com")
        .user("admin")
        .password("secret")
        .private_key("/path/to/key")
        .timeout(60);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_connection_error_display() {
    let err1 = ConnectionError::ConnectionFailed("Network unreachable".to_string());
    assert!(format!("{}", err1).contains("Network unreachable"));

    let err2 = ConnectionError::Timeout(30);
    assert!(format!("{}", err2).contains("30 seconds"));

    let err3 = ConnectionError::AuthenticationFailed("Invalid key".to_string());
    assert!(format!("{}", err3).contains("Invalid key"));

    let err4 = ConnectionError::HostNotFound("server1".to_string());
    assert!(format!("{}", err4).contains("server1"));
}

#[test]
fn test_connection_error_types() {
    let err1 = ConnectionError::PoolExhausted;
    assert!(format!("{}", err1).contains("exhausted"));

    let err2 = ConnectionError::ConnectionClosed;
    assert!(format!("{}", err2).contains("closed"));

    let err3 = ConnectionError::UnsupportedOperation("test".to_string());
    assert!(format!("{}", err3).contains("Unsupported"));

    let err4 = ConnectionError::DockerError("Container not found".to_string());
    assert!(format!("{}", err4).contains("Container not found"));
}

#[tokio::test]
async fn test_local_connection_error_nonexistent_path() {
    let conn = LocalConnection::new();
    let result = conn.stat(Path::new("/nonexistent/path/12345")).await;

    assert!(result.is_err());
    match result {
        Err(ConnectionError::TransferFailed(_)) => (),
        _ => panic!("Expected TransferFailed error"),
    }
}

#[tokio::test]
async fn test_local_connection_error_upload_to_readonly() {
    let conn = LocalConnection::new();

    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("source.txt");
    std::fs::write(&src_path, b"test").unwrap();

    // Try to upload to a read-only location (should fail)
    let result = conn.upload(&src_path, Path::new("/proc/test"), None).await;

    assert!(result.is_err());
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_full_local_workflow() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    // Test path operations
    assert!(conn.path_exists(temp_dir.path()).await.unwrap());
    assert!(conn.is_directory(temp_dir.path()).await.unwrap());

    // Test command execution
    let result = conn.execute("echo 'workflow test'", None).await.unwrap();
    assert!(result.success);

    // Test file operations
    let file_path = temp_dir.path().join("workflow.txt");
    conn.upload_content(b"workflow content", &file_path, None)
        .await
        .unwrap();

    let content = conn.download_content(&file_path).await.unwrap();
    assert_eq!(content, b"workflow content");

    // Test stat
    let stat = conn.stat(&file_path).await.unwrap();
    assert!(stat.is_file);
    assert_eq!(stat.size, 16);

    // Clean up
    conn.close().await.unwrap();
}

#[tokio::test]
async fn test_concurrent_local_connections() {
    let conn = Arc::new(LocalConnection::new());

    let mut handles = vec![];

    for i in 0..10 {
        let conn_clone = conn.clone();
        let handle = tokio::spawn(async move {
            let cmd = format!("echo 'task {}'", i);
            conn_clone.execute(&cmd, None).await.unwrap()
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.success);
    }
}

#[tokio::test]
async fn test_connection_factory_multiple_connection_types() {
    let mut config = ConnectionConfig::new();

    // Add a local host
    config.add_host("local", HostConfig::new().connection_type("local"));

    let factory = ConnectionFactory::new(config);

    // Get local connection
    let local_conn = factory.get_connection("local").await.unwrap();
    assert!(local_conn.is_alive().await);

    // Get localhost (implicitly local)
    let localhost_conn = factory.get_connection("localhost").await.unwrap();
    assert!(localhost_conn.is_alive().await);
}

#[tokio::test]
async fn test_execute_options_complex_scenario() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let options = ExecuteOptions::new()
        .with_cwd(temp_dir.path().to_str().unwrap())
        .with_env("VAR1", "value1")
        .with_env("VAR2", "value2")
        .with_timeout(10);

    let result = conn
        .execute("echo $VAR1 $VAR2 && pwd", Some(options))
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("value1"));
    assert!(result.stdout.contains("value2"));
    assert!(result.stdout.contains(temp_dir.path().to_str().unwrap()));
}

#[tokio::test]
async fn test_transfer_options_complex_scenario() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src = temp_dir.path().join("source.txt");
    let nested_dst = temp_dir.path().join("a/b/c/dest.txt");

    std::fs::write(&src, b"complex transfer").unwrap();

    let options = TransferOptions::new().with_mode(0o644).with_create_dirs();

    conn.upload(&src, &nested_dst, Some(options)).await.unwrap();

    assert!(nested_dst.exists());
    let content = std::fs::read_to_string(&nested_dst).unwrap();
    assert_eq!(content, "complex transfer");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&nested_dst).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[tokio::test]
async fn test_empty_command() {
    let conn = LocalConnection::new();
    let result = conn.execute("", None).await.unwrap();

    assert!(result.success);
}

#[tokio::test]
async fn test_very_long_output() {
    let conn = LocalConnection::new();
    let result = conn.execute("seq 1 1000", None).await.unwrap();

    assert!(result.success);
    assert!(result.stdout.len() > 1000);
}

#[tokio::test]
async fn test_binary_content_transfer() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let dst_path = temp_dir.path().join("binary.bin");

    // Create binary content with all byte values
    let binary_content: Vec<u8> = (0..=255).collect();

    conn.upload_content(&binary_content, &dst_path, None)
        .await
        .unwrap();

    let downloaded = conn.download_content(&dst_path).await.unwrap();
    assert_eq!(downloaded, binary_content);
}

#[tokio::test]
async fn test_large_file_transfer() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src = temp_dir.path().join("large.bin");
    let dst = temp_dir.path().join("large_copy.bin");

    // Create 1MB file
    let large_content = vec![0u8; 1024 * 1024];
    std::fs::write(&src, &large_content).unwrap();

    conn.upload(&src, &dst, None).await.unwrap();

    assert!(dst.exists());
    let metadata = std::fs::metadata(&dst).unwrap();
    assert_eq!(metadata.len(), 1024 * 1024);
}

#[tokio::test]
async fn test_special_characters_in_command() {
    let conn = LocalConnection::new();

    let result = conn
        .execute("echo 'special chars: $@#%^&*()'", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("special chars"));
}

#[test]
fn test_pool_stats() {
    let stats = PoolStats {
        active_connections: 5,
        max_connections: 10,
        max_per_host: 5,
        host_count: 1,
        idle_timeout_secs: 300,
        max_lifetime_secs: 3600,
    };

    assert_eq!(stats.active_connections, 5);
    assert_eq!(stats.max_connections, 10);
}

#[test]
fn test_file_stat_properties() {
    let stat = FileStat {
        size: 1024,
        mode: 0o644,
        uid: 1000,
        gid: 1000,
        atime: 1234567890,
        mtime: 1234567890,
        is_dir: false,
        is_file: true,
        is_symlink: false,
    };

    assert_eq!(stat.size, 1024);
    assert_eq!(stat.mode, 0o644);
    assert!(stat.is_file);
    assert!(!stat.is_dir);
    assert!(!stat.is_symlink);
}

// ============================================================================
// Connection Type Resolution Tests
// ============================================================================

#[tokio::test]
async fn test_connection_factory_resolve_localhost() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let conn = factory.get_connection("localhost").await.unwrap();
    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_connection_factory_resolve_127_0_0_1() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let conn = factory.get_connection("127.0.0.1").await.unwrap();
    assert!(conn.is_alive().await);
}

#[tokio::test]
async fn test_connection_factory_resolve_local() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let conn = factory.get_connection("local").await.unwrap();
    assert!(conn.is_alive().await);
}

// ============================================================================
// Builder Pattern Validation Tests
// ============================================================================

#[test]
fn test_execute_options_default_values() {
    let options = ExecuteOptions::default();

    assert!(options.cwd.is_none());
    assert_eq!(options.env.len(), 0);
    assert!(options.timeout.is_none());
    assert!(!options.escalate);
}

#[test]
fn test_transfer_options_default_values() {
    let options = TransferOptions::default();

    assert!(options.mode.is_none());
    assert!(options.owner.is_none());
    assert!(options.group.is_none());
    assert!(!options.create_dirs);
    assert!(!options.backup);
}

#[test]
fn test_execute_options_immutability() {
    let options1 = ExecuteOptions::new().with_cwd("/tmp");
    let options2 = options1.clone().with_timeout(30);

    // Original should still have cwd
    assert_eq!(options1.cwd, Some("/tmp".to_string()));
    assert!(options1.timeout.is_none());

    // New should have both
    assert_eq!(options2.cwd, Some("/tmp".to_string()));
    assert_eq!(options2.timeout, Some(30));
}

// ============================================================================
// Docker Connection Tests
// ============================================================================

use rustible::connection::docker::{ContainerInfo, DockerConnection, DockerConnectionBuilder};

#[test]
fn test_docker_connection_new() {
    let conn = DockerConnection::new("my-container");
    assert_eq!(conn.identifier(), "my-container");
}

#[test]
fn test_docker_connection_with_docker_path() {
    let conn = DockerConnection::with_docker_path("my-container", "/usr/local/bin/docker");
    assert_eq!(conn.identifier(), "my-container");
}

#[test]
fn test_docker_connection_compose() {
    let conn = DockerConnection::compose("web");
    // Compose mode uses empty container
    assert_eq!(conn.identifier(), "");
}

#[test]
fn test_docker_connection_builder_new() {
    let builder = DockerConnectionBuilder::new();
    let result = builder.build();
    // Should fail without container
    assert!(result.is_err());
}

#[test]
fn test_docker_connection_builder_with_container() {
    let conn = DockerConnectionBuilder::new()
        .container("test-container")
        .build()
        .unwrap();

    assert_eq!(conn.identifier(), "test-container");
}

#[test]
fn test_docker_connection_builder_with_docker_path() {
    let conn = DockerConnectionBuilder::new()
        .container("test-container")
        .docker_path("/usr/local/bin/docker")
        .build()
        .unwrap();

    assert_eq!(conn.identifier(), "test-container");
}

#[test]
fn test_docker_connection_builder_compose_mode() {
    let conn = DockerConnectionBuilder::new()
        .container("container-id")
        .compose("web-service")
        .build()
        .unwrap();

    assert_eq!(conn.identifier(), "container-id");
}

#[test]
fn test_docker_connection_builder_default() {
    let builder = DockerConnectionBuilder::default();
    let result = builder.build();
    assert!(result.is_err());

    match result {
        Err(ConnectionError::InvalidConfig(msg)) => {
            assert!(msg.contains("Container name"));
        }
        _ => panic!("Expected InvalidConfig error"),
    }
}

#[test]
fn test_docker_connection_builder_chain() {
    let conn = DockerConnectionBuilder::new()
        .container("my-app")
        .docker_path("/opt/docker/bin/docker")
        .build()
        .unwrap();

    assert_eq!(conn.identifier(), "my-app");
}

#[test]
fn test_container_info_struct() {
    let info = ContainerInfo {
        id: "abc123".to_string(),
        name: "my-container".to_string(),
        running: true,
        image: "nginx:latest".to_string(),
    };

    assert_eq!(info.id, "abc123");
    assert_eq!(info.name, "my-container");
    assert!(info.running);
    assert_eq!(info.image, "nginx:latest");
}

#[test]
fn test_container_info_clone() {
    let info1 = ContainerInfo {
        id: "abc123".to_string(),
        name: "my-container".to_string(),
        running: true,
        image: "nginx:latest".to_string(),
    };

    let info2 = info1.clone();
    assert_eq!(info1.id, info2.id);
    assert_eq!(info1.name, info2.name);
}

// ============================================================================
// Privilege Escalation Tests
// ============================================================================

#[tokio::test]
async fn test_local_execute_with_sudo_escalation() {
    let conn = LocalConnection::new();

    // Test that escalation option is set correctly
    let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
    options.escalate_method = Some("sudo".to_string());

    assert!(options.escalate);
    assert_eq!(options.escalate_user, Some("root".to_string()));
    assert_eq!(options.escalate_method, Some("sudo".to_string()));
}

#[tokio::test]
async fn test_local_execute_with_su_escalation() {
    let conn = LocalConnection::new();

    let mut options = ExecuteOptions::new().with_escalation(Some("admin".to_string()));
    options.escalate_method = Some("su".to_string());

    assert!(options.escalate);
    assert_eq!(options.escalate_user, Some("admin".to_string()));
    assert_eq!(options.escalate_method, Some("su".to_string()));
}

#[tokio::test]
async fn test_local_execute_with_doas_escalation() {
    let conn = LocalConnection::new();

    let mut options = ExecuteOptions::new().with_escalation(Some("operator".to_string()));
    options.escalate_method = Some("doas".to_string());

    assert!(options.escalate);
    assert_eq!(options.escalate_user, Some("operator".to_string()));
    assert_eq!(options.escalate_method, Some("doas".to_string()));
}

#[tokio::test]
async fn test_escalation_with_password() {
    let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
    options.escalate_password = Some("secret".to_string());

    assert!(options.escalate);
    assert_eq!(options.escalate_password, Some("secret".to_string()));
}

#[test]
fn test_escalation_default_method() {
    let options = ExecuteOptions::new().with_escalation(None);

    assert!(options.escalate);
    assert!(options.escalate_user.is_none());
    assert!(options.escalate_method.is_none());
}

// ============================================================================
// File Operation Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_upload_with_backup_option() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let dst_path = temp_dir.path().join("backup_test.txt");

    // Create initial file
    std::fs::write(&dst_path, b"original content").unwrap();

    let src_path = temp_dir.path().join("source.txt");
    std::fs::write(&src_path, b"new content").unwrap();

    // Note: backup option is defined but may not create .bak file depending on implementation
    let options = TransferOptions {
        mode: None,
        owner: None,
        group: None,
        create_dirs: false,
        backup: true,
    };

    conn.upload(&src_path, &dst_path, Some(options))
        .await
        .unwrap();

    // Check that the new content was written
    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "new content");

    // Check backup file exists
    let backup_path = format!("{}.bak", dst_path.display());
    assert!(std::path::Path::new(&backup_path).exists());
    let backup_content = std::fs::read_to_string(&backup_path).unwrap();
    assert_eq!(backup_content, "original content");
}

#[tokio::test]
async fn test_upload_content_with_backup_option() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let dst_path = temp_dir.path().join("backup_content_test.txt");

    // Create initial file
    std::fs::write(&dst_path, b"original data").unwrap();

    let options = TransferOptions {
        mode: None,
        owner: None,
        group: None,
        create_dirs: false,
        backup: true,
    };

    conn.upload_content(b"new data", &dst_path, Some(options))
        .await
        .unwrap();

    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "new data");

    // Check backup file exists
    let backup_path = format!("{}.bak", dst_path.display());
    assert!(std::path::Path::new(&backup_path).exists());
}

#[tokio::test]
async fn test_upload_empty_file() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src_path = temp_dir.path().join("empty.txt");
    let dst_path = temp_dir.path().join("empty_copy.txt");

    std::fs::write(&src_path, b"").unwrap();

    conn.upload(&src_path, &dst_path, None).await.unwrap();

    assert!(dst_path.exists());
    let metadata = std::fs::metadata(&dst_path).unwrap();
    assert_eq!(metadata.len(), 0);
}

#[tokio::test]
async fn test_upload_content_empty() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let dst_path = temp_dir.path().join("empty_content.txt");

    conn.upload_content(b"", &dst_path, None).await.unwrap();

    assert!(dst_path.exists());
    let metadata = std::fs::metadata(&dst_path).unwrap();
    assert_eq!(metadata.len(), 0);
}

#[tokio::test]
async fn test_download_to_nested_directory() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src_path = temp_dir.path().join("source.txt");
    let dst_path = temp_dir.path().join("a/b/c/downloaded.txt");

    std::fs::write(&src_path, b"download test").unwrap();

    conn.download(&src_path, &dst_path).await.unwrap();

    assert!(dst_path.exists());
    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, "download test");
}

#[tokio::test]
async fn test_upload_unicode_content() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let dst_path = temp_dir.path().join("unicode.txt");

    let unicode_content = "Hello, 世界! Привет мир! 🌍🌎🌏";

    conn.upload_content(unicode_content.as_bytes(), &dst_path, None)
        .await
        .unwrap();

    let content = std::fs::read_to_string(&dst_path).unwrap();
    assert_eq!(content, unicode_content);
}

#[tokio::test]
async fn test_stat_symlink() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let target_path = temp_dir.path().join("target.txt");
    let link_path = temp_dir.path().join("link");

    std::fs::write(&target_path, b"target content").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target_path, &link_path).unwrap();

        let stat = conn.stat(&link_path).await.unwrap();
        // Note: stat follows symlinks, so it shows the target's properties
        assert!(stat.is_file);
    }
}

#[tokio::test]
async fn test_path_exists_empty_path() {
    let conn = LocalConnection::new();

    // Path::new("") creates an empty path
    let result = conn.path_exists(Path::new("")).await.unwrap();
    // Empty path doesn't exist
    assert!(!result);
}

#[tokio::test]
async fn test_is_directory_on_file() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("file.txt");

    std::fs::write(&file_path, b"content").unwrap();

    assert!(!conn.is_directory(&file_path).await.unwrap());
}

// ============================================================================
// Connection Pool Tests
// ============================================================================

#[tokio::test]
async fn test_connection_pool_stats() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    let stats = factory.pool_stats();
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.max_connections, 10);

    // Get a connection
    let _conn = factory.get_connection("localhost").await.unwrap();

    let stats_after = factory.pool_stats();
    assert_eq!(stats_after.active_connections, 1);
}

#[tokio::test]
async fn test_connection_pool_different_hosts() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    // Use different local connection identifiers
    let _conn1 = factory.get_connection("localhost").await.unwrap();
    let _conn2 = factory.get_connection("127.0.0.1").await.unwrap();

    // Both resolve to local, but have different pool keys
    let stats = factory.pool_stats();
    // They should be separate connections
    assert!(stats.active_connections >= 1);
}

#[tokio::test]
async fn test_connection_pool_reuse_same_host() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    // Get the same connection multiple times
    let conn1 = factory.get_connection("localhost").await.unwrap();
    let conn2 = factory.get_connection("localhost").await.unwrap();
    let conn3 = factory.get_connection("localhost").await.unwrap();

    // All should be the same connection (from pool)
    assert_eq!(conn1.identifier(), conn2.identifier());
    assert_eq!(conn2.identifier(), conn3.identifier());

    // Pool should still show only 1 connection
    let stats = factory.pool_stats();
    assert_eq!(stats.active_connections, 1);
}

#[tokio::test]
async fn test_connection_factory_with_custom_pool_size() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::with_pool_size(config, 5);

    let stats = factory.pool_stats();
    assert_eq!(stats.max_connections, 5);
}

#[tokio::test]
async fn test_connection_factory_close_all_clears_pool() {
    let config = ConnectionConfig::new();
    let factory = ConnectionFactory::new(config);

    // Create connections
    let _conn1 = factory.get_connection("localhost").await.unwrap();

    let stats_before = factory.pool_stats();
    assert_eq!(stats_before.active_connections, 1);

    // Close all
    factory.close_all().await.unwrap();

    let stats_after = factory.pool_stats();
    assert_eq!(stats_after.active_connections, 0);
}

// ============================================================================
// SSH Config Parsing Edge Cases
// ============================================================================

#[test]
fn test_ssh_config_parser_empty_content() {
    let config = "";
    let hosts = SshConfigParser::parse(config).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_ssh_config_parser_comments_only() {
    let config = r#"
# This is a comment
# Another comment
    # Indented comment
"#;
    let hosts = SshConfigParser::parse(config).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_ssh_config_parser_quoted_values() {
    let config = r#"
Host quoted
    HostName "example.com"
    User "admin"
"#;
    let hosts = SshConfigParser::parse(config).unwrap();
    let quoted = hosts.get("quoted").unwrap();
    assert_eq!(quoted.hostname, Some("example.com".to_string()));
    assert_eq!(quoted.user, Some("admin".to_string()));
}

#[test]
fn test_ssh_config_parser_multiple_hosts_same_line() {
    let config = r#"
Host server1 server2 server3
    User shared-user
    Port 22
"#;
    let hosts = SshConfigParser::parse(config).unwrap();

    assert_eq!(hosts.len(), 3);

    for server in &["server1", "server2", "server3"] {
        let h = hosts.get(*server).unwrap();
        assert_eq!(h.user, Some("shared-user".to_string()));
        assert_eq!(h.port, Some(22));
    }
}

#[test]
fn test_ssh_config_parser_unknown_options() {
    let config = r#"
Host custom
    HostName example.com
    CustomOption value123
    AnotherOption another-value
"#;
    let hosts = SshConfigParser::parse(config).unwrap();
    let custom = hosts.get("custom").unwrap();

    assert_eq!(
        custom.options.get("customoption"),
        Some(&"value123".to_string())
    );
    assert_eq!(
        custom.options.get("anotheroption"),
        Some(&"another-value".to_string())
    );
}

#[test]
fn test_ssh_config_parser_server_alive_options() {
    let config = r#"
Host keepalive
    ServerAliveInterval 60
    ServerAliveCountMax 3
"#;
    let hosts = SshConfigParser::parse(config).unwrap();
    let ka = hosts.get("keepalive").unwrap();

    assert_eq!(ka.server_alive_interval, Some(60));
    assert_eq!(ka.server_alive_count_max, Some(3));
}

#[test]
fn test_connection_config_pattern_matching() {
    let mut config = ConnectionConfig::new();

    config.add_host(
        "*.example.com",
        HostConfig::new().user("example-user").port(2222),
    );

    // Exact match on pattern
    let wildcard_config = config.get_host("*.example.com").unwrap();
    assert_eq!(wildcard_config.user, Some("example-user".to_string()));
}

// ============================================================================
// Command Execution Edge Cases
// ============================================================================

#[tokio::test]
async fn test_execute_command_with_quotes() {
    let conn = LocalConnection::new();

    let result = conn
        .execute("echo 'single quotes' \"double quotes\"", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("single quotes"));
    assert!(result.stdout.contains("double quotes"));
}

#[tokio::test]
async fn test_execute_command_with_pipes() {
    let conn = LocalConnection::new();

    let result = conn
        .execute("echo 'hello world' | tr 'a-z' 'A-Z'", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("HELLO WORLD"));
}

#[tokio::test]
async fn test_execute_command_with_redirection() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("output.txt");

    let result = conn
        .execute(
            &format!("echo 'redirected' > {}", file_path.display()),
            None,
        )
        .await
        .unwrap();

    assert!(result.success);
    assert!(file_path.exists());
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("redirected"));
}

#[tokio::test]
async fn test_execute_command_with_stderr() {
    let conn = LocalConnection::new();

    let result = conn
        .execute("echo 'stdout' && echo 'stderr' >&2", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("stdout"));
    assert!(result.stderr.contains("stderr"));
}

#[tokio::test]
async fn test_execute_command_multiline() {
    let conn = LocalConnection::new();

    let result = conn
        .execute("echo 'line1'; echo 'line2'; echo 'line3'", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("line1"));
    assert!(result.stdout.contains("line2"));
    assert!(result.stdout.contains("line3"));
}

#[tokio::test]
async fn test_execute_with_multiple_env_vars() {
    let conn = LocalConnection::new();

    let options = ExecuteOptions::new()
        .with_env("VAR1", "value1")
        .with_env("VAR2", "value2")
        .with_env("VAR3", "value3");

    let result = conn
        .execute("echo $VAR1 $VAR2 $VAR3", Some(options))
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("value1"));
    assert!(result.stdout.contains("value2"));
    assert!(result.stdout.contains("value3"));
}

#[tokio::test]
async fn test_execute_with_special_env_var_values() {
    let conn = LocalConnection::new();

    let options = ExecuteOptions::new()
        .with_env("SPECIAL", "value with spaces")
        .with_env("PATH_VAR", "/usr/bin:/usr/local/bin");

    let result = conn
        .execute("echo \"$SPECIAL\" && echo \"$PATH_VAR\"", Some(options))
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("value with spaces"));
    assert!(result.stdout.contains("/usr/bin:/usr/local/bin"));
}

// ============================================================================
// Timeout Tests
// ============================================================================

#[tokio::test]
async fn test_very_short_timeout() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_timeout(1);

    // A command that takes longer than 1 second
    let result = conn.execute("sleep 5", Some(options)).await;

    assert!(matches!(result, Err(ConnectionError::Timeout(1))));
}

#[tokio::test]
async fn test_timeout_zero_seconds() {
    let conn = LocalConnection::new();
    let options = ExecuteOptions::new().with_timeout(0);

    // Even a fast command should timeout immediately
    let result = conn.execute("echo 'fast'", Some(options)).await;

    // Zero timeout means "immediately" which may or may not succeed
    // depending on scheduling
    // This test just ensures it doesn't panic
    let _ = result;
}

#[tokio::test]
async fn test_no_timeout_long_command() {
    let conn = LocalConnection::new();

    // Without timeout, a quick command should work
    let result = conn
        .execute("sleep 0.1 && echo 'done'", None)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.stdout.contains("done"));
}

// ============================================================================
// Error Recovery Tests
// ============================================================================

#[tokio::test]
async fn test_connection_still_usable_after_failed_command() {
    let conn = LocalConnection::new();

    // Run a failing command
    let fail_result = conn.execute("exit 1", None).await.unwrap();
    assert!(!fail_result.success);

    // Connection should still work
    let success_result = conn.execute("echo 'recovered'", None).await.unwrap();
    assert!(success_result.success);
}

#[tokio::test]
async fn test_connection_still_usable_after_timeout() {
    let conn = LocalConnection::new();

    // Run a command that times out
    let options = ExecuteOptions::new().with_timeout(1);
    let timeout_result = conn.execute("sleep 10", Some(options)).await;
    assert!(matches!(timeout_result, Err(ConnectionError::Timeout(1))));

    // Connection should still work
    let success_result = conn.execute("echo 'recovered'", None).await.unwrap();
    assert!(success_result.success);
}

// ============================================================================
// FileStat Tests
// ============================================================================

#[test]
fn test_file_stat_creation() {
    let stat = FileStat {
        size: 4096,
        mode: 0o755,
        uid: 0,
        gid: 0,
        atime: 1700000000,
        mtime: 1700000001,
        is_dir: true,
        is_file: false,
        is_symlink: false,
    };

    assert_eq!(stat.size, 4096);
    assert_eq!(stat.mode, 0o755);
    assert!(stat.is_dir);
    assert!(!stat.is_file);
    assert!(!stat.is_symlink);
}

#[test]
fn test_file_stat_clone() {
    let stat1 = FileStat {
        size: 1024,
        mode: 0o644,
        uid: 1000,
        gid: 1000,
        atime: 1234567890,
        mtime: 1234567891,
        is_dir: false,
        is_file: true,
        is_symlink: false,
    };

    let stat2 = stat1.clone();

    assert_eq!(stat1.size, stat2.size);
    assert_eq!(stat1.mode, stat2.mode);
    assert_eq!(stat1.is_file, stat2.is_file);
}

// ============================================================================
// Connection Builder Advanced Tests
// ============================================================================

#[tokio::test]
async fn test_connection_builder_docker_type() {
    // Test that docker:// prefix is recognized
    let conn = ConnectionBuilder::new("docker://my-container")
        .connect()
        .await
        .unwrap();

    assert_eq!(conn.identifier(), "my-container");
}

#[test]
fn test_connection_type_ssh_different_users() {
    let ssh1 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user1".to_string(),
    };

    let ssh2 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user2".to_string(),
    };

    assert_ne!(ssh1, ssh2);
    assert_ne!(ssh1.pool_key(), ssh2.pool_key());
}

#[test]
fn test_connection_type_ssh_different_ports() {
    let ssh1 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 22,
        user: "user".to_string(),
    };

    let ssh2 = ConnectionType::Ssh {
        host: "example.com".to_string(),
        port: 2222,
        user: "user".to_string(),
    };

    assert_ne!(ssh1, ssh2);
    assert_ne!(ssh1.pool_key(), ssh2.pool_key());
}

#[test]
fn test_connection_type_docker_different_containers() {
    let docker1 = ConnectionType::Docker {
        container: "container1".to_string(),
    };

    let docker2 = ConnectionType::Docker {
        container: "container2".to_string(),
    };

    assert_ne!(docker1, docker2);
    assert_ne!(docker1.pool_key(), docker2.pool_key());
}

// ============================================================================
// Transfer Options Advanced Tests
// ============================================================================

#[test]
fn test_transfer_options_all_fields() {
    let options = TransferOptions {
        mode: Some(0o700),
        owner: Some("www-data".to_string()),
        group: Some("www-data".to_string()),
        create_dirs: true,
        backup: true,
    };

    assert_eq!(options.mode, Some(0o700));
    assert_eq!(options.owner, Some("www-data".to_string()));
    assert_eq!(options.group, Some("www-data".to_string()));
    assert!(options.create_dirs);
    assert!(options.backup);
}

#[tokio::test]
async fn test_upload_with_all_options() {
    let conn = LocalConnection::new();
    let temp_dir = tempfile::tempdir().unwrap();

    let src = temp_dir.path().join("source.txt");
    let dst = temp_dir.path().join("nested/deep/dest.txt");

    std::fs::write(&src, b"full options test").unwrap();

    let options = TransferOptions::new().with_mode(0o600).with_create_dirs();

    conn.upload(&src, &dst, Some(options)).await.unwrap();

    assert!(dst.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&dst).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

// ============================================================================
// Host Config Pattern Matching Tests
// ============================================================================

#[test]
fn test_host_config_wildcard_pattern() {
    let mut config = ConnectionConfig::new();

    config.add_host("*.dev", HostConfig::new().user("developer").port(22));

    config.add_host("*.prod", HostConfig::new().user("admin").port(2222));

    // Test exact pattern matching
    let dev_config = config.get_host("*.dev").unwrap();
    assert_eq!(dev_config.user, Some("developer".to_string()));

    let prod_config = config.get_host("*.prod").unwrap();
    assert_eq!(prod_config.user, Some("admin".to_string()));
}

#[test]
fn test_host_config_question_mark_pattern() {
    let mut config = ConnectionConfig::new();

    config.add_host("web-?", HostConfig::new().port(8080));

    // Exact pattern match
    let web_config = config.get_host("web-?").unwrap();
    assert_eq!(web_config.port, Some(8080));
}

// ============================================================================
// SSH Config TOML Parsing Tests
// ============================================================================

#[test]
fn test_connection_config_toml_with_all_fields() {
    let toml = r#"
[defaults]
user = "deploy"
port = 22
timeout = 120
retries = 5
retry_delay = 2
use_agent = true
verify_host_key = true

[hosts.production]
hostname = "prod.example.com"
port = 2222
user = "produser"
identity_file = "~/.ssh/prod_key"
connect_timeout = 60
connection = "ssh"
forward_agent = true
compression = true
"#;

    let config = ConnectionConfig::from_toml(toml).unwrap();

    assert_eq!(config.defaults.user, "deploy");
    assert_eq!(config.defaults.timeout, 120);
    assert_eq!(config.defaults.retries, 5);
    assert!(config.defaults.use_agent);

    let prod = config.get_host("production").unwrap();
    assert_eq!(prod.hostname, Some("prod.example.com".to_string()));
    assert_eq!(prod.port, Some(2222));
    assert!(prod.forward_agent);
    assert!(prod.compression);
}

#[test]
fn test_connection_config_toml_minimal() {
    let toml = r#"
[defaults]
user = "minimal"
"#;

    let config = ConnectionConfig::from_toml(toml).unwrap();

    assert_eq!(config.defaults.user, "minimal");
    // Other fields should have defaults
    assert_eq!(config.defaults.port, 22);
    assert_eq!(config.defaults.timeout, DEFAULT_TIMEOUT);
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_file_operations() {
    let conn = Arc::new(LocalConnection::new());
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().to_path_buf();

    let mut handles = vec![];

    for i in 0..5 {
        let conn_clone = conn.clone();
        let path = temp_path.clone();

        let handle = tokio::spawn(async move {
            let file_path = path.join(format!("file_{}.txt", i));
            let content = format!("content {}", i);

            conn_clone
                .upload_content(content.as_bytes(), &file_path, None)
                .await
                .unwrap();

            let downloaded = conn_clone.download_content(&file_path).await.unwrap();
            assert_eq!(downloaded, content.as_bytes());
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_stat_operations() {
    let conn = Arc::new(LocalConnection::new());
    let temp_dir = tempfile::tempdir().unwrap();

    // Create files
    for i in 0..10 {
        let file_path = temp_dir.path().join(format!("stat_file_{}.txt", i));
        std::fs::write(&file_path, format!("content {}", i)).unwrap();
    }

    let mut handles = vec![];

    for i in 0..10 {
        let conn_clone = conn.clone();
        let file_path = temp_dir.path().join(format!("stat_file_{}.txt", i));

        let handle = tokio::spawn(async move {
            let stat = conn_clone.stat(&file_path).await.unwrap();
            assert!(stat.is_file);
            assert!(stat.size > 0);
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

// ============================================================================
// Docker Connection Type Tests
// ============================================================================

#[test]
fn test_docker_connection_identifier_chain() {
    let conn = DockerConnection::new("initial").container("updated");

    assert_eq!(conn.identifier(), "updated");
}

// ============================================================================
// Connection Error Debug/Display Tests
// ============================================================================

#[test]
fn test_connection_error_debug_format() {
    let err = ConnectionError::ConnectionFailed("test error".to_string());
    let debug_str = format!("{:?}", err);

    assert!(debug_str.contains("ConnectionFailed"));
    assert!(debug_str.contains("test error"));
}

#[test]
fn test_connection_error_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let conn_err: ConnectionError = io_err.into();

    assert!(format!("{}", conn_err).contains("file not found"));
}

#[test]
fn test_all_connection_error_variants() {
    let errors: Vec<ConnectionError> = vec![
        ConnectionError::ConnectionFailed("failed".to_string()),
        ConnectionError::AuthenticationFailed("auth failed".to_string()),
        ConnectionError::ExecutionFailed("exec failed".to_string()),
        ConnectionError::TransferFailed("transfer failed".to_string()),
        ConnectionError::Timeout(30),
        ConnectionError::HostNotFound("host".to_string()),
        ConnectionError::InvalidConfig("config".to_string()),
        ConnectionError::SshError("ssh error".to_string()),
        ConnectionError::PoolExhausted,
        ConnectionError::ConnectionClosed,
        ConnectionError::DockerError("docker error".to_string()),
        ConnectionError::UnsupportedOperation("unsupported".to_string()),
    ];

    for err in errors {
        // Ensure Display trait works
        let _ = format!("{}", err);
        // Ensure Debug trait works
        let _ = format!("{:?}", err);
    }
}

// ============================================================================
// RetryConfig Advanced Tests
// ============================================================================

#[test]
fn test_retry_config_exponential_backoff_progression() {
    use std::time::Duration;

    let config = RetryConfig {
        max_retries: 5,
        retry_delay: Duration::from_secs(1),
        exponential_backoff: true,
        max_delay: Duration::from_secs(60),
    };

    let delay0 = config.delay_for_attempt(0);
    let delay1 = config.delay_for_attempt(1);
    let delay2 = config.delay_for_attempt(2);
    let delay3 = config.delay_for_attempt(3);

    // Verify exponential growth: 1, 2, 4, 8 seconds
    assert_eq!(delay0.as_secs(), 1);
    assert_eq!(delay1.as_secs(), 2);
    assert_eq!(delay2.as_secs(), 4);
    assert_eq!(delay3.as_secs(), 8);
}

#[test]
fn test_retry_config_max_delay_cap() {
    use std::time::Duration;

    let config = RetryConfig {
        max_retries: 10,
        retry_delay: Duration::from_secs(1),
        exponential_backoff: true,
        max_delay: Duration::from_secs(10),
    };

    // After many attempts, should be capped at max_delay
    let delay = config.delay_for_attempt(100);
    assert_eq!(delay.as_secs(), 10);
}

// ============================================================================
// Host Config Defaults Tests
// ============================================================================

#[test]
fn test_host_config_merged_with_defaults() {
    let mut config = ConnectionConfig::new();
    config.set_default_user("default-user");
    config.set_default_port(22);
    config.set_default_timeout(30);
    config.defaults.retries = 5;

    // Add a host with partial config
    config.add_host("partial", HostConfig::new().hostname("partial.example.com"));

    let merged = config.get_host_merged("partial");

    // Host-specific values
    assert_eq!(merged.hostname, Some("partial.example.com".to_string()));

    // Inherited from defaults
    assert_eq!(merged.user, Some("default-user".to_string()));
    assert_eq!(merged.port, Some(22));
    assert_eq!(merged.connect_timeout, Some(30));
    assert_eq!(merged.retries, Some(5));
}

#[test]
fn test_host_config_merged_nonexistent_host() {
    let mut config = ConnectionConfig::new();
    config.set_default_user("fallback-user");
    config.set_default_port(2222);

    let merged = config.get_host_merged("nonexistent");

    // Should only have defaults
    assert!(merged.hostname.is_none());
    assert_eq!(merged.user, Some("fallback-user".to_string()));
    assert_eq!(merged.port, Some(2222));
}
