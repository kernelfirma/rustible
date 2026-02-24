//! AWS Systems Manager (SSM) Session Manager connection module
//!
//! This module provides connectivity to EC2 instances via AWS SSM
//! Session Manager. It uses the AWS CLI `aws ssm start-session` and
//! `aws ssm send-command` for command execution and file transfer.

use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, trace};

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

/// AWS SSM connection for executing commands on EC2 instances
#[derive(Debug, Clone)]
pub struct SsmConnection {
    /// EC2 instance ID (e.g., i-0123456789abcdef0)
    instance_id: String,
    /// AWS region (e.g., us-east-1)
    region: Option<String>,
    /// AWS profile name
    profile: Option<String>,
    /// AWS CLI executable path
    aws_path: String,
}

impl SsmConnection {
    /// Create a new SSM connection to an EC2 instance
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self {
            instance_id: instance_id.into(),
            region: None,
            profile: None,
            aws_path: "aws".to_string(),
        }
    }

    /// Set the AWS region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the AWS profile
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Set the AWS CLI path
    pub fn with_aws_path(mut self, path: impl Into<String>) -> Self {
        self.aws_path = path.into();
        self
    }

    /// Build base AWS CLI command with common flags
    fn base_command(&self) -> Command {
        let mut cmd = Command::new(&self.aws_path);
        if let Some(region) = &self.region {
            cmd.arg("--region").arg(region);
        }
        if let Some(profile) = &self.profile {
            cmd.arg("--profile").arg(profile);
        }
        cmd
    }

    /// Execute a command via SSM send-command and wait for output
    async fn ssm_send_command(
        &self,
        command: &str,
        options: &ExecuteOptions,
    ) -> ConnectionResult<CommandResult> {
        // Build the shell command with options
        let full_command = self.build_shell_command(command, options);

        let mut cmd = self.base_command();
        cmd.arg("ssm")
            .arg("send-command")
            .arg("--instance-ids")
            .arg(&self.instance_id)
            .arg("--document-name")
            .arg("AWS-RunShellScript")
            .arg("--parameters")
            .arg(format!("commands=[{:?}]", full_command))
            .arg("--output")
            .arg("json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ConnectionError::ExecutionFailed(format!(
                "Failed to execute aws ssm send-command: {}",
                e
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectionError::ExecutionFailed(format!(
                "SSM send-command failed: {}",
                stderr
            )));
        }

        // Parse command ID from JSON output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let command_id = Self::extract_command_id(&stdout)?;

        // Wait for command to complete
        self.wait_for_command(&command_id, options.timeout).await
    }

    /// Build shell command with cwd and env vars
    fn build_shell_command(&self, command: &str, options: &ExecuteOptions) -> String {
        let mut parts = Vec::new();

        // Add environment variables
        for (key, value) in &options.env {
            parts.push(format!("export {}={:?}", key, value));
        }

        // Change directory if specified
        if let Some(cwd) = &options.cwd {
            parts.push(format!("cd {:?}", cwd));
        }

        // Add privilege escalation
        if options.escalate {
            let user = options.escalate_user.as_deref().unwrap_or("root");
            parts.push(format!("sudo -u {} -- sh -c {:?}", user, command));
        } else {
            parts.push(command.to_string());
        }

        parts.join(" && ")
    }

    /// Extract command ID from SSM send-command JSON response
    fn extract_command_id(json_output: &str) -> ConnectionResult<String> {
        // Simple JSON parsing - look for "CommandId": "..."
        if let Some(start) = json_output.find("\"CommandId\"") {
            let rest = &json_output[start..];
            if let Some(colon) = rest.find(':') {
                let after_colon = rest[colon + 1..].trim();
                if after_colon.starts_with('"') {
                    let value_start = 1;
                    if let Some(end) = after_colon[value_start..].find('"') {
                        return Ok(after_colon[value_start..value_start + end].to_string());
                    }
                }
            }
        }
        Err(ConnectionError::ExecutionFailed(
            "Failed to parse CommandId from SSM response".to_string(),
        ))
    }

    /// Wait for an SSM command to complete and return its output
    async fn wait_for_command(
        &self,
        command_id: &str,
        timeout: Option<u64>,
    ) -> ConnectionResult<CommandResult> {
        let max_attempts = timeout.unwrap_or(300) / 2; // Poll every 2 seconds

        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let mut cmd = self.base_command();
            cmd.arg("ssm")
                .arg("get-command-invocation")
                .arg("--command-id")
                .arg(command_id)
                .arg("--instance-id")
                .arg(&self.instance_id)
                .arg("--output")
                .arg("json")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let output = cmd.output().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to get command invocation: {}", e))
            })?;

            let stdout_raw = String::from_utf8_lossy(&output.stdout);

            // Check if status is terminal
            if stdout_raw.contains("\"InProgress\"") || stdout_raw.contains("\"Pending\"") {
                trace!(
                    attempt = attempt,
                    command_id = command_id,
                    "SSM command still running"
                );
                continue;
            }

            // Parse the result
            let ssm_stdout =
                Self::extract_json_field(&stdout_raw, "StandardOutputContent").unwrap_or_default();
            let ssm_stderr =
                Self::extract_json_field(&stdout_raw, "StandardErrorContent").unwrap_or_default();
            let exit_code = Self::extract_json_field(&stdout_raw, "ResponseCode")
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(-1);

            if exit_code == 0 {
                return Ok(CommandResult::success(ssm_stdout, ssm_stderr));
            } else {
                return Ok(CommandResult::failure(exit_code, ssm_stdout, ssm_stderr));
            }
        }

        Err(ConnectionError::Timeout(timeout.unwrap_or(300)))
    }

    /// Extract a string field value from JSON output (simple parser)
    fn extract_json_field(json: &str, field: &str) -> Option<String> {
        let pattern = format!("\"{}\"", field);
        if let Some(start) = json.find(&pattern) {
            let rest = &json[start + pattern.len()..];
            if let Some(colon) = rest.find(':') {
                let after_colon = rest[colon + 1..].trim();
                if after_colon.starts_with('"') {
                    let value_start = 1;
                    // Handle escaped quotes in the value
                    let mut end = 0;
                    let bytes = after_colon[value_start..].as_bytes();
                    while end < bytes.len() {
                        if bytes[end] == b'"' && (end == 0 || bytes[end - 1] != b'\\') {
                            break;
                        }
                        end += 1;
                    }
                    return Some(
                        after_colon[value_start..value_start + end]
                            .replace("\\n", "\n")
                            .replace("\\\"", "\""),
                    );
                }
                // Handle numeric values
                let value_end = after_colon
                    .find(|c: char| c == ',' || c == '}' || c == '\n')
                    .unwrap_or(after_colon.len());
                let value = after_colon[..value_end].trim().trim_matches('"');
                return Some(value.to_string());
            }
        }
        None
    }
}

#[async_trait]
impl Connection for SsmConnection {
    fn identifier(&self) -> &str {
        &self.instance_id
    }

    async fn is_alive(&self) -> bool {
        // Check instance SSM connectivity via ping
        let mut cmd = self.base_command();
        cmd.arg("ssm")
            .arg("describe-instance-information")
            .arg("--filters")
            .arg(format!("Key=InstanceIds,Values={}", self.instance_id))
            .arg("--output")
            .arg("json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains("\"Online\"")
            }
            Err(_) => false,
        }
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        debug!(
            instance_id = %self.instance_id,
            command = %command,
            "Executing command via AWS SSM"
        );

        self.ssm_send_command(command, &options).await
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
            instance_id = %self.instance_id,
            "Uploading file via SSM (base64 encoding)"
        );

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                let mkdir_cmd = format!("mkdir -p {}", parent.display());
                self.execute(&mkdir_cmd, None).await?;
            }
        }

        // Read file content and base64 encode it
        let content = std::fs::read(local_path).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read local file: {}", e))
        })?;

        self.upload_content(&content, remote_path, Some(options))
            .await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);

        // Write via base64 decode on remote
        let write_cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_path.display());
        let result = self.execute(&write_cmd, None).await?;
        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to write file: {}",
                result.stderr
            )));
        }

        // Set permissions if specified
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

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            instance_id = %self.instance_id,
            "Downloading file via SSM (base64 encoding)"
        );

        let content = self.download_content(remote_path).await?;

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create local directory: {}", e))
            })?;
        }

        std::fs::write(local_path, content).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write local file: {}", e))
        })?;

        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(
            remote = %remote_path.display(),
            instance_id = %self.instance_id,
            "Downloading content via SSM"
        );

        // Base64 encode on remote and decode locally
        let cmd = format!("base64 {}", remote_path.display());
        let result = self.execute(&cmd, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to read file: {}",
                result.stderr
            )));
        }

        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(result.stdout.trim())
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to decode base64: {}", e)))
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
    fn test_ssm_connection_new() {
        let conn = SsmConnection::new("i-0123456789abcdef0");
        assert_eq!(conn.instance_id, "i-0123456789abcdef0");
        assert_eq!(conn.aws_path, "aws");
        assert!(conn.region.is_none());
        assert!(conn.profile.is_none());
    }

    #[test]
    fn test_ssm_connection_with_options() {
        let conn = SsmConnection::new("i-abc123")
            .with_region("us-west-2")
            .with_profile("myprofile");
        assert_eq!(conn.region, Some("us-west-2".to_string()));
        assert_eq!(conn.profile, Some("myprofile".to_string()));
    }

    #[test]
    fn test_extract_command_id() {
        let json = r#"{"Command": {"CommandId": "abc-123-def", "Status": "Pending"}}"#;
        let id = SsmConnection::extract_command_id(json).unwrap();
        assert_eq!(id, "abc-123-def");
    }

    #[test]
    fn test_extract_command_id_missing() {
        let json = r#"{"error": "something"}"#;
        assert!(SsmConnection::extract_command_id(json).is_err());
    }

    #[test]
    fn test_extract_json_field() {
        let json = r#"{"StandardOutputContent": "hello world", "ResponseCode": 0}"#;
        assert_eq!(
            SsmConnection::extract_json_field(json, "StandardOutputContent"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn test_build_shell_command_simple() {
        let conn = SsmConnection::new("i-abc");
        let opts = ExecuteOptions::default();
        let cmd = conn.build_shell_command("echo hello", &opts);
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_build_shell_command_with_cwd() {
        let conn = SsmConnection::new("i-abc");
        let opts = ExecuteOptions::new().with_cwd("/tmp");
        let cmd = conn.build_shell_command("ls", &opts);
        assert!(cmd.contains("cd"));
        assert!(cmd.contains("ls"));
    }
}
