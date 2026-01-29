//! Agent mode for persistent target execution
//!
//! This module provides a deployable agent binary that runs on target hosts,
//! enabling persistent, low-latency command execution without SSH connection
//! overhead for each task.
//!
//! # Architecture
//!
//! ```text
//! Controller                    Target Host
//! +-------------+              +-------------------+
//! |  Rustible   | ---deploy--> |  rustible-agent   |
//! |  Executor   |              |                   |
//! |             | <--socket--> |  Task Executor    |
//! |             |              |  State Manager    |
//! +-------------+              +-------------------+
//! ```
//!
//! # Agent Lifecycle
//!
//! 1. **Build**: `rustible agent-build` compiles agent for target architecture
//! 2. **Deploy**: Agent binary is transferred to target hosts
//! 3. **Start**: Agent starts and listens on Unix socket or TCP port
//! 4. **Execute**: Controller sends tasks, agent executes and returns results
//! 5. **Shutdown**: Agent terminates on explicit shutdown or timeout
//!
//! # Communication Protocol
//!
//! The agent uses a simple JSON-RPC-like protocol over Unix sockets or TCP:
//!
//! ```json
//! // Request
//! {
//!   "id": "task-123",
//!   "method": "execute",
//!   "params": {
//!     "command": "apt-get install nginx",
//!     "cwd": "/tmp",
//!     "env": {"DEBIAN_FRONTEND": "noninteractive"},
//!     "timeout": 300
//!   }
//! }
//!
//! // Response
//! {
//!   "id": "task-123",
//!   "result": {
//!     "exit_code": 0,
//!     "stdout": "...",
//!     "stderr": "",
//!     "duration_ms": 1234
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Agent errors
#[derive(Error, Debug)]
pub enum AgentError {
    /// Failed to build agent binary
    #[error("Build failed: {0}")]
    BuildFailed(String),

    /// Failed to deploy agent
    #[error("Deploy failed: {0}")]
    DeployFailed(String),

    /// Connection to agent failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Command execution failed
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Agent not running
    #[error("Agent not running on host: {0}")]
    AgentNotRunning(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Timeout
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

/// Result type for agent operations
pub type AgentResult<T> = Result<T, AgentError>;

/// Agent build configuration
#[derive(Debug, Clone)]
pub struct AgentBuildConfig {
    /// Target triple (e.g., "x86_64-unknown-linux-gnu")
    pub target: String,
    /// Release or debug build
    pub release: bool,
    /// Output directory for built binary
    pub output_dir: PathBuf,
    /// Features to enable
    pub features: Vec<String>,
    /// Strip binary for smaller size
    pub strip: bool,
    /// Compress binary with UPX (if available)
    pub compress: bool,
}

impl Default for AgentBuildConfig {
    fn default() -> Self {
        Self {
            target: current_target(),
            release: true,
            output_dir: PathBuf::from("target/agent"),
            features: Vec::new(),
            strip: true,
            compress: false,
        }
    }
}

/// Agent configuration for runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Listen address (Unix socket path or TCP address)
    pub listen: String,
    /// Enable TLS for TCP connections
    pub tls: bool,
    /// Path to TLS certificate
    pub tls_cert: Option<PathBuf>,
    /// Path to TLS key
    pub tls_key: Option<PathBuf>,
    /// Idle timeout before auto-shutdown (0 = never)
    pub idle_timeout: Duration,
    /// Maximum concurrent tasks
    pub max_concurrent: usize,
    /// Working directory for task execution
    pub work_dir: PathBuf,
    /// Log level
    pub log_level: String,
    /// Authentication token (if required)
    pub auth_token: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            listen: "/var/run/rustible-agent.sock".to_string(),
            tls: false,
            tls_cert: None,
            tls_key: None,
            idle_timeout: Duration::from_secs(3600), // 1 hour
            max_concurrent: 10,
            work_dir: PathBuf::from("/tmp/rustible"),
            log_level: "info".to_string(),
            auth_token: None,
        }
    }
}

/// Request message to agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Request ID for correlation
    pub id: String,
    /// Method to invoke
    pub method: AgentMethod,
    /// Request parameters
    pub params: Option<serde_json::Value>,
}

/// Agent methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMethod {
    /// Execute a command
    Execute,
    /// Upload a file
    Upload,
    /// Download a file
    Download,
    /// Check file stat
    Stat,
    /// Create directory
    Mkdir,
    /// Delete file/directory
    Delete,
    /// Get system facts
    Facts,
    /// Ping/health check
    Ping,
    /// Shutdown agent
    Shutdown,
}

/// Response from agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Request ID for correlation
    pub id: String,
    /// Result (on success)
    pub result: Option<serde_json::Value>,
    /// Error (on failure)
    pub error: Option<AgentRpcError>,
}

/// RPC error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

/// Command execution request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteParams {
    /// Command to execute
    pub command: String,
    /// Working directory
    pub cwd: Option<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Timeout in seconds
    pub timeout: Option<u64>,
    /// Run as specific user
    pub user: Option<String>,
    /// Run as specific group
    pub group: Option<String>,
    /// Use shell (vs direct exec)
    pub shell: bool,
}

/// Command execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// Exit code
    pub exit_code: i32,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Agent status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    /// Agent version
    pub version: String,
    /// Uptime in seconds
    pub uptime: u64,
    /// Number of tasks executed
    pub tasks_executed: u64,
    /// Number of tasks currently running
    pub tasks_running: usize,
    /// Host information
    pub host_info: HostInfo,
}

/// Host information collected by agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    /// Hostname
    pub hostname: String,
    /// Operating system
    pub os: String,
    /// Architecture
    pub arch: String,
    /// Number of CPUs
    pub cpus: usize,
    /// Total memory in bytes
    pub memory_total: u64,
    /// Available memory in bytes
    pub memory_available: u64,
}

/// Agent builder for creating deployable binaries
pub struct AgentBuilder {
    config: AgentBuildConfig,
}

impl AgentBuilder {
    /// Create a new agent builder with default configuration
    pub fn new() -> Self {
        Self {
            config: AgentBuildConfig::default(),
        }
    }

    /// Set target architecture
    pub fn target(mut self, target: &str) -> Self {
        self.config.target = target.to_string();
        self
    }

    /// Build in release mode
    pub fn release(mut self, release: bool) -> Self {
        self.config.release = release;
        self
    }

    /// Set output directory
    pub fn output_dir(mut self, dir: PathBuf) -> Self {
        self.config.output_dir = dir;
        self
    }

    /// Enable stripping
    pub fn strip(mut self, strip: bool) -> Self {
        self.config.strip = strip;
        self
    }

    /// Build the agent binary
    pub fn build(&self) -> AgentResult<PathBuf> {
        use std::process::Command;

        // Ensure output directory exists
        std::fs::create_dir_all(&self.config.output_dir)?;

        let mut cmd = Command::new("cargo");
        cmd.arg("build");
        cmd.arg("--bin").arg("rustible-agent");
        cmd.arg("--target").arg(&self.config.target);

        if self.config.release {
            cmd.arg("--release");
        }

        if !self.config.features.is_empty() {
            cmd.arg("--features").arg(self.config.features.join(","));
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AgentError::BuildFailed(stderr.to_string()));
        }

        // Determine binary path
        let profile = if self.config.release {
            "release"
        } else {
            "debug"
        };
        let binary_name = if cfg!(target_os = "windows") {
            "rustible-agent.exe"
        } else {
            "rustible-agent"
        };
        let src_path = PathBuf::from("target")
            .join(&self.config.target)
            .join(profile)
            .join(binary_name);

        let dest_path = self
            .config
            .output_dir
            .join(format!("rustible-agent-{}", self.config.target));

        std::fs::copy(&src_path, &dest_path)?;

        // Strip binary if requested
        if self.config.strip {
            let _ = Command::new("strip").arg(&dest_path).output();
        }

        Ok(dest_path)
    }

    /// Get the build configuration
    pub fn config(&self) -> &AgentBuildConfig {
        &self.config
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent runtime for executing on target hosts
pub struct AgentRuntime {
    config: AgentConfig,
    start_time: Instant,
    tasks_executed: std::sync::atomic::AtomicU64,
    tasks_running: std::sync::atomic::AtomicUsize,
}

impl AgentRuntime {
    /// Create a new agent runtime with configuration
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            start_time: Instant::now(),
            tasks_executed: std::sync::atomic::AtomicU64::new(0),
            tasks_running: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Start the agent runtime
    pub async fn start(&self) -> AgentResult<()> {
        // Ensure work directory exists
        std::fs::create_dir_all(&self.config.work_dir)?;

        // TODO: Implement actual socket listener
        // This is a placeholder for the full implementation

        Ok(())
    }

    /// Execute a command
    pub async fn execute(&self, params: ExecuteParams) -> AgentResult<ExecuteResult> {
        use std::sync::atomic::Ordering;

        self.tasks_running.fetch_add(1, Ordering::SeqCst);
        let start = Instant::now();

        let result = self.execute_inner(params).await;

        self.tasks_running.fetch_sub(1, Ordering::SeqCst);
        self.tasks_executed.fetch_add(1, Ordering::SeqCst);

        match result {
            Ok(mut res) => {
                res.duration_ms = start.elapsed().as_millis() as u64;
                Ok(res)
            }
            Err(e) => Err(e),
        }
    }

    /// Inner execution logic
    async fn execute_inner(&self, params: ExecuteParams) -> AgentResult<ExecuteResult> {
        use std::process::Command;

        let mut cmd = if params.shell {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&params.command);
            c
        } else {
            let parts: Vec<&str> = params.command.split_whitespace().collect();
            if parts.is_empty() {
                return Err(AgentError::ExecutionFailed("Empty command".to_string()));
            }
            let mut c = Command::new(parts[0]);
            if parts.len() > 1 {
                c.args(&parts[1..]);
            }
            c
        };

        // Set working directory
        if let Some(cwd) = &params.cwd {
            cmd.current_dir(cwd);
        } else {
            cmd.current_dir(&self.config.work_dir);
        }

        // Set environment variables
        for (key, value) in &params.env {
            cmd.env(key, value);
        }

        let output = cmd
            .output()
            .map_err(|e| AgentError::ExecutionFailed(format!("Failed to execute: {}", e)))?;

        Ok(ExecuteResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: 0, // Set by caller
        })
    }

    /// Get agent status
    pub fn status(&self) -> AgentStatus {
        use std::sync::atomic::Ordering;

        AgentStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime: self.start_time.elapsed().as_secs(),
            tasks_executed: self.tasks_executed.load(Ordering::SeqCst),
            tasks_running: self.tasks_running.load(Ordering::SeqCst),
            host_info: collect_host_info(),
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }
}

/// Agent client for connecting to remote agents
pub struct AgentClient {
    /// Host address
    host: String,
    /// Connection address (socket path or TCP)
    address: String,
    /// Authentication token
    auth_token: Option<String>,
    /// Request timeout
    timeout: Duration,
}

impl AgentClient {
    /// Create a new agent client
    pub fn new(host: &str, address: &str) -> Self {
        Self {
            host: host.to_string(),
            address: address.to_string(),
            auth_token: None,
            timeout: Duration::from_secs(30),
        }
    }

    /// Set authentication token
    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if agent is running
    pub async fn ping(&self) -> AgentResult<bool> {
        // TODO: Implement actual socket communication
        // For now, this is a placeholder
        Err(AgentError::AgentNotRunning(self.host.clone()))
    }

    /// Execute a command on the remote agent
    pub async fn execute(&self, params: ExecuteParams) -> AgentResult<ExecuteResult> {
        let request = AgentRequest {
            id: uuid::Uuid::new_v4().to_string(),
            method: AgentMethod::Execute,
            params: Some(
                serde_json::to_value(&params)
                    .map_err(|e| AgentError::Serialization(e.to_string()))?,
            ),
        };

        self.send_request(request).await
    }

    /// Send a request and wait for response
    async fn send_request(&self, _request: AgentRequest) -> AgentResult<ExecuteResult> {
        // TODO: Implement actual socket communication
        // This is a placeholder for the full implementation
        Err(AgentError::AgentNotRunning(self.host.clone()))
    }

    /// Get agent status
    pub async fn status(&self) -> AgentResult<AgentStatus> {
        // TODO: Implement actual socket communication
        Err(AgentError::AgentNotRunning(self.host.clone()))
    }

    /// Shutdown the remote agent
    pub async fn shutdown(&self) -> AgentResult<()> {
        // TODO: Implement actual socket communication
        Err(AgentError::AgentNotRunning(self.host.clone()))
    }
}

/// Get the current target triple
pub fn current_target() -> String {
    format!(
        "{}-{}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        "gnu" // Simplified; could be more precise
    )
}

/// Collect host information
fn collect_host_info() -> HostInfo {
    HostInfo {
        hostname: hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpus: num_cpus::get(),
        memory_total: 0, // Would need sys-info crate for this
        memory_available: 0,
    }
}

/// Compute SHA256 checksum of a file
pub fn compute_checksum(path: &std::path::Path) -> AgentResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify checksum of agent binary
pub fn verify_checksum(path: &std::path::Path, expected: &str) -> AgentResult<()> {
    let actual = compute_checksum(path)?;
    if actual != expected {
        return Err(AgentError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.max_concurrent, 10);
        assert!(!config.tls);
    }

    #[test]
    fn test_execute_params_serialization() {
        let params = ExecuteParams {
            command: "echo hello".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("echo hello"));
    }

    #[test]
    fn test_agent_method_serialization() {
        let method = AgentMethod::Execute;
        let json = serde_json::to_string(&method).unwrap();
        assert_eq!(json, "\"execute\"");
    }

    #[test]
    fn test_current_target() {
        let target = current_target();
        assert!(!target.is_empty());
        assert!(target.contains('-'));
    }

    #[test]
    fn test_agent_builder() {
        let builder = AgentBuilder::new()
            .target("x86_64-unknown-linux-gnu")
            .release(true)
            .strip(true);

        assert_eq!(builder.config().target, "x86_64-unknown-linux-gnu");
        assert!(builder.config().release);
    }

    #[tokio::test]
    async fn test_agent_runtime_status() {
        let runtime = AgentRuntime::new(AgentConfig::default());
        let status = runtime.status();
        assert_eq!(status.tasks_executed, 0);
        assert_eq!(status.tasks_running, 0);
    }
}
