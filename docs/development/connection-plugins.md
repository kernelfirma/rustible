---
summary: Guide to implementing custom connection plugins for SSH, local, Docker, or other transport backends.
read_when: You need to add support for new connection types or customize existing transport behavior.
---

# Creating Connection Plugins

This guide explains how to create custom connection plugins for Rustible. Connection plugins handle the transport layer for executing commands and transferring files to target systems.

## Overview

A connection plugin in Rustible is a struct that implements the `Connection` trait. Each connection:
- Provides command execution on target systems
- Handles file upload and download operations
- Manages file metadata and path operations
- Supports privilege escalation (become/sudo)

## Built-in Connection Types

Rustible includes several built-in connection types:

| Type | Feature Flag | Description |
|------|--------------|-------------|
| `LocalConnection` | `local` (default) | Execute on the control node |
| `RusshConnection` | `russh` (default) | Pure Rust SSH client (recommended) |
| `SshConnection` | `ssh2-backend` | libssh2 bindings (legacy) |
| `DockerConnection` | `docker` | Execute in Docker containers |

## Connection Trait

The core `Connection` trait is defined in `src/connection/mod.rs`:

```rust
#[async_trait]
pub trait Connection: Send + Sync {
    /// Get the connection identifier (hostname or container name)
    fn identifier(&self) -> &str;

    /// Check if the connection is still alive
    async fn is_alive(&self) -> bool;

    /// Execute a command on the remote host
    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult>;

    /// Upload a file to the remote host
    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()>;

    /// Upload content directly to a remote file
    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()>;

    /// Download a file from the remote host
    async fn download(
        &self,
        remote_path: &Path,
        local_path: &Path,
    ) -> ConnectionResult<()>;

    /// Download file content from the remote host
    async fn download_content(
        &self,
        remote_path: &Path,
    ) -> ConnectionResult<Vec<u8>>;

    /// Check if a path exists on the remote host
    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool>;

    /// Check if a path is a directory on the remote host
    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool>;

    /// Get file stats (size, mode, owner, etc.)
    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat>;

    /// Close the connection
    async fn close(&self) -> ConnectionResult<()>;

    /// Execute multiple commands in batch (default: sequential)
    async fn execute_batch(
        &self,
        commands: &[&str],
        options: Option<ExecuteOptions>,
    ) -> Vec<ConnectionResult<CommandResult>> {
        let mut results = Vec::with_capacity(commands.len());
        for cmd in commands {
            results.push(self.execute(cmd, options.clone()).await);
        }
        results
    }
}
```

## Supporting Types

### ExecuteOptions

Options for command execution:

```rust
#[derive(Debug, Clone, Default)]
pub struct ExecuteOptions {
    /// Working directory for the command
    pub cwd: Option<String>,

    /// Environment variables to set
    pub env: HashMap<String, String>,

    /// Timeout in seconds (None for no timeout)
    pub timeout: Option<u64>,

    /// Run command with privilege escalation (sudo/su)
    pub escalate: bool,

    /// User to escalate to (default: root)
    pub escalate_user: Option<String>,

    /// Method for privilege escalation (sudo, su, doas)
    pub escalate_method: Option<String>,

    /// Password for privilege escalation
    pub escalate_password: Option<String>,
}

impl ExecuteOptions {
    pub fn new() -> Self { Self::default() }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_escalation(mut self, user: Option<String>) -> Self {
        self.escalate = true;
        self.escalate_user = user;
        self
    }
}
```

### TransferOptions

Options for file transfers:

```rust
#[derive(Debug, Clone, Default)]
pub struct TransferOptions {
    /// File mode (permissions) to set
    pub mode: Option<u32>,

    /// Owner to set
    pub owner: Option<String>,

    /// Group to set
    pub group: Option<String>,

    /// Create parent directories if they don't exist
    pub create_dirs: bool,

    /// Backup existing file before overwriting
    pub backup: bool,
}

impl TransferOptions {
    pub fn new() -> Self { Self::default() }

    pub fn with_mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }

    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_create_dirs(mut self) -> Self {
        self.create_dirs = true;
        self
    }
}
```

### CommandResult

Result of command execution:

```rust
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Exit code of the command
    pub exit_code: i32,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// Convenience flag: true if exit_code == 0
    pub success: bool,
}

impl CommandResult {
    pub fn success(stdout: String, stderr: String) -> Self {
        Self { exit_code: 0, stdout, stderr, success: true }
    }

    pub fn failure(exit_code: i32, stdout: String, stderr: String) -> Self {
        Self { exit_code, stdout, stderr, success: false }
    }

    pub fn combined_output(&self) -> String {
        if self.stderr.is_empty() { self.stdout.clone() }
        else if self.stdout.is_empty() { self.stderr.clone() }
        else { format!("{}\n{}", self.stdout, self.stderr) }
    }
}
```

### FileStat

File statistics:

```rust
#[derive(Debug, Clone)]
pub struct FileStat {
    /// File size in bytes
    pub size: u64,

    /// File mode (permissions)
    pub mode: u32,

    /// Owner UID
    pub uid: u32,

    /// Group GID
    pub gid: u32,

    /// Last access time (Unix timestamp)
    pub atime: i64,

    /// Last modification time (Unix timestamp)
    pub mtime: i64,

    /// Is this a directory?
    pub is_dir: bool,

    /// Is this a regular file?
    pub is_file: bool,

    /// Is this a symbolic link?
    pub is_symlink: bool,
}
```

### ConnectionError

Error types for connection operations:

```rust
#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Command execution failed: {0}")]
    ExecutionFailed(String),

    #[error("File transfer failed: {0}")]
    TransferFailed(String),

    #[error("Connection timeout after {0} seconds")]
    Timeout(u64),

    #[error("Host not found: {0}")]
    HostNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Connection pool exhausted")]
    PoolExhausted,

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Docker error: {0}")]
    DockerError(String),

    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
}

pub type ConnectionResult<T> = Result<T, ConnectionError>;
```

## Creating a Custom Connection Plugin

Here's a complete example of a custom connection plugin that connects to a fictional "RemoteExec" service:

```rust
// src/connection/remote_exec.rs

use async_trait::async_trait;
use std::path::Path;
use std::collections::HashMap;

use super::{
    Connection, ConnectionError, ConnectionResult,
    CommandResult, ExecuteOptions, TransferOptions, FileStat,
};

/// Connection to a RemoteExec service
#[derive(Debug)]
pub struct RemoteExecConnection {
    /// Host identifier
    host: String,

    /// API endpoint
    endpoint: String,

    /// Authentication token
    token: String,

    /// Connection state
    connected: bool,
}

impl RemoteExecConnection {
    /// Create a new RemoteExec connection
    pub fn new(host: &str, endpoint: &str, token: &str) -> Self {
        Self {
            host: host.to_string(),
            endpoint: endpoint.to_string(),
            token: token.to_string(),
            connected: false,
        }
    }

    /// Connect to the service
    pub async fn connect(&mut self) -> ConnectionResult<()> {
        // Validate the connection by pinging the endpoint
        // ... implementation details ...
        self.connected = true;
        Ok(())
    }

    /// Build command with escalation
    fn build_escalated_command(&self, command: &str, options: &ExecuteOptions) -> String {
        if !options.escalate {
            return command.to_string();
        }

        let method = options.escalate_method.as_deref().unwrap_or("sudo");
        let user = options.escalate_user.as_deref().unwrap_or("root");

        match method {
            "sudo" => format!("sudo -u {} -- sh -c '{}'", user, command.replace("'", "'\\''")),
            "su" => format!("su - {} -c '{}'", user, command.replace("'", "'\\''")),
            _ => format!("sudo -u {} -- sh -c '{}'", user, command.replace("'", "'\\''")),
        }
    }
}

#[async_trait]
impl Connection for RemoteExecConnection {
    fn identifier(&self) -> &str {
        &self.host
    }

    async fn is_alive(&self) -> bool {
        if !self.connected {
            return false;
        }

        // Send a ping/heartbeat to check connection
        // ... implementation details ...
        true
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        if !self.connected {
            return Err(ConnectionError::ConnectionClosed);
        }

        let options = options.unwrap_or_default();

        // Build the final command with escalation if needed
        let final_command = self.build_escalated_command(command, &options);

        // Handle working directory
        let full_command = if let Some(cwd) = &options.cwd {
            format!("cd {} && {}", cwd, final_command)
        } else {
            final_command
        };

        // Execute via the RemoteExec API
        // ... implementation details ...

        // For this example, we'll simulate success
        Ok(CommandResult::success(
            "command output".to_string(),
            String::new(),
        ))
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if !self.connected {
            return Err(ConnectionError::ConnectionClosed);
        }

        let options = options.unwrap_or_default();

        // Read local file
        let content = tokio::fs::read(local_path).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to read {}: {}", local_path.display(), e
            ))
        })?;

        // Upload using upload_content
        self.upload_content(&content, remote_path, Some(options)).await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if !self.connected {
            return Err(ConnectionError::ConnectionClosed);
        }

        let options = options.unwrap_or_default();

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                self.execute(
                    &format!("mkdir -p '{}'", parent.display()),
                    None,
                ).await?;
            }
        }

        // Upload content via API
        // ... implementation details ...

        // Set permissions if specified
        if let Some(mode) = options.mode {
            self.execute(
                &format!("chmod {:o} '{}'", mode, remote_path.display()),
                None,
            ).await?;
        }

        // Set ownership if specified
        if options.owner.is_some() || options.group.is_some() {
            let ownership = match (&options.owner, &options.group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => return Ok(()),
            };
            self.execute(
                &format!("chown {} '{}'", ownership, remote_path.display()),
                Some(ExecuteOptions::new().with_escalation(Some("root".to_string()))),
            ).await?;
        }

        Ok(())
    }

    async fn download(
        &self,
        remote_path: &Path,
        local_path: &Path,
    ) -> ConnectionResult<()> {
        let content = self.download_content(remote_path).await?;

        // Create parent directories locally
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create directory {}: {}", parent.display(), e
                ))
            })?;
        }

        // Write to local file
        tokio::fs::write(local_path, content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to write {}: {}", local_path.display(), e
            ))
        })
    }

    async fn download_content(
        &self,
        remote_path: &Path,
    ) -> ConnectionResult<Vec<u8>> {
        if !self.connected {
            return Err(ConnectionError::ConnectionClosed);
        }

        // Download content via API
        // ... implementation details ...

        Ok(vec![]) // Placeholder
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        let result = self.execute(
            &format!("test -e '{}'", path.display()),
            None,
        ).await?;

        Ok(result.success)
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        let result = self.execute(
            &format!("test -d '{}'", path.display()),
            None,
        ).await?;

        Ok(result.success)
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        // Use stat command to get file information
        let result = self.execute(
            &format!(
                "stat -c '%s %a %u %g %X %Y' '{}' 2>/dev/null && \
                 test -d '{}' && echo 'd' || test -L '{}' && echo 'l' || echo 'f'",
                path.display(), path.display(), path.display()
            ),
            None,
        ).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to stat {}", path.display()
            )));
        }

        // Parse stat output
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        if lines.len() < 2 {
            return Err(ConnectionError::TransferFailed(
                "Invalid stat output".to_string()
            ));
        }

        let parts: Vec<&str> = lines[0].split_whitespace().collect();
        if parts.len() < 6 {
            return Err(ConnectionError::TransferFailed(
                "Invalid stat output format".to_string()
            ));
        }

        let file_type = lines[1].trim();

        Ok(FileStat {
            size: parts[0].parse().unwrap_or(0),
            mode: u32::from_str_radix(parts[1], 8).unwrap_or(0),
            uid: parts[2].parse().unwrap_or(0),
            gid: parts[3].parse().unwrap_or(0),
            atime: parts[4].parse().unwrap_or(0),
            mtime: parts[5].parse().unwrap_or(0),
            is_dir: file_type == "d",
            is_file: file_type == "f",
            is_symlink: file_type == "l",
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        // Clean up connection resources
        // ... implementation details ...
        Ok(())
    }

    /// Override for batch execution with parallelization
    async fn execute_batch(
        &self,
        commands: &[&str],
        options: Option<ExecuteOptions>,
    ) -> Vec<ConnectionResult<CommandResult>> {
        // This implementation could parallelize commands
        // For now, use default sequential behavior
        let mut results = Vec::with_capacity(commands.len());
        for cmd in commands {
            results.push(self.execute(cmd, options.clone()).await);
        }
        results
    }
}
```

## Connection Builder Pattern

Provide a builder for easy construction:

```rust
/// Builder for RemoteExecConnection
pub struct RemoteExecConnectionBuilder {
    host: String,
    endpoint: Option<String>,
    token: Option<String>,
    timeout: Option<u64>,
}

impl RemoteExecConnectionBuilder {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            endpoint: None,
            token: None,
            timeout: None,
        }
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub async fn connect(self) -> ConnectionResult<RemoteExecConnection> {
        let endpoint = self.endpoint.ok_or_else(|| {
            ConnectionError::InvalidConfig("endpoint is required".to_string())
        })?;

        let token = self.token.ok_or_else(|| {
            ConnectionError::InvalidConfig("token is required".to_string())
        })?;

        let mut conn = RemoteExecConnection::new(&self.host, &endpoint, &token);
        conn.connect().await?;
        Ok(conn)
    }
}
```

## Using the Connection Factory

Register your connection with the `ConnectionFactory`:

```rust
use std::sync::Arc;

impl ConnectionFactory {
    pub async fn create_connection(
        &self,
        conn_type: &ConnectionType,
    ) -> ConnectionResult<Arc<dyn Connection + Send + Sync>> {
        match conn_type {
            ConnectionType::Local => {
                Ok(Arc::new(local::LocalConnection::new()))
            }
            ConnectionType::Ssh { host, port, user } => {
                // ... SSH connection creation ...
            }
            ConnectionType::Docker { container } => {
                Ok(Arc::new(docker::DockerConnection::new(container.clone())))
            }
            // Add your custom connection type
            ConnectionType::RemoteExec { host, endpoint, token } => {
                let conn = RemoteExecConnectionBuilder::new(host)
                    .endpoint(endpoint)
                    .token(token)
                    .connect()
                    .await?;
                Ok(Arc::new(conn))
            }
        }
    }
}
```

## Connection Pooling

For connections that benefit from reuse, implement pooling:

```rust
use parking_lot::RwLock;
use std::collections::HashMap;

pub struct ConnectionPool {
    max_connections: usize,
    connections: HashMap<String, Arc<dyn Connection + Send + Sync>>,
}

impl ConnectionPool {
    pub fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            connections: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<Arc<dyn Connection + Send + Sync>> {
        self.connections.get(key).cloned()
    }

    pub fn put(&mut self, key: String, conn: Arc<dyn Connection + Send + Sync>) {
        if self.connections.len() >= self.max_connections {
            // Evict oldest connection (simple FIFO)
            if let Some(oldest_key) = self.connections.keys().next().cloned() {
                self.connections.remove(&oldest_key);
            }
        }
        self.connections.insert(key, conn);
    }
}
```

## Best Practices

### 1. Handle Timeouts Properly

```rust
async fn execute_with_timeout(
    &self,
    command: &str,
    timeout_secs: Option<u64>,
) -> ConnectionResult<CommandResult> {
    let future = self.execute_internal(command);

    match timeout_secs {
        Some(secs) => {
            match tokio::time::timeout(Duration::from_secs(secs), future).await {
                Ok(result) => result,
                Err(_) => Err(ConnectionError::Timeout(secs)),
            }
        }
        None => future.await,
    }
}
```

### 2. Implement Proper Cleanup

```rust
impl Drop for RemoteExecConnection {
    fn drop(&mut self) {
        if self.connected {
            // Use a runtime handle to run async cleanup
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async {
                    // Cleanup code
                });
            }
        }
    }
}
```

### 3. Support Debug Logging

```rust
use tracing::{debug, trace, warn};

async fn execute(&self, command: &str, options: Option<ExecuteOptions>)
    -> ConnectionResult<CommandResult>
{
    debug!(host = %self.host, command = %command, "Executing command");

    let result = self.execute_internal(command).await?;

    trace!(
        exit_code = %result.exit_code,
        stdout_len = %result.stdout.len(),
        stderr_len = %result.stderr.len(),
        "Command completed"
    );

    if !result.success {
        warn!(
            host = %self.host,
            exit_code = %result.exit_code,
            "Command failed"
        );
    }

    Ok(result)
}
```

### 4. Handle Privilege Escalation

```rust
fn build_escalated_command(&self, command: &str, options: &ExecuteOptions) -> String {
    if !options.escalate {
        return command.to_string();
    }

    let method = options.escalate_method.as_deref().unwrap_or("sudo");
    let user = options.escalate_user.as_deref().unwrap_or("root");

    // Escape single quotes in the command
    let escaped = command.replace("'", "'\\''");

    match method {
        "sudo" => {
            if options.escalate_password.is_some() {
                format!("sudo -S -u {} -- sh -c '{}'", user, escaped)
            } else {
                format!("sudo -u {} -- sh -c '{}'", user, escaped)
            }
        }
        "su" => format!("su - {} -c '{}'", user, escaped),
        "doas" => format!("doas -u {} sh -c '{}'", user, escaped),
        _ => format!("sudo -u {} -- sh -c '{}'", user, escaped),
    }
}
```

## Testing Connection Plugins

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let conn = LocalConnection::new();
        let result = conn.execute("echo 'hello'", None).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_execute_with_env() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new()
            .with_env("TEST_VAR", "test_value");

        let result = conn.execute("echo $TEST_VAR", Some(options)).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("test_value"));
    }

    #[tokio::test]
    async fn test_execute_with_cwd() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new().with_cwd("/tmp");

        let result = conn.execute("pwd", Some(options)).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("/tmp"));
    }

    #[tokio::test]
    async fn test_file_upload_download() {
        let conn = LocalConnection::new();
        let temp = tempdir().unwrap();

        let src = temp.path().join("source.txt");
        let dst = temp.path().join("dest.txt");

        std::fs::write(&src, b"test content").unwrap();

        conn.upload(&src, &dst, None).await.unwrap();

        let content = conn.download_content(&dst).await.unwrap();
        assert_eq!(content, b"test content");
    }

    #[tokio::test]
    async fn test_path_operations() {
        let conn = LocalConnection::new();

        assert!(conn.path_exists(Path::new("/tmp")).await.unwrap());
        assert!(conn.is_directory(Path::new("/tmp")).await.unwrap());
        assert!(!conn.path_exists(Path::new("/nonexistent")).await.unwrap());
    }

    #[tokio::test]
    async fn test_timeout() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new().with_timeout(1);

        let result = conn.execute("sleep 10", Some(options)).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(1))));
    }
}
```

## Summary

1. Implement the `Connection` trait with all required methods
2. Handle `ExecuteOptions` for command customization
3. Support `TransferOptions` for file transfer configuration
4. Return appropriate `ConnectionResult` variants
5. Implement proper privilege escalation support
6. Add connection pooling for better performance
7. Use the builder pattern for easy construction
8. Write comprehensive tests for all operations
