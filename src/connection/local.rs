//! Local connection module
//!
//! This module provides local command execution and file operations
//! without any network transport.

use async_trait::async_trait;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, trace};

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

/// Local connection for executing commands on the current host
#[derive(Debug, Clone)]
pub struct LocalConnection {
    /// Identifier for this connection
    identifier: String,
}

impl LocalConnection {
    /// Create a new local connection
    pub fn new() -> Self {
        let identifier = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());

        Self { identifier }
    }

    /// Create a local connection with a custom identifier
    pub fn with_identifier(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }

    /// Build the command with options
    fn build_command(&self, command: &str, options: &ExecuteOptions) -> Command {
        let mut cmd = if options.escalate {
            let escalate_method = options.escalate_method.as_deref().unwrap_or("sudo");
            let escalate_user = options.escalate_user.as_deref().unwrap_or("root");

            match escalate_method {
                "sudo" => {
                    let mut c = Command::new("sudo");
                    c.arg("-u").arg(escalate_user);
                    if options.escalate_password.is_some() {
                        c.arg("-S"); // Read password from stdin
                    }
                    c.arg("--").arg("sh").arg("-c").arg(command);
                    c
                }
                "su" => {
                    let mut c = Command::new("su");
                    c.arg("-").arg(escalate_user).arg("-c").arg(command);
                    c
                }
                "doas" => {
                    let mut c = Command::new("doas");
                    c.arg("-u")
                        .arg(escalate_user)
                        .arg("sh")
                        .arg("-c")
                        .arg(command);
                    c
                }
                _ => {
                    // Default to sudo
                    let mut c = Command::new("sudo");
                    c.arg("-u").arg(escalate_user);
                    c.arg("--").arg("sh").arg("-c").arg(command);
                    c
                }
            }
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };

        // Set working directory
        if let Some(cwd) = &options.cwd {
            cmd.current_dir(cwd);
        }

        // Set environment variables
        for (key, value) in &options.env {
            cmd.env(key, value);
        }

        // Configure stdio
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd
    }
}

impl Default for LocalConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Connection for LocalConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        // Local connection is always alive
        true
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();
        debug!(command = %command, "Executing local command");

        let mut cmd = self.build_command(command, &options);

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to spawn process: {}", e))
        })?;

        // Handle escalation password if needed
        if options.escalate && options.escalate_password.is_some() {
            if let Some(mut stdin) = child.stdin.take() {
                let password = options.escalate_password.as_ref().unwrap();
                stdin
                    .write_all(format!("{}\n", password).as_bytes())
                    .await
                    .map_err(|e| {
                        ConnectionError::ExecutionFailed(format!("Failed to write password: {}", e))
                    })?;
            }
        }

        // Wait for the process with optional timeout
        let output = if let Some(timeout_secs) = options.timeout {
            let timeout = tokio::time::Duration::from_secs(timeout_secs);
            let wait_future = child.wait_with_output();
            match tokio::time::timeout(timeout, wait_future).await {
                Ok(result) => result.map_err(|e| {
                    ConnectionError::ExecutionFailed(format!("Failed to wait for process: {}", e))
                })?,
                Err(_) => {
                    // Timeout occurred
                    return Err(ConnectionError::Timeout(timeout_secs));
                }
            }
        } else {
            child.wait_with_output().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to wait for process: {}", e))
            })?
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        trace!(exit_code = %exit_code, stdout_len = %stdout.len(), stderr_len = %stderr.len(), "Command completed");

        if output.status.success() {
            Ok(CommandResult::success(stdout, stderr))
        } else {
            Ok(CommandResult::failure(exit_code, stdout, stderr))
        }
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();
        debug!(src = %local_path.display(), dst = %remote_path.display(), "Copying file locally");

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    ConnectionError::TransferFailed(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        // Backup existing file if requested
        if options.backup && remote_path.exists() {
            let backup_path = format!("{}.bak", remote_path.display());
            fs::copy(remote_path, &backup_path).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create backup: {}", e))
            })?;
        }

        // Use OpenOptions to set mode atomically at creation if possible
        // This avoids race conditions where the file is created with default permissions
        // and then chmodded, leaving a window of exposure.
        #[cfg(unix)]
        if let Some(mode) = options.mode {
            use std::os::unix::fs::OpenOptionsExt;
            let mut open_options = fs::OpenOptions::new();
            open_options.write(true).create(true).truncate(true);
            open_options.mode(mode);

            // Open destination file with correct mode
            let mut dest_file = open_options.open(remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to open/create destination {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            // Open source file
            let mut src_file = fs::File::open(local_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to open source {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

            // Copy content
            std::io::copy(&mut src_file, &mut dest_file).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to copy content to {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;
        } else {
            // Fallback for non-Unix or when mode is not specified (preserves source perms via fs::copy)
            fs::copy(local_path, remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to copy {} to {}: {}",
                    local_path.display(),
                    remote_path.display(),
                    e
                ))
            })?;
        }

        #[cfg(not(unix))]
        if let Some(_) = options.mode {
            // Fallback for non-Unix where OpenOptionsExt is not available
            // We still copy first then set mode, accepting the race condition
            // as Windows permissions are different anyway.
            fs::copy(local_path, remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to copy {} to {}: {}",
                    local_path.display(),
                    remote_path.display(),
                    e
                ))
            })?;

            if let Some(mode) = options.mode {
                self.set_mode(remote_path, mode)?;
            }
        }

        // Set owner/group if specified
        if options.owner.is_some() || options.group.is_some() {
            self.set_ownership(
                remote_path,
                options.owner.as_deref(),
                options.group.as_deref(),
            )
            .await?;
        }

        Ok(())
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();
        debug!(dst = %remote_path.display(), size = %content.len(), "Writing content locally");

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    ConnectionError::TransferFailed(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        // Backup existing file if requested
        if options.backup && remote_path.exists() {
            let backup_path = format!("{}.bak", remote_path.display());
            fs::copy(remote_path, &backup_path).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create backup: {}", e))
            })?;
        }

        // Use OpenOptions to set mode atomically at creation
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::io::Write;

            let mut open_options = fs::OpenOptions::new();
            open_options.write(true).create(true).truncate(true);

            if let Some(mode) = options.mode {
                open_options.mode(mode);
            }

            // Open/create file and write content
            let mut file = open_options.open(remote_path).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to open/create {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            file.write_all(content).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to write to {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;
        }

        #[cfg(not(unix))]
        {
            // Write the content
            fs::write(remote_path, content).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to write to {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

            // Set permissions if specified
            if let Some(mode) = options.mode {
                self.set_mode(remote_path, mode)?;
            }
        }

        // Set owner/group if specified
        if options.owner.is_some() || options.group.is_some() {
            self.set_ownership(
                remote_path,
                options.owner.as_deref(),
                options.group.as_deref(),
            )
            .await?;
        }

        Ok(())
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(src = %remote_path.display(), dst = %local_path.display(), "Copying file locally");

        // Create parent directories for destination
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::copy(remote_path, local_path).map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to copy {} to {}: {}",
                remote_path.display(),
                local_path.display(),
                e
            ))
        })?;

        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(src = %remote_path.display(), "Reading file content locally");

        fs::read(remote_path).map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to read {}: {}",
                remote_path.display(),
                e
            ))
        })
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        Ok(path.exists())
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        Ok(path.is_dir())
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        let metadata = fs::metadata(path).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to stat {}: {}", path.display(), e))
        })?;

        Ok(FileStat {
            size: metadata.len(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            atime: metadata.atime(),
            mtime: metadata.mtime(),
            is_dir: metadata.is_dir(),
            is_file: metadata.is_file(),
            is_symlink: metadata.is_symlink(),
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        // Nothing to close for local connection
        Ok(())
    }
}

impl LocalConnection {
    /// Set file mode/permissions
    fn set_mode(&self, path: &Path, mode: u32) -> ConnectionResult<()> {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, permissions).map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to set permissions on {}: {}",
                path.display(),
                e
            ))
        })
    }

    /// Set file ownership
    async fn set_ownership(
        &self,
        path: &Path,
        owner: Option<&str>,
        group: Option<&str>,
    ) -> ConnectionResult<()> {
        // Build chown command
        let ownership = match (owner, group) {
            (Some(o), Some(g)) => format!("{}:{}", o, g),
            (Some(o), None) => o.to_string(),
            (None, Some(g)) => format!(":{}", g),
            (None, None) => return Ok(()),
        };

        let command = format!("chown {} {}", ownership, path.display());
        let result = self
            .execute(
                &command,
                Some(ExecuteOptions::new().with_escalation(Some("root".to_string()))),
            )
            .await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to set ownership: {}",
                result.stderr
            )));
        }

        Ok(())
    }
}

/// Execute a one-off local command
pub async fn execute_local(command: &str) -> ConnectionResult<CommandResult> {
    let conn = LocalConnection::new();
    conn.execute(command, None).await
}

/// Execute a one-off local command with options
pub async fn execute_local_with_options(
    command: &str,
    options: ExecuteOptions,
) -> ConnectionResult<CommandResult> {
    let conn = LocalConnection::new();
    conn.execute(command, Some(options)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_execute() {
        let conn = LocalConnection::new();
        let result = conn.execute("echo 'hello world'", None).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn test_local_execute_with_env() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new().with_env("TEST_VAR", "test_value");
        let result = conn.execute("echo $TEST_VAR", Some(options)).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("test_value"));
    }

    #[tokio::test]
    async fn test_local_execute_with_cwd() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new().with_cwd("/tmp");
        let result = conn.execute("pwd", Some(options)).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.contains("/tmp"));
    }

    #[tokio::test]
    async fn test_local_execute_failure() {
        let conn = LocalConnection::new();
        let result = conn.execute("exit 42", None).await.unwrap();

        assert!(!result.success);
        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn test_local_timeout() {
        let conn = LocalConnection::new();
        let options = ExecuteOptions::new().with_timeout(1);
        let result = conn.execute("sleep 10", Some(options)).await;

        assert!(matches!(result, Err(ConnectionError::Timeout(1))));
    }

    #[tokio::test]
    async fn test_local_path_exists() {
        let conn = LocalConnection::new();

        assert!(conn.path_exists(Path::new("/tmp")).await.unwrap());
        assert!(!conn
            .path_exists(Path::new("/nonexistent/path"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_local_is_directory() {
        let conn = LocalConnection::new();

        assert!(conn.is_directory(Path::new("/tmp")).await.unwrap());
        assert!(!conn.is_directory(Path::new("/etc/passwd")).await.unwrap());
    }

    #[tokio::test]
    async fn test_local_upload_download() {
        let conn = LocalConnection::new();
        let temp_dir = tempfile::tempdir().unwrap();

        let src_path = temp_dir.path().join("source.txt");
        let dst_path = temp_dir.path().join("dest.txt");

        // Create source file
        fs::write(&src_path, b"test content").unwrap();

        // Upload
        conn.upload(&src_path, &dst_path, None).await.unwrap();
        assert!(dst_path.exists());

        // Download content
        let content = conn.download_content(&dst_path).await.unwrap();
        assert_eq!(content, b"test content");
    }

    #[tokio::test]
    async fn test_local_upload_content() {
        let conn = LocalConnection::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let dst_path = temp_dir.path().join("content.txt");

        conn.upload_content(b"direct content", &dst_path, None)
            .await
            .unwrap();

        assert!(dst_path.exists());
        let content = fs::read_to_string(&dst_path).unwrap();
        assert_eq!(content, "direct content");
    }

    #[tokio::test]
    async fn test_local_stat() {
        let conn = LocalConnection::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");

        fs::write(&file_path, b"some content").unwrap();

        let stat = conn.stat(&file_path).await.unwrap();
        assert!(stat.is_file);
        assert!(!stat.is_dir);
        assert_eq!(stat.size, 12); // "some content" = 12 bytes
    }

    #[tokio::test]
    async fn test_local_is_alive() {
        let conn = LocalConnection::new();
        assert!(conn.is_alive().await);
    }
}
