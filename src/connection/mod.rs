//! Connection layer for remote host communication.
//!
//! This module provides a unified interface for executing commands and transferring
//! files across different transport mechanisms (SSH, local, Docker).
//!
//! # Overview
//!
//! The connection layer abstracts the transport mechanism so that modules and tasks
//! don't need to know whether they're running locally, over SSH, or in a container.
//! All connections implement the [`Connection`] trait.
//!
//! # Supported Transports
//!
//! - **SSH** (via `russh` or `ssh2`): Secure remote execution and file transfer
//!   - Pure Rust implementation (`russh` feature, default)
//!   - libssh2 bindings (`ssh2-backend` feature)
//! - **Local**: Direct execution on the control node
//! - **Docker**: Container-based execution via `docker exec`
//!
//! # Connection Management
//!
//! Connections are managed through the [`ConnectionFactory`] which provides:
//! - Connection pooling and reuse
//! - Automatic transport selection based on host configuration
//! - Credential management
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::connection::{ConnectionBuilder, ExecuteOptions};
//!
//! // Create a connection to a remote host
//! let conn = ConnectionBuilder::new("192.168.1.100")
//!     .user("admin")
//!     .private_key("~/.ssh/id_rsa")
//!     .connect()
//!     .await?;
//!
//! // Execute a command
//! let result = conn.execute("uname -a", None).await?;
//! println!("Output: {}", result.stdout);
//!
//! // Execute with options
//! let opts = ExecuteOptions::new()
//!     .with_cwd("/opt/app")
//!     .with_escalation(Some("root".into()));
//! let result = conn.execute("systemctl restart myservice", Some(opts)).await?;
//! ```

/// Connection configuration types.
pub mod config;

/// Docker container connection implementation.
pub mod docker;

/// Local execution connection implementation.
pub mod local;

/// Shared SSH connection utilities.
pub(crate) mod ssh_common;

/// Pure Rust SSH implementation using russh.
#[cfg(feature = "russh")]
pub mod russh;

// russh_auth: Advanced authentication module (currently disabled)
// The russh_auth module was designed for advanced authentication scenarios but
// needs updating for russh 0.45 API changes (Signer trait, AuthResult enum).
// Core authentication (agent, key, password) is implemented directly in russh.rs.
// If advanced features are needed, this module can be updated later.
// #[cfg(feature = "russh")]
// pub mod russh_auth;

/// Connection pooling for russh connections.
#[cfg(feature = "russh")]
pub mod russh_pool;

/// SSH implementation using libssh2 bindings.
#[cfg(feature = "ssh2-backend")]
pub mod ssh;

/// Circuit breaker pattern for connection resilience.
pub mod circuit_breaker;

/// SSH pipelining for reduced round-trip latency.
pub mod pipelining;

/// Connection health monitoring and diagnostics.
pub mod health;

/// Jump host (bastion) support for SSH connections.
pub mod jump_host;

/// Robust retry logic with exponential backoff.
pub mod retry;

/// SSH Agent forwarding support.
#[cfg(feature = "russh")]
pub mod ssh_agent;

/// Network security module (host key pinning, TLS validation, audit logging).
pub mod security;

/// Windows Remote Management (WinRM) connection support.
#[cfg(feature = "winrm")]
pub mod winrm;

/// Kubernetes pod connection support.
#[cfg(feature = "kubernetes")]
pub mod kubernetes;

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

// Re-export config types at module level for convenience
pub use crate::config::SshConfig;
pub use config::{ConnectionConfig, HostConfig};
#[cfg(feature = "ssh2-backend")]
pub use ssh::{SshConnection, SshConnectionBuilder};

// Re-export russh types when the feature is enabled
#[cfg(feature = "russh")]
pub use russh::{
    ConnectionGroup, ConnectionMetrics, HighPerformanceConnectionFactory, PendingCommand,
    PipelinedExecutor, RusshConnection, RusshConnectionBuilder,
};
// TODO: russh_auth needs updating for russh 0.45 API changes
// #[cfg(feature = "russh")]
// pub use russh_auth::{
//     AuthConfig, AuthMethod, AuthResult, RusshAuthenticator, RusshClientHandler,
//     KeyLoader, KeyType, KeyError, KeyInfo,
//     connect_to_agent, load_private_key, load_private_key_from_string,
//     default_identity_files, standard_key_locations, is_key_encrypted,
// };
#[cfg(feature = "russh")]
pub use russh_pool::{
    HealthCheckResult, HostUtilization, PoolConfig, PoolStats as RusshPoolStats,
    PoolUtilizationMetrics, PooledConnectionHandle, PrewarmResult, RusshConnectionPool,
    RusshConnectionPoolBuilder, WarmupResult,
};

// Re-export circuit breaker types
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerOpenError, CircuitBreakerRegistry,
    CircuitBreakerStats, CircuitState,
};

// Re-export health monitoring types
pub use health::{
    DegradationConfig, DegradationResult, DegradationStrategy, HealthChecker, HealthConfig,
    HealthMonitor, HealthStats, HealthStatus,
};

// Re-export jump host types
pub use jump_host::{JumpHostChain, JumpHostConfig, JumpHostResolver, MAX_JUMP_DEPTH};

// Re-export retry types
pub use retry::{BackoffStrategy, RetryPolicy, RetryResult, RetryStats};

// Re-export SSH agent types (feature-gated)
#[cfg(feature = "russh")]
pub use ssh_agent::{
    AgentClientMetrics, AgentConnectionPool, AgentError, AgentForwarder, AgentForwardingConfig,
    AgentKeyInfo, ForwardingMetrics, PoolStats as AgentPoolStats, SshAgentClient,
};

// Re-export security types
pub use security::{
    AuditEvent, AuditEventType, AuditLevel, EncryptionAuditLog, HostKeyPolicy,
    HostKeyVerificationMode, HostKeyVerificationResult, NetworkIsolation, NetworkSecurityConfig,
    PinnedHostKey, SecurityError, SecurityResult, TlsValidationConfig, TlsVersion,
};

// Re-export pipelining types
pub use pipelining::{
    HostPipeline, PipelineManager, PipelinedCommand, PipelinedResult, PipeliningConfig,
    PipeliningStats,
};

/// Russh-related error type - wraps russh::Error for compatibility with the Handler trait
#[cfg(feature = "russh")]
#[derive(Debug)]
pub struct RusshError(pub ::russh::Error);

#[cfg(feature = "russh")]
impl From<::russh::Error> for RusshError {
    fn from(err: ::russh::Error) -> Self {
        RusshError(err)
    }
}

#[cfg(feature = "russh")]
impl std::fmt::Display for RusshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Russh error: {}", self.0)
    }
}

#[cfg(feature = "russh")]
impl std::error::Error for RusshError {}

#[cfg(feature = "russh")]
impl From<::russh::Error> for ConnectionError {
    fn from(err: ::russh::Error) -> Self {
        ConnectionError::SshError(format!("Russh error: {}", err))
    }
}

#[cfg(feature = "russh")]
impl From<russh_sftp::client::error::Error> for ConnectionError {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        ConnectionError::TransferFailed(format!("SFTP error: {}", e))
    }
}

/// Errors that can occur during connection operations.
///
/// This enum covers all error conditions that may arise when establishing
/// connections, executing commands, or transferring files.
#[derive(Error, Debug)]
pub enum ConnectionError {
    /// Failed to establish initial connection to the host.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Authentication was rejected by the remote host.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Command execution failed (not to be confused with non-zero exit code).
    #[error("Command execution failed: {0}")]
    ExecutionFailed(String),

    /// File upload or download operation failed.
    #[error("File transfer failed: {0}")]
    TransferFailed(String),

    /// Connection or operation timed out.
    #[error("Connection timeout after {0} seconds")]
    Timeout(u64),

    /// The specified host could not be resolved.
    #[error("Host not found: {0}")]
    HostNotFound(String),

    /// Configuration is invalid or incomplete.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// SSH-specific error from the underlying implementation.
    #[error("SSH error: {0}")]
    SshError(String),

    /// I/O error during connection operations.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// No connections available in the pool.
    #[error("Connection pool exhausted")]
    PoolExhausted,

    /// Connection was closed unexpectedly.
    #[error("Connection closed")]
    ConnectionClosed,

    /// Docker-specific error during container operations.
    #[error("Docker error: {0}")]
    DockerError(String),

    /// Kubernetes-specific error during pod operations.
    #[error("Kubernetes error: {0}")]
    KubernetesError(String),

    /// The requested operation is not supported by this transport.
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
}

/// Result type for connection operations.
///
/// A type alias for `Result<T, ConnectionError>`.
pub type ConnectionResult<T> = Result<T, ConnectionError>;

/// The result of executing a command on a connection.
///
/// Contains the exit code, stdout, stderr, and a convenience boolean
/// indicating whether the command succeeded (exit code 0).
///
/// # Example
///
/// ```rust
/// use rustible::connection::CommandResult;
///
/// let result = CommandResult::success("Hello".into(), String::new());
/// assert!(result.success);
/// assert_eq!(result.exit_code, 0);
///
/// let failed = CommandResult::failure(1, String::new(), "error".into());
/// assert!(!failed.success);
/// ```
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Exit code of the command (0 typically indicates success).
    pub exit_code: i32,
    /// Content written to standard output.
    pub stdout: String,
    /// Content written to standard error.
    pub stderr: String,
    /// Convenience flag: `true` if `exit_code == 0`.
    pub success: bool,
}

impl CommandResult {
    /// Create a new successful command result
    pub fn success(stdout: String, stderr: String) -> Self {
        Self {
            exit_code: 0,
            stdout,
            stderr,
            success: true,
        }
    }

    /// Create a new failed command result
    pub fn failure(exit_code: i32, stdout: String, stderr: String) -> Self {
        Self {
            exit_code,
            stdout,
            stderr,
            success: false,
        }
    }

    /// Get the combined output (stdout + stderr)
    pub fn combined_output(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Options for command execution
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
    /// Method for privilege escalation (sudo, su, etc.)
    pub escalate_method: Option<String>,
    /// Password for privilege escalation operations
    pub escalate_password: Option<String>,
}

impl ExecuteOptions {
    /// Create new execute options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the working directory
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Add an environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Enable privilege escalation
    pub fn with_escalation(mut self, user: Option<String>) -> Self {
        self.escalate = true;
        self.escalate_user = user;
        self
    }

    /// Set the privilege escalation method
    pub fn with_escalate_method(mut self, method: impl Into<String>) -> Self {
        self.escalate_method = Some(method.into());
        self
    }

    /// Set the privilege escalation password
    pub fn with_escalate_password(mut self, password: impl Into<String>) -> Self {
        self.escalate_password = Some(password.into());
        self
    }
}

/// Options for file transfer
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
    /// Create new transfer options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set file mode
    pub fn with_mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set owner
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    /// Set group
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Enable directory creation
    pub fn with_create_dirs(mut self) -> Self {
        self.create_dirs = true;
        self
    }
}

/// The main connection trait that all transport implementations must implement
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
    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()>;

    /// Download a file content from the remote host
    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>>;

    /// Check if a path exists on the remote host
    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool>;

    /// Check if a path is a directory on the remote host
    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool>;

    /// Get file stats (size, mode, owner, etc.)
    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat>;

    /// Close the connection
    async fn close(&self) -> ConnectionResult<()>;

    /// Execute multiple commands in batch (default: sequential)
    ///
    /// This method executes multiple commands and returns results in the same
    /// order as input commands. The default implementation runs commands
    /// sequentially. Transport implementations (like Russh) may override this
    /// to provide parallel execution using channel multiplexing.
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

/// File statistics
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

/// Connection type enum for factory pattern
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConnectionType {
    /// Local connection (no network)
    Local,
    /// SSH connection to remote host
    Ssh {
        host: String,
        port: u16,
        user: String,
    },
    /// Docker container connection
    Docker { container: String },
    /// Kubernetes pod connection
    Kubernetes {
        namespace: String,
        pod: String,
        container: Option<String>,
    },
}

impl ConnectionType {
    /// Get a unique key for this connection type (for pooling)
    pub fn pool_key(&self) -> String {
        match self {
            ConnectionType::Local => "local".to_string(),
            ConnectionType::Ssh { host, port, user } => format!("ssh://{}@{}:{}", user, host, port),
            ConnectionType::Docker { container } => format!("docker://{}", container),
            ConnectionType::Kubernetes {
                namespace,
                pod,
                container,
            } => {
                if let Some(c) = container {
                    format!("k8s://{}/{}:{}", namespace, pod, c)
                } else {
                    format!("k8s://{}/{}", namespace, pod)
                }
            }
        }
    }
}

/// Factory for creating connections
#[derive(Clone)]
pub struct ConnectionFactory {
    /// Global configuration
    config: Arc<ConnectionConfig>,
    /// Connection pool
    pool: Arc<RwLock<ConnectionPool>>,
}

impl ConnectionFactory {
    /// Create a new connection factory
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config: Arc::new(config),
            pool: Arc::new(RwLock::new(ConnectionPool::new(10))), // Default pool size of 10
        }
    }

    /// Create a new connection factory with custom pool size
    pub fn with_pool_size(config: ConnectionConfig, pool_size: usize) -> Self {
        Self {
            config: Arc::new(config),
            pool: Arc::new(RwLock::new(ConnectionPool::new(pool_size))),
        }
    }

    /// Get a connection for a host
    pub async fn get_connection(
        &self,
        host: &str,
    ) -> ConnectionResult<Arc<dyn Connection + Send + Sync>> {
        let conn_type = self.resolve_connection_type(host)?;
        let pool_key = conn_type.pool_key();

        // Try to get from pool first - release lock before await
        let pooled_conn = { self.pool.write().get(&pool_key) };

        if let Some(conn) = pooled_conn {
            if conn.is_alive().await {
                return Ok(conn);
            }
        }

        // Create new connection
        let conn = self.create_connection(&conn_type).await?;

        // Add to pool
        self.pool.write().put(pool_key, conn.clone());

        Ok(conn)
    }

    /// Resolve a host name to a connection type
    fn resolve_connection_type(&self, host: &str) -> ConnectionResult<ConnectionType> {
        // Check for special connection types
        if host == "localhost" || host == "127.0.0.1" || host == "local" {
            // Check if we should use local connection
            if let Some(host_config) = self.config.get_host(host) {
                if host_config.connection == Some("local".to_string()) {
                    return Ok(ConnectionType::Local);
                }
            }
            // Default to local for localhost
            return Ok(ConnectionType::Local);
        }

        // Check for docker connection
        if host.starts_with("docker://") {
            let container = host.strip_prefix("docker://").unwrap().to_string();
            return Ok(ConnectionType::Docker { container });
        }

        // Default to SSH
        let host_config = self.config.get_host(host);
        let (actual_host, port, user) = if let Some(hc) = host_config {
            (
                hc.hostname.clone().unwrap_or_else(|| host.to_string()),
                hc.port.unwrap_or(22),
                hc.user
                    .clone()
                    .unwrap_or_else(|| self.config.defaults.user.clone()),
            )
        } else {
            (host.to_string(), 22, self.config.defaults.user.clone())
        };

        Ok(ConnectionType::Ssh {
            host: actual_host,
            port,
            user,
        })
    }

    /// Create a new connection based on type
    async fn create_connection(
        &self,
        conn_type: &ConnectionType,
    ) -> ConnectionResult<Arc<dyn Connection + Send + Sync>> {
        match conn_type {
            ConnectionType::Local => {
                let conn = local::LocalConnection::new();
                Ok(Arc::new(conn))
            }
            ConnectionType::Ssh { host, port, user } => {
                let host_config = self.config.get_host(host).cloned();
                // Prefer russh (pure Rust) when available, fall back to ssh2
                #[cfg(feature = "russh")]
                {
                    let conn = russh::RusshConnection::connect(
                        host,
                        *port,
                        user,
                        host_config,
                        &self.config,
                    )
                    .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(all(feature = "ssh2-backend", not(feature = "russh")))]
                {
                    let conn =
                        ssh::SshConnection::connect(host, *port, user, host_config, &self.config)
                            .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(not(any(feature = "russh", feature = "ssh2-backend")))]
                {
                    Err(ConnectionError::InvalidConfig(
                        "No SSH backend available. Enable 'russh' or 'ssh2-backend' feature."
                            .to_string(),
                    ))
                }
            }
            ConnectionType::Docker { container } => {
                let conn = docker::DockerConnection::new(container.clone());
                Ok(Arc::new(conn))
            }
            ConnectionType::Kubernetes {
                namespace,
                pod,
                container,
            } => {
                // Kubernetes connection requires the kubernetes feature
                #[cfg(feature = "kubernetes")]
                {
                    let conn = kubernetes::KubernetesConnection::new(
                        namespace.clone(),
                        pod.clone(),
                        container.clone(),
                        kubernetes::KubernetesAuth::default(),
                    )
                    .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(not(feature = "kubernetes"))]
                {
                    let _ = (namespace, pod, container);
                    Err(ConnectionError::InvalidConfig(
                        "Kubernetes support not available. Enable 'kubernetes' feature."
                            .to_string(),
                    ))
                }
            }
        }
    }

    /// Close all connections in the pool
    pub async fn close_all(&self) -> ConnectionResult<()> {
        let connections: Vec<_> = {
            let mut pool = self.pool.write();
            pool.drain()
        };

        for conn in connections {
            let _ = conn.close().await;
        }

        Ok(())
    }

    /// Get pool statistics
    pub fn pool_stats(&self) -> PoolStats {
        self.pool.read().stats()
    }
}

/// Configuration for connection pool limits
#[derive(Debug, Clone)]
pub struct ConnectionPoolConfig {
    /// Maximum connections per host (default: 5)
    pub max_connections_per_host: usize,

    /// Maximum total connections across all hosts (default: 100)
    pub max_total_connections: usize,

    /// Idle connection timeout in seconds (default: 300 = 5 minutes)
    /// Connections unused for longer than this will be closed
    pub idle_timeout_secs: u64,

    /// Maximum connection lifetime in seconds (default: 3600 = 1 hour)
    /// Connections older than this will be closed regardless of activity
    pub max_lifetime_secs: u64,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_host: 5,
            max_total_connections: 100,
            idle_timeout_secs: 300,  // 5 minutes
            max_lifetime_secs: 3600, // 1 hour
        }
    }
}

/// A pooled connection with lifecycle tracking
struct PooledConnection {
    connection: Arc<dyn Connection + Send + Sync>,
    created_at: std::time::Instant,
    last_used: std::time::Instant,
    host_key: String,
}

impl PooledConnection {
    fn new(connection: Arc<dyn Connection + Send + Sync>, host_key: String) -> Self {
        let now = std::time::Instant::now();
        Self {
            connection,
            created_at: now,
            last_used: now,
            host_key,
        }
    }

    fn touch(&mut self) {
        self.last_used = std::time::Instant::now();
    }

    fn is_expired(&self, config: &ConnectionPoolConfig) -> bool {
        let now = std::time::Instant::now();

        // Check idle timeout
        if now.duration_since(self.last_used).as_secs() > config.idle_timeout_secs {
            return true;
        }

        // Check max lifetime
        if now.duration_since(self.created_at).as_secs() > config.max_lifetime_secs {
            return true;
        }

        false
    }
}

/// Connection pool for reusing connections with resource limits
///
/// Provides:
/// - Per-host connection limits to prevent overloading single targets
/// - Total connection limits to prevent resource exhaustion
/// - Idle timeout to release unused connections
/// - Max lifetime to ensure connection rotation
pub struct ConnectionPool {
    /// Pool configuration
    config: ConnectionPoolConfig,
    /// Active connections by pool key
    connections: HashMap<String, PooledConnection>,
    /// Per-host connection counts for enforcing limits
    host_counts: HashMap<String, usize>,
}

impl ConnectionPool {
    /// Create a new connection pool with default configuration
    pub fn new(max_connections: usize) -> Self {
        Self::with_config(ConnectionPoolConfig {
            max_total_connections: max_connections,
            ..Default::default()
        })
    }

    /// Create a new connection pool with custom configuration
    pub fn with_config(config: ConnectionPoolConfig) -> Self {
        Self {
            config,
            connections: HashMap::new(),
            host_counts: HashMap::new(),
        }
    }

    /// Extract the host key from a pool key for per-host tracking
    /// e.g., "ssh://user@host:22/path" -> "host:22"
    fn extract_host_key(pool_key: &str) -> String {
        // Simple extraction: try to get host:port from the key
        if let Some(stripped) = pool_key.strip_prefix("ssh://") {
            // Format: user@host:port or host:port
            let without_user = if let Some(at_pos) = stripped.find('@') {
                &stripped[at_pos + 1..]
            } else {
                stripped
            };
            // Take everything before any path
            if let Some(slash_pos) = without_user.find('/') {
                without_user[..slash_pos].to_string()
            } else {
                without_user.to_string()
            }
        } else if pool_key.starts_with("docker://") || pool_key.starts_with("local") {
            // For docker and local, use the full key as the host key
            pool_key.to_string()
        } else {
            // Fallback: use the whole key
            pool_key.to_string()
        }
    }

    /// Clean up expired connections
    fn cleanup_expired(&mut self) {
        let expired_keys: Vec<String> = self
            .connections
            .iter()
            .filter(|(_, pooled)| pooled.is_expired(&self.config))
            .map(|(key, _)| key.clone())
            .collect();

        for key in expired_keys {
            self.remove(&key);
            tracing::trace!("Expired connection removed: {}", key);
        }
    }

    /// Get a connection from the pool
    pub fn get(&mut self, key: &str) -> Option<Arc<dyn Connection + Send + Sync>> {
        // Clean up expired connections first
        self.cleanup_expired();

        // Get and touch the connection
        if let Some(pooled) = self.connections.get_mut(key) {
            pooled.touch();
            Some(pooled.connection.clone())
        } else {
            None
        }
    }

    /// Put a connection into the pool
    ///
    /// Returns `true` if the connection was added, `false` if pool limits prevented it
    pub fn put(&mut self, key: String, conn: Arc<dyn Connection + Send + Sync>) -> bool {
        // Clean up expired connections first
        self.cleanup_expired();

        let host_key = Self::extract_host_key(&key);

        // Check per-host limit
        let host_count = self.host_counts.get(&host_key).copied().unwrap_or(0);
        if host_count >= self.config.max_connections_per_host {
            tracing::warn!(
                "Connection pool per-host limit reached for {}: {}/{}",
                host_key,
                host_count,
                self.config.max_connections_per_host
            );
            return false;
        }

        // Check total limit
        if self.connections.len() >= self.config.max_total_connections {
            // Try to evict the oldest idle connection
            let oldest_key = self
                .connections
                .iter()
                .min_by_key(|(_, pooled)| pooled.last_used)
                .map(|(key, _)| key.clone());

            if let Some(key_to_remove) = oldest_key {
                tracing::debug!(
                    "Connection pool total limit reached, evicting oldest: {}",
                    key_to_remove
                );
                self.remove(&key_to_remove);
            } else {
                tracing::warn!(
                    "Connection pool total limit reached: {}/{}",
                    self.connections.len(),
                    self.config.max_total_connections
                );
                return false;
            }
        }

        // Add the connection
        let pooled = PooledConnection::new(conn, host_key.clone());
        self.connections.insert(key, pooled);

        // Update host count
        *self.host_counts.entry(host_key).or_insert(0) += 1;

        true
    }

    /// Remove a connection from the pool
    pub fn remove(&mut self, key: &str) -> Option<Arc<dyn Connection + Send + Sync>> {
        if let Some(pooled) = self.connections.remove(key) {
            // Update host count
            if let Some(count) = self.host_counts.get_mut(&pooled.host_key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.host_counts.remove(&pooled.host_key);
                }
            }
            Some(pooled.connection)
        } else {
            None
        }
    }

    /// Drain all connections from the pool
    pub fn drain(&mut self) -> Vec<Arc<dyn Connection + Send + Sync>> {
        self.host_counts.clear();
        self.connections
            .drain()
            .map(|(_, pooled)| pooled.connection)
            .collect()
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            active_connections: self.connections.len(),
            max_connections: self.config.max_total_connections,
            max_per_host: self.config.max_connections_per_host,
            host_count: self.host_counts.len(),
            idle_timeout_secs: self.config.idle_timeout_secs,
            max_lifetime_secs: self.config.max_lifetime_secs,
        }
    }

    /// Get detailed per-host statistics
    pub fn host_stats(&self) -> HashMap<String, usize> {
        self.host_counts.clone()
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Number of active connections
    pub active_connections: usize,
    /// Maximum total connections allowed
    pub max_connections: usize,
    /// Maximum connections per host
    pub max_per_host: usize,
    /// Number of unique hosts with connections
    pub host_count: usize,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Maximum connection lifetime in seconds
    pub max_lifetime_secs: u64,
}

/// Builder for creating connections with custom options
pub struct ConnectionBuilder {
    host: String,
    port: Option<u16>,
    user: Option<String>,
    password: Option<String>,
    private_key: Option<String>,
    timeout: Option<u64>,
    connection_type: Option<String>,
}

impl ConnectionBuilder {
    /// Create a new connection builder
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: None,
            user: None,
            password: None,
            private_key: None,
            timeout: None,
            connection_type: None,
        }
    }

    /// Set the port
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set the user
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set the password
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the private key path
    pub fn private_key(mut self, key_path: impl Into<String>) -> Self {
        self.private_key = Some(key_path.into());
        self
    }

    /// Set the connection timeout
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the connection type explicitly
    pub fn connection_type(mut self, conn_type: impl Into<String>) -> Self {
        self.connection_type = Some(conn_type.into());
        self
    }

    /// Build and connect
    pub async fn connect(self) -> ConnectionResult<Arc<dyn Connection + Send + Sync>> {
        // Determine connection type
        let conn_type = if let Some(ct) = &self.connection_type {
            match ct.as_str() {
                "local" => ConnectionType::Local,
                "docker" => ConnectionType::Docker {
                    container: self.host.clone(),
                },
                "ssh" | _ => ConnectionType::Ssh {
                    host: self.host.clone(),
                    port: self.port.unwrap_or(22),
                    user: self.user.clone().unwrap_or_else(whoami),
                },
            }
        } else if self.host == "localhost" || self.host == "127.0.0.1" || self.host == "local" {
            ConnectionType::Local
        } else if self.host.starts_with("docker://") {
            ConnectionType::Docker {
                container: self.host.strip_prefix("docker://").unwrap().to_string(),
            }
        } else {
            ConnectionType::Ssh {
                host: self.host.clone(),
                port: self.port.unwrap_or(22),
                user: self.user.clone().unwrap_or_else(whoami),
            }
        };

        // Create connection based on type
        match conn_type {
            ConnectionType::Local => Ok(Arc::new(local::LocalConnection::new())),
            ConnectionType::Ssh { host, port, user } => {
                // Build host config from builder options
                let host_config = HostConfig {
                    hostname: Some(host.clone()),
                    port: Some(port),
                    user: Some(user.clone()),
                    identity_file: self.private_key.clone(),
                    password: self.password.clone(),
                    connect_timeout: self.timeout,
                    ..Default::default()
                };

                let config = ConnectionConfig::default();
                // Prefer russh (pure Rust) when available, fall back to ssh2
                #[cfg(feature = "russh")]
                {
                    let conn = russh::RusshConnection::connect(
                        &host,
                        port,
                        &user,
                        Some(host_config),
                        &config,
                    )
                    .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(all(feature = "ssh2-backend", not(feature = "russh")))]
                {
                    let conn =
                        ssh::SshConnection::connect(&host, port, &user, Some(host_config), &config)
                            .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(not(any(feature = "russh", feature = "ssh2-backend")))]
                {
                    let _ = (host, port, user, host_config, config); // silence unused warnings
                    Err(ConnectionError::InvalidConfig(
                        "No SSH backend available. Enable 'russh' or 'ssh2-backend' feature."
                            .to_string(),
                    ))
                }
            }
            ConnectionType::Docker { container } => {
                Ok(Arc::new(docker::DockerConnection::new(container)))
            }
            ConnectionType::Kubernetes {
                namespace,
                pod,
                container,
            } => {
                // Kubernetes connection requires the kubernetes feature
                #[cfg(feature = "kubernetes")]
                {
                    let conn = kubernetes::KubernetesConnection::new(
                        namespace,
                        pod,
                        container,
                        kubernetes::KubernetesAuth::default(),
                    )
                    .await?;
                    Ok(Arc::new(conn))
                }
                #[cfg(not(feature = "kubernetes"))]
                {
                    let _ = (namespace, pod, container);
                    Err(ConnectionError::InvalidConfig(
                        "Kubernetes support not available. Enable 'kubernetes' feature."
                            .to_string(),
                    ))
                }
            }
        }
    }
}

/// Get the current username
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_result_success() {
        let result = CommandResult::success("output".to_string(), "".to_string());
        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "output");
    }

    #[test]
    fn test_command_result_failure() {
        let result = CommandResult::failure(1, "".to_string(), "error".to_string());
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "error");
    }

    #[test]
    fn test_connection_type_pool_key() {
        assert_eq!(ConnectionType::Local.pool_key(), "local");
        assert_eq!(
            ConnectionType::Ssh {
                host: "example.com".to_string(),
                port: 22,
                user: "user".to_string()
            }
            .pool_key(),
            "ssh://user@example.com:22"
        );
        assert_eq!(
            ConnectionType::Docker {
                container: "mycontainer".to_string()
            }
            .pool_key(),
            "docker://mycontainer"
        );
    }

    #[test]
    fn test_execute_options_builder() {
        let options = ExecuteOptions::new()
            .with_cwd("/tmp")
            .with_env("FOO", "bar")
            .with_timeout(30)
            .with_escalation(Some("root".to_string()));

        assert_eq!(options.cwd, Some("/tmp".to_string()));
        assert_eq!(options.env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(options.timeout, Some(30));
        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    #[test]
    fn test_connection_pool_config_default() {
        let config = ConnectionPoolConfig::default();
        assert_eq!(config.max_connections_per_host, 5);
        assert_eq!(config.max_total_connections, 100);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.max_lifetime_secs, 3600);
    }

    #[test]
    fn test_connection_pool_extract_host_key() {
        assert_eq!(
            ConnectionPool::extract_host_key("ssh://user@example.com:22"),
            "example.com:22"
        );
        assert_eq!(
            ConnectionPool::extract_host_key("ssh://example.com:22"),
            "example.com:22"
        );
        assert_eq!(
            ConnectionPool::extract_host_key("ssh://user@host:2222/path"),
            "host:2222"
        );
        assert_eq!(
            ConnectionPool::extract_host_key("docker://mycontainer"),
            "docker://mycontainer"
        );
        assert_eq!(ConnectionPool::extract_host_key("local"), "local");
    }

    #[test]
    fn test_connection_pool_stats() {
        let config = ConnectionPoolConfig {
            max_connections_per_host: 3,
            max_total_connections: 10,
            idle_timeout_secs: 60,
            max_lifetime_secs: 300,
        };
        let pool = ConnectionPool::with_config(config);
        let stats = pool.stats();

        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.max_connections, 10);
        assert_eq!(stats.max_per_host, 3);
        assert_eq!(stats.host_count, 0);
        assert_eq!(stats.idle_timeout_secs, 60);
        assert_eq!(stats.max_lifetime_secs, 300);
    }
}
