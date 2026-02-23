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

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

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
    /// Query agent status
    Status,
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

#[derive(Debug, Clone)]
enum AgentTransportAddress {
    Tcp(String),
    #[cfg(unix)]
    Unix(PathBuf),
}

impl AgentTransportAddress {
    fn parse(address: &str) -> AgentResult<Self> {
        let address = address.trim();
        if address.is_empty() {
            return Err(AgentError::ConnectionFailed(
                "Agent address cannot be empty".to_string(),
            ));
        }

        if let Some(addr) = address.strip_prefix("tcp://") {
            return Ok(Self::Tcp(addr.to_string()));
        }

        if let Some(path) = address.strip_prefix("unix://") {
            #[cfg(unix)]
            {
                return Ok(Self::Unix(PathBuf::from(path)));
            }

            #[cfg(not(unix))]
            {
                return Err(AgentError::ConnectionFailed(
                    "Unix sockets are not supported on this platform".to_string(),
                ));
            }
        }

        #[cfg(unix)]
        {
            if address.starts_with('/')
                || address.starts_with("./")
                || address.starts_with("../")
                || address.ends_with(".sock")
            {
                return Ok(Self::Unix(PathBuf::from(address)));
            }
        }

        Ok(Self::Tcp(address.to_string()))
    }
}

fn rpc_error(code: i32, message: impl Into<String>) -> AgentRpcError {
    AgentRpcError {
        code,
        message: message.into(),
        data: None,
    }
}

/// Agent runtime for executing on target hosts
pub struct AgentRuntime {
    config: AgentConfig,
    start_time: Instant,
    tasks_executed: std::sync::atomic::AtomicU64,
    tasks_running: std::sync::atomic::AtomicUsize,
    shutdown_requested: AtomicBool,
}

impl AgentRuntime {
    /// Create a new agent runtime with configuration
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            start_time: Instant::now(),
            tasks_executed: std::sync::atomic::AtomicU64::new(0),
            tasks_running: std::sync::atomic::AtomicUsize::new(0),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    /// Start the agent runtime
    pub async fn start(&self) -> AgentResult<()> {
        use std::sync::atomic::Ordering;

        // Ensure work directory exists
        std::fs::create_dir_all(&self.config.work_dir)?;
        self.shutdown_requested.store(false, Ordering::SeqCst);

        match AgentTransportAddress::parse(&self.config.listen)? {
            AgentTransportAddress::Tcp(address) => self.serve_tcp(&address).await,
            #[cfg(unix)]
            AgentTransportAddress::Unix(path) => self.serve_unix(&path).await,
        }
    }

    async fn serve_tcp(&self, address: &str) -> AgentResult<()> {
        use std::sync::atomic::Ordering;

        let listener = TcpListener::bind(address).await.map_err(|e| {
            AgentError::ConnectionFailed(format!("Failed to bind TCP listener {}: {}", address, e))
        })?;

        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                break;
            }

            let (stream, _) = listener.accept().await.map_err(|e| {
                AgentError::ConnectionFailed(format!(
                    "Failed to accept TCP connection on {}: {}",
                    address, e
                ))
            })?;

            self.handle_connection(stream).await?;
        }

        Ok(())
    }

    #[cfg(unix)]
    async fn serve_unix(&self, path: &std::path::Path) -> AgentResult<()> {
        use std::sync::atomic::Ordering;
        use tokio::net::UnixListener;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if path.exists() {
            match std::fs::remove_file(path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(AgentError::Io(e)),
            }
        }

        struct SocketCleanup {
            path: PathBuf,
        }

        impl Drop for SocketCleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.path);
            }
        }

        let listener = UnixListener::bind(path).map_err(|e| {
            AgentError::ConnectionFailed(format!(
                "Failed to bind Unix listener {}: {}",
                path.display(),
                e
            ))
        })?;
        let _cleanup = SocketCleanup {
            path: path.to_path_buf(),
        };

        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                break;
            }

            let (stream, _) = listener.accept().await.map_err(|e| {
                AgentError::ConnectionFailed(format!(
                    "Failed to accept Unix connection on {}: {}",
                    path.display(),
                    e
                ))
            })?;

            self.handle_connection(stream).await?;
        }

        Ok(())
    }

    async fn handle_connection<S>(&self, stream: S) -> AgentResult<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
            AgentError::ConnectionFailed(format!("Failed reading agent request: {}", e))
        })?;

        if bytes_read == 0 {
            return Ok(());
        }

        let request = match serde_json::from_str::<AgentRequest>(line.trim_end()) {
            Ok(request) => request,
            Err(err) => {
                let response = AgentResponse {
                    id: "invalid".to_string(),
                    result: None,
                    error: Some(rpc_error(-32700, format!("Invalid request JSON: {}", err))),
                };
                return self.write_response(reader.into_inner(), &response).await;
            }
        };

        let response = self.handle_request(request).await;
        self.write_response(reader.into_inner(), &response).await
    }

    async fn write_response<S>(&self, mut stream: S, response: &AgentResponse) -> AgentResult<()>
    where
        S: AsyncWrite + Unpin,
    {
        let payload = serde_json::to_string(response)
            .map_err(|e| AgentError::Serialization(e.to_string()))?;

        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| AgentError::ConnectionFailed(format!("Failed writing response: {}", e)))?;
        stream.write_all(b"\n").await.map_err(|e| {
            AgentError::ConnectionFailed(format!("Failed writing response newline: {}", e))
        })?;
        stream.flush().await.map_err(|e| {
            AgentError::ConnectionFailed(format!("Failed flushing response: {}", e))
        })?;

        Ok(())
    }

    async fn handle_request(&self, request: AgentRequest) -> AgentResponse {
        use serde_json::json;
        use std::sync::atomic::Ordering;

        match request.method {
            AgentMethod::Execute => {
                let params = match request.params.clone() {
                    Some(value) => match serde_json::from_value::<ExecuteParams>(value) {
                        Ok(params) => params,
                        Err(_) => {
                            return AgentResponse {
                                id: request.id,
                                result: None,
                                error: Some(rpc_error(-32602, "Invalid params for execute")),
                            };
                        }
                    },
                    None => {
                        return AgentResponse {
                            id: request.id,
                            result: None,
                            error: Some(rpc_error(-32602, "Missing params for execute")),
                        };
                    }
                };

                match self.execute(params).await {
                    Ok(result) => AgentResponse {
                        id: request.id,
                        result: Some(json!(result)),
                        error: None,
                    },
                    Err(err) => AgentResponse {
                        id: request.id,
                        result: None,
                        error: Some(rpc_error(-32000, err.to_string())),
                    },
                }
            }
            AgentMethod::Ping => AgentResponse {
                id: request.id,
                result: Some(json!({"ok": true})),
                error: None,
            },
            AgentMethod::Status => AgentResponse {
                id: request.id,
                result: Some(json!(self.status())),
                error: None,
            },
            AgentMethod::Shutdown => {
                self.shutdown_requested.store(true, Ordering::SeqCst);
                AgentResponse {
                    id: request.id,
                    result: Some(json!({"ok": true})),
                    error: None,
                }
            }
            _ => AgentResponse {
                id: request.id,
                result: None,
                error: Some(rpc_error(-32601, "Method not supported")),
            },
        }
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
        let request = AgentRequest {
            id: uuid::Uuid::new_v4().to_string(),
            method: AgentMethod::Ping,
            params: None,
        };

        let response = self.send_request(request).await?;
        let payload = response
            .result
            .ok_or_else(|| AgentError::ProtocolError("Missing ping result".to_string()))?;

        match payload {
            serde_json::Value::Bool(ok) => Ok(ok),
            serde_json::Value::Object(map) => map
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| {
                    AgentError::ProtocolError("Invalid ping result payload".to_string())
                }),
            _ => Err(AgentError::ProtocolError(
                "Invalid ping result payload".to_string(),
            )),
        }
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

        let response = self.send_request(request).await?;
        self.decode_result(response)
    }

    /// Send a request and wait for response
    async fn send_request(&self, request: AgentRequest) -> AgentResult<AgentResponse> {
        let payload = format!(
            "{}\n",
            serde_json::to_string(&request)
                .map_err(|e| AgentError::Serialization(e.to_string()))?
        );
        let endpoint = AgentTransportAddress::parse(&self.address)?;
        let request_id = request.id.clone();

        let response_line = tokio::time::timeout(self.timeout, async {
            match endpoint {
                AgentTransportAddress::Tcp(address) => {
                    let stream = tokio::net::TcpStream::connect(&address)
                        .await
                        .map_err(|e| self.map_connect_error(e, &address))?;
                    self.exchange_stream(stream, &payload).await
                }
                #[cfg(unix)]
                AgentTransportAddress::Unix(path) => {
                    let display = path.display().to_string();
                    let stream = tokio::net::UnixStream::connect(&path)
                        .await
                        .map_err(|e| self.map_connect_error(e, &display))?;
                    self.exchange_stream(stream, &payload).await
                }
            }
        })
        .await
        .map_err(|_| {
            AgentError::Timeout(format!(
                "Request {} timed out after {:?}",
                request_id, self.timeout
            ))
        })??;

        let response: AgentResponse = serde_json::from_str(response_line.trim_end())
            .map_err(|e| AgentError::Serialization(e.to_string()))?;

        if response.id != request.id {
            return Err(AgentError::ProtocolError(format!(
                "Mismatched response id: expected {}, got {}",
                request.id, response.id
            )));
        }

        if let Some(err) = response.error.as_ref() {
            return Err(AgentError::ProtocolError(format!(
                "RPC error {}: {}",
                err.code, err.message
            )));
        }

        Ok(response)
    }

    /// Get agent status
    pub async fn status(&self) -> AgentResult<AgentStatus> {
        let request = AgentRequest {
            id: uuid::Uuid::new_v4().to_string(),
            method: AgentMethod::Status,
            params: None,
        };

        let response = self.send_request(request).await?;
        self.decode_result(response)
    }

    /// Shutdown the remote agent
    pub async fn shutdown(&self) -> AgentResult<()> {
        let request = AgentRequest {
            id: uuid::Uuid::new_v4().to_string(),
            method: AgentMethod::Shutdown,
            params: None,
        };

        let _ = self.send_request(request).await?;
        Ok(())
    }

    async fn exchange_stream<S>(&self, mut stream: S, payload: &str) -> AgentResult<String>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| AgentError::ConnectionFailed(format!("Failed to send request: {}", e)))?;
        stream
            .flush()
            .await
            .map_err(|e| AgentError::ConnectionFailed(format!("Failed to flush request: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        let bytes_read = reader.read_line(&mut response_line).await.map_err(|e| {
            AgentError::ConnectionFailed(format!("Failed to read response payload: {}", e))
        })?;

        if bytes_read == 0 {
            return Err(AgentError::ProtocolError(
                "Agent closed connection without response".to_string(),
            ));
        }

        Ok(response_line)
    }

    fn map_connect_error(&self, error: std::io::Error, address: &str) -> AgentError {
        match error.kind() {
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotFound => AgentError::AgentNotRunning(self.host.clone()),
            _ => {
                AgentError::ConnectionFailed(format!("Failed connecting to {}: {}", address, error))
            }
        }
    }

    fn decode_result<T>(&self, response: AgentResponse) -> AgentResult<T>
    where
        T: DeserializeOwned,
    {
        let payload = response
            .result
            .ok_or_else(|| AgentError::ProtocolError("Missing response result".to_string()))?;
        serde_json::from_value(payload).map_err(|e| AgentError::Serialization(e.to_string()))
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
    use std::sync::Arc;
    use tokio::time::{sleep, timeout};

    fn reserve_tcp_address() -> String {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("should bind ephemeral port");
        let address = listener
            .local_addr()
            .expect("listener should expose local address");
        drop(listener);
        address.to_string()
    }

    async fn wait_for_agent(client: &AgentClient) {
        for _ in 0..100 {
            match client.ping().await {
                Ok(true) => return,
                Ok(false) => sleep(Duration::from_millis(20)).await,
                Err(AgentError::AgentNotRunning(_)) | Err(AgentError::ConnectionFailed(_)) => {
                    sleep(Duration::from_millis(20)).await;
                }
                Err(err) => panic!("unexpected error while waiting for agent: {}", err),
            }
        }
        panic!("agent did not become ready in time");
    }

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

    #[tokio::test]
    async fn test_agent_client_not_running_for_unbound_tcp_address() {
        let address = reserve_tcp_address();
        let client =
            AgentClient::new("local-test", &address).with_timeout(Duration::from_millis(250));

        let err = client.ping().await.expect_err("expected ping to fail");
        assert!(matches!(
            err,
            AgentError::AgentNotRunning(ref host) if host == "local-test"
        ));
    }

    #[tokio::test]
    async fn test_agent_runtime_client_rpc_roundtrip_over_tcp() {
        let listen = reserve_tcp_address();
        let work_dir = tempfile::tempdir().expect("tempdir should be created");

        let config = AgentConfig {
            listen: listen.clone(),
            work_dir: work_dir.path().join("work"),
            ..AgentConfig::default()
        };

        let runtime = Arc::new(AgentRuntime::new(config));
        let runtime_task = {
            let runtime = Arc::clone(&runtime);
            tokio::spawn(async move { runtime.start().await })
        };

        let client = AgentClient::new("local-test", &listen).with_timeout(Duration::from_secs(1));
        wait_for_agent(&client).await;

        assert!(client.ping().await.expect("ping should succeed"));

        let status_before = client.status().await.expect("status should succeed");
        assert_eq!(status_before.tasks_executed, 0);

        let result = client
            .execute(ExecuteParams {
                command: "printf rustible-agent-rpc".to_string(),
                cwd: None,
                env: HashMap::new(),
                timeout: None,
                user: None,
                group: None,
                shell: true,
            })
            .await
            .expect("execute should succeed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "rustible-agent-rpc");

        let status_after = client.status().await.expect("status should succeed");
        assert_eq!(status_after.tasks_executed, 1);
        assert_eq!(status_after.tasks_running, 0);

        client.shutdown().await.expect("shutdown should succeed");
        let runtime_result = timeout(Duration::from_secs(2), runtime_task)
            .await
            .expect("runtime should exit promptly")
            .expect("runtime task should join");
        assert!(
            runtime_result.is_ok(),
            "runtime returned error: {:?}",
            runtime_result
        );
    }
}
