//! Podman connection module
//!
//! This module provides connectivity to Podman containers using the
//! podman CLI commands. It allows executing commands inside containers
//! and copying files to/from containers. The API mirrors the Docker
//! connection module since Podman provides a Docker-compatible CLI.

use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, trace};

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

/// Podman connection for executing commands inside containers
#[derive(Debug, Clone)]
pub struct PodmanConnection {
    /// Container ID or name
    container: String,
    /// Podman executable path (default: "podman")
    podman_path: String,
}

impl PodmanConnection {
    /// Create a new Podman connection
    pub fn new(container: impl Into<String>) -> Self {
        Self {
            container: container.into(),
            podman_path: "podman".to_string(),
        }
    }

    /// Create a new Podman connection with a custom podman path
    pub fn with_podman_path(
        container: impl Into<String>,
        podman_path: impl Into<String>,
    ) -> Self {
        Self {
            container: container.into(),
            podman_path: podman_path.into(),
        }
    }

    /// Build the podman exec command
    fn build_exec_command(&self, command: &str, options: &ExecuteOptions) -> Command {
        let mut cmd = Command::new(&self.podman_path);

        cmd.arg("exec");

        // Keep STDIN open
        cmd.arg("-i");

        // Set user if escalation is requested
        if options.escalate {
            let user = options.escalate_user.as_deref().unwrap_or("root");
            cmd.arg("-u").arg(user);
        }

        // Set working directory
        if let Some(cwd) = &options.cwd {
            cmd.arg("-w").arg(cwd);
        }

        // Set environment variables
        for (key, value) in &options.env {
            cmd.arg("-e").arg(format!("{}={}", key, value));
        }

        cmd.arg(&self.container);

        // Add the actual command
        cmd.arg("sh").arg("-c").arg(command);

        // Configure stdio
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd
    }

    /// Check if container is running
    async fn is_container_running(&self) -> ConnectionResult<bool> {
        let mut cmd = Command::new(&self.podman_path);

        cmd.arg("inspect")
            .arg("--format")
            .arg("{{.State.Running}}")
            .arg(&self.container)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to inspect container: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim() == "true")
    }
}

#[async_trait]
impl Connection for PodmanConnection {
    fn identifier(&self) -> &str {
        &self.container
    }

    async fn is_alive(&self) -> bool {
        self.is_container_running().await.unwrap_or(false)
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        if !self.is_container_running().await? {
            return Err(ConnectionError::ExecutionFailed(format!(
                "Container {} is not running",
                self.container
            )));
        }

        debug!(
            container = %self.container,
            command = %command,
            "Executing command in Podman container"
        );

        let mut cmd = self.build_exec_command(command, &options);

        let child = cmd.spawn().map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to execute podman exec: {}", e))
        })?;

        let output = if let Some(timeout_secs) = options.timeout {
            let timeout = tokio::time::Duration::from_secs(timeout_secs);
            match tokio::time::timeout(timeout, child.wait_with_output()).await {
                Ok(result) => result.map_err(|e| {
                    ConnectionError::ExecutionFailed(format!(
                        "Failed to wait for process: {}",
                        e
                    ))
                })?,
                Err(_) => return Err(ConnectionError::Timeout(timeout_secs)),
            }
        } else {
            child.wait_with_output().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to wait for process: {}", e))
            })?
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        trace!(
            exit_code = %exit_code,
            stdout_len = %stdout.len(),
            stderr_len = %stderr.len(),
            "Podman exec completed"
        );

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

        debug!(
            local = %local_path.display(),
            remote = %remote_path.display(),
            container = %self.container,
            "Uploading file to Podman container"
        );

        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                let mkdir_cmd = format!("mkdir -p {}", parent.display());
                self.execute(&mkdir_cmd, None).await?;
            }
        }

        let mut cmd = Command::new(&self.podman_path);
        cmd.arg("cp")
            .arg(local_path)
            .arg(format!("{}:{}", self.container, remote_path.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to execute podman cp: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectionError::TransferFailed(format!(
                "podman cp failed: {}",
                stderr
            )));
        }

        if let Some(mode) = options.mode {
            let chmod_cmd = format!("chmod {:o} {}", mode, remote_path.display());
            self.execute(&chmod_cmd, None).await?;
        }

        if options.owner.is_some() || options.group.is_some() {
            let ownership = match (&options.owner, &options.group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.to_string(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => return Ok(()),
            };
            let chown_cmd = format!("chown {} {}", ownership, remote_path.display());
            self.execute(&chown_cmd, None).await?;
        }

        Ok(())
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            container = %self.container,
            size = %content.len(),
            "Uploading content to Podman container"
        );

        let temp_file = tempfile::NamedTempFile::new().map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to create temp file: {}", e))
        })?;

        std::fs::write(temp_file.path(), content).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write temp file: {}", e))
        })?;

        self.upload(temp_file.path(), remote_path, options).await
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            container = %self.container,
            "Downloading file from Podman container"
        );

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create local directory: {}",
                    e
                ))
            })?;
        }

        let mut cmd = Command::new(&self.podman_path);
        cmd.arg("cp")
            .arg(format!("{}:{}", self.container, remote_path.display()))
            .arg(local_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to execute podman cp: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectionError::TransferFailed(format!(
                "podman cp failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(
            remote = %remote_path.display(),
            container = %self.container,
            "Downloading content from Podman container"
        );

        let command = format!("cat {}", remote_path.display());
        let result = self.execute(&command, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to read file: {}",
                result.stderr
            )));
        }

        Ok(result.stdout.into_bytes())
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        let command = format!("test -e {} && echo yes || echo no", path.display());
        let result = self.execute(&command, None).await?;
        Ok(result.stdout.trim() == "yes")
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        let command = format!("test -d {} && echo yes || echo no", path.display());
        let result = self.execute(&command, None).await?;
        Ok(result.stdout.trim() == "yes")
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        let command = format!("stat -c '%s|%a|%u|%g|%X|%Y|%F' {}", path.display());
        let result = self.execute(&command, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to stat file: {}",
                result.stderr
            )));
        }

        let parts: Vec<&str> = result.stdout.trim().split('|').collect();
        if parts.len() != 7 {
            return Err(ConnectionError::TransferFailed(
                "Invalid stat output".to_string(),
            ));
        }

        let file_type = parts[6];

        Ok(FileStat {
            size: parts[0].parse().unwrap_or(0),
            mode: u32::from_str_radix(parts[1], 8).unwrap_or(0),
            uid: parts[2].parse().unwrap_or(0),
            gid: parts[3].parse().unwrap_or(0),
            atime: parts[4].parse().unwrap_or(0),
            mtime: parts[5].parse().unwrap_or(0),
            is_dir: file_type.contains("directory"),
            is_file: file_type.contains("regular"),
            is_symlink: file_type.contains("symbolic link"),
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_podman_connection_new() {
        let conn = PodmanConnection::new("my-container");
        assert_eq!(conn.container, "my-container");
        assert_eq!(conn.podman_path, "podman");
    }

    #[test]
    fn test_podman_connection_custom_path() {
        let conn = PodmanConnection::with_podman_path("test", "/usr/local/bin/podman");
        assert_eq!(conn.container, "test");
        assert_eq!(conn.podman_path, "/usr/local/bin/podman");
    }

    #[test]
    fn test_build_exec_command() {
        let conn = PodmanConnection::new("my-container");
        let options = ExecuteOptions::default();
        let _ = conn.build_exec_command("echo hello", &options);
    }

    #[test]
    fn test_build_exec_command_with_options() {
        let conn = PodmanConnection::new("my-container");
        let options = ExecuteOptions::new()
            .with_cwd("/app")
            .with_env("FOO", "bar")
            .with_escalation(Some("root".to_string()));
        let _ = conn.build_exec_command("echo hello", &options);
    }
}
