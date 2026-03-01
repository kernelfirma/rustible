//! Agent-backed connection wrapper.
//!
//! This connection delegates non-command operations to an underlying transport
//! (typically SSH) while executing commands via the Rustible agent binary.
//! The agent invocation is currently one-shot per command.

use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::agent::{AgentMethod, AgentRequest, AgentResponse, ExecuteParams, ExecuteResult};
use crate::utils::shell_escape;

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

/// Connection wrapper that executes commands via rustible-agent.
pub struct AgentConnection {
    inner: Arc<dyn Connection + Send + Sync>,
    agent_path: String,
    agent_socket: Option<String>,
}

impl AgentConnection {
    /// Create a new agent connection wrapper.
    pub fn new(
        inner: Arc<dyn Connection + Send + Sync>,
        agent_path: impl Into<String>,
        agent_socket: Option<String>,
    ) -> Self {
        Self {
            inner,
            agent_path: agent_path.into(),
            agent_socket,
        }
    }

    fn build_agent_command(&self, request: &AgentRequest) -> ConnectionResult<String> {
        let payload = serde_json::to_string(request).map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to serialize agent request: {}", e))
        })?;

        Ok(format!(
            "{} --request {}",
            shell_escape(&self.agent_path),
            shell_escape(&payload)
        ))
    }

    fn wrap_with_escalation(command: &str, options: &ExecuteOptions) -> String {
        if !options.escalate {
            return command.to_string();
        }

        let user = options.escalate_user.as_deref().unwrap_or("root");
        let method = options.escalate_method.as_deref().unwrap_or("sudo");
        let escaped_user = shell_escape(user);
        let escaped_cmd = shell_escape(command);

        match method {
            "sudo" => format!("sudo -u {} -- sh -c {}", escaped_user, escaped_cmd),
            "su" => format!("su - {} -c {}", escaped_user, escaped_cmd),
            "doas" => format!("doas -u {} sh -c {}", escaped_user, escaped_cmd),
            _ => format!("sudo -u {} -- sh -c {}", escaped_user, escaped_cmd),
        }
    }

    fn decode_agent_response(&self, output: &str) -> ConnectionResult<ExecuteResult> {
        let trimmed = output.trim();
        let response: AgentResponse = match serde_json::from_str(trimmed) {
            Ok(resp) => resp,
            Err(_) => {
                let last_line = output
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .ok_or_else(|| {
                        ConnectionError::ExecutionFailed("Empty agent response".to_string())
                    })?;

                serde_json::from_str(last_line.trim()).map_err(|e| {
                    ConnectionError::ExecutionFailed(format!(
                        "Failed to decode agent response: {}",
                        e
                    ))
                })?
            }
        };

        if let Some(err) = response.error {
            return Err(ConnectionError::ExecutionFailed(format!(
                "Agent error {}: {}",
                err.code, err.message
            )));
        }

        let result_value = response.result.ok_or_else(|| {
            ConnectionError::ExecutionFailed("Agent response missing result".to_string())
        })?;

        let result: ExecuteResult = serde_json::from_value(result_value).map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Invalid agent execute result: {}", e))
        })?;

        Ok(result)
    }
}

#[async_trait]
impl Connection for AgentConnection {
    fn identifier(&self) -> &str {
        self.inner.identifier()
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    async fn is_alive(&self) -> bool {
        self.inner.is_alive().await
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        // Escalation passwords are not supported in agent mode yet - fall back.
        if options.escalate && options.escalate_password.is_some() {
            return self.inner.execute(command, Some(options)).await;
        }

        let agent_command = Self::wrap_with_escalation(command, &options);
        let params = ExecuteParams {
            command: agent_command,
            cwd: options.cwd.clone(),
            env: options.env.clone(),
            timeout: options.timeout,
            user: None,
            group: None,
            shell: true,
        };

        let request = AgentRequest {
            id: uuid::Uuid::new_v4().to_string(),
            method: AgentMethod::Execute,
            params: Some(
                serde_json::to_value(&params).map_err(|e| {
                    ConnectionError::ExecutionFailed(format!(
                        "Failed to serialize execute params: {}",
                        e
                    ))
                })?,
            ),
            auth_token: None,
        };

        let agent_cmd = self.build_agent_command(&request)?;

        let mut agent_options = ExecuteOptions::new();
        if let Some(socket) = &self.agent_socket {
            agent_options = agent_options.with_env("RUSTIBLE_AGENT_SOCKET", socket.clone());
        }
        if let Some(timeout) = options.timeout {
            agent_options = agent_options.with_timeout(timeout);
        }

        let result = self.inner.execute(&agent_cmd, Some(agent_options)).await?;
        if !result.success {
            return Err(ConnectionError::ExecutionFailed(format!(
                "Agent invocation failed ({}): {}",
                self.agent_path,
                result.combined_output()
            )));
        }

        let exec = self.decode_agent_response(&result.stdout)?;
        Ok(CommandResult {
            exit_code: exec.exit_code,
            stdout: exec.stdout,
            stderr: exec.stderr,
            success: exec.exit_code == 0,
        })
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        self.inner.upload(local_path, remote_path, options).await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        self.inner
            .upload_content(content, remote_path, options)
            .await
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        self.inner.download(remote_path, local_path).await
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        self.inner.download_content(remote_path).await
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        self.inner.path_exists(path).await
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        self.inner.is_directory(path).await
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        self.inner.stat(path).await
    }

    async fn close(&self) -> ConnectionResult<()> {
        self.inner.close().await
    }
}
