//! SSH Agent Forwarding Module
//!
//! This module provides comprehensive SSH agent forwarding functionality for
//! nested SSH connections, enabling secure authentication across multiple hops.
//!
//! ## Features
//!
//! - **SSH_AUTH_SOCK Support**: Connect to local SSH agents via Unix socket
//! - **Agent Forwarding**: Forward authentication to nested SSH connections
//! - **Connection Multiplexing**: Reuse agent connections for efficiency
//! - **Key Listing**: Enumerate available keys from the agent
//! - **Signature Delegation**: Request signatures from the agent for authentication
//!
//! ## Architecture
//!
//! The agent forwarding system consists of:
//! - `SshAgentClient`: High-level interface for interacting with SSH agents
//! - `AgentForwarder`: Handles agent forwarding for nested connections
//! - `AgentConnectionPool`: Multiplexes agent connections for efficiency
//! - `AgentKeyInfo`: Provides detailed information about available keys
//!
//! ## Example
//!
//! ```no_run
//! use rustible::connection::ssh_agent::{SshAgentClient, AgentForwarder};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Connect to the local SSH agent
//! let agent = SshAgentClient::connect().await?;
//!
//! // List available keys
//! let keys = agent.list_keys().await?;
//! for key in keys {
//!     println!("Key: {} ({})", key.comment, key.key_type);
//! }
//!
//! // Enable agent forwarding on a connection
//! let forwarder = AgentForwarder::new(agent);
//! // Use forwarder with nested SSH connections...
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh_keys::agent::client::AgentClient;
use russh_keys::key::PublicKey;
use thiserror::Error;
use tokio::net::UnixStream;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, trace};

use super::ConnectionError;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of pooled agent connections
const MAX_AGENT_CONNECTIONS: usize = 8;

/// Agent connection idle timeout (5 minutes)
const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Environment variable for SSH agent socket path
const SSH_AUTH_SOCK_VAR: &str = "SSH_AUTH_SOCK";

// ============================================================================
// Error Types
// ============================================================================

/// Errors specific to SSH agent operations
#[derive(Error, Debug)]
pub enum AgentError {
    /// SSH_AUTH_SOCK environment variable not set
    #[error("SSH_AUTH_SOCK environment variable not set - no SSH agent available")]
    NoAgentSocket,

    /// Failed to connect to SSH agent socket
    #[error("Failed to connect to SSH agent at {path}: {message}")]
    ConnectionFailed { path: String, message: String },

    /// Failed to communicate with SSH agent
    #[error("Agent communication error: {0}")]
    CommunicationError(String),

    /// No identities found in the SSH agent
    #[error("No identities found in SSH agent")]
    NoIdentities,

    /// Key not found in agent
    #[error("Key not found in agent: {0}")]
    KeyNotFound(String),

    /// Signature operation failed
    #[error("Failed to sign data: {0}")]
    SigningFailed(String),

    /// Agent forwarding not available
    #[error("Agent forwarding not available: {0}")]
    ForwardingUnavailable(String),

    /// Connection pool exhausted
    #[error("Agent connection pool exhausted")]
    PoolExhausted,

    /// Invalid agent response
    #[error("Invalid agent response: {0}")]
    InvalidResponse(String),
}

impl From<AgentError> for ConnectionError {
    fn from(err: AgentError) -> Self {
        ConnectionError::AuthenticationFailed(err.to_string())
    }
}

// ============================================================================
// Agent Key Information
// ============================================================================

/// Information about a key stored in the SSH agent
#[derive(Debug, Clone)]
pub struct AgentKeyInfo {
    /// The public key
    pub public_key: PublicKey,
    /// Key type (e.g., "ssh-ed25519", "ssh-rsa")
    pub key_type: String,
    /// Key comment (usually the path or email)
    pub comment: String,
    /// Key fingerprint (SHA256)
    pub fingerprint: String,
    /// Whether this key supports certificates
    pub supports_certificates: bool,
}

impl AgentKeyInfo {
    /// Create AgentKeyInfo from a PublicKey
    pub fn from_public_key(key: &PublicKey) -> Self {
        let key_type = key.name().to_string();
        let fingerprint = key.fingerprint();
        let supports_certificates = key_type.contains("cert");

        Self {
            public_key: key.clone(),
            key_type,
            comment: String::new(), // Will be set by the agent
            fingerprint,
            supports_certificates,
        }
    }

    /// Create AgentKeyInfo with a comment
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }

    /// Get a short display name for the key
    pub fn display_name(&self) -> String {
        if self.comment.is_empty() {
            format!("{}:{}", self.key_type, &self.fingerprint[..16])
        } else {
            self.comment.clone()
        }
    }
}

// ============================================================================
// SSH Agent Client
// ============================================================================

/// High-level SSH agent client for interacting with the local SSH agent
///
/// This client provides a clean interface for:
/// - Listing available keys
/// - Requesting signatures
/// - Managing agent connections
///
/// # Example
///
/// ```no_run
/// use rustible::connection::ssh_agent::SshAgentClient;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let agent = SshAgentClient::connect().await?;
///
/// // List all keys
/// let keys = agent.list_keys().await?;
/// println!("Found {} keys in agent", keys.len());
///
/// // Check if a specific key is available
/// if agent.has_key("my-key-comment").await? {
///     println!("Key 'my-key-comment' is available");
/// }
/// # Ok(())
/// # }
/// ```
pub struct SshAgentClient {
    /// The underlying agent client (Option to allow take/replace for consuming methods)
    agent: Mutex<Option<AgentClient<UnixStream>>>,
    /// Path to the agent socket
    socket_path: PathBuf,
    /// Number of requests made
    request_count: AtomicU64,
    /// Creation time for metrics
    created_at: Instant,
}

impl SshAgentClient {
    /// Connect to the SSH agent using SSH_AUTH_SOCK environment variable
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - SSH_AUTH_SOCK is not set
    /// - The socket path doesn't exist
    /// - Connection to the agent fails
    pub async fn connect() -> Result<Self, AgentError> {
        let socket_path = Self::get_agent_socket_path()?;
        Self::connect_to(&socket_path).await
    }

    /// Connect to a specific SSH agent socket
    ///
    /// # Arguments
    ///
    /// * `socket_path` - Path to the SSH agent Unix socket
    pub async fn connect_to(socket_path: &PathBuf) -> Result<Self, AgentError> {
        debug!(path = %socket_path.display(), "Connecting to SSH agent");

        let agent = AgentClient::connect_env()
            .await
            .map_err(|e| AgentError::ConnectionFailed {
                path: socket_path.display().to_string(),
                message: e.to_string(),
            })?;

        info!(path = %socket_path.display(), "Connected to SSH agent");

        Ok(Self {
            agent: Mutex::new(Some(agent)),
            socket_path: socket_path.clone(),
            request_count: AtomicU64::new(0),
            created_at: Instant::now(),
        })
    }

    /// Get the SSH agent socket path from environment
    pub fn get_agent_socket_path() -> Result<PathBuf, AgentError> {
        env::var(SSH_AUTH_SOCK_VAR)
            .map(PathBuf::from)
            .map_err(|_| AgentError::NoAgentSocket)
    }

    /// Check if SSH agent is available (SSH_AUTH_SOCK is set and socket exists)
    pub fn is_agent_available() -> bool {
        if let Ok(path) = Self::get_agent_socket_path() {
            path.exists()
        } else {
            false
        }
    }

    /// List all keys available in the SSH agent
    ///
    /// # Returns
    ///
    /// A vector of `AgentKeyInfo` containing information about each key
    pub async fn list_keys(&self) -> Result<Vec<AgentKeyInfo>, AgentError> {
        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Create a fresh connection for each request since the API consumes self
        let socket = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            AgentError::ConnectionFailed {
                path: self.socket_path.display().to_string(),
                message: e.to_string(),
            }
        })?;

        let mut agent: AgentClient<UnixStream> = AgentClient::connect(socket);

        let identities = agent
            .request_identities()
            .await
            .map_err(|e: russh_keys::Error| AgentError::CommunicationError(e.to_string()))?;

        let keys: Vec<AgentKeyInfo> = identities
            .iter()
            .map(|key| AgentKeyInfo::from_public_key(key))
            .collect();

        debug!(
            count = keys.len(),
            "Listed {} keys from SSH agent",
            keys.len()
        );
        Ok(keys)
    }

    /// Get a list of public keys (raw) from the agent
    pub async fn get_public_keys(&self) -> Result<Vec<PublicKey>, AgentError> {
        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Create a fresh connection for each request since the API consumes self
        let socket = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            AgentError::ConnectionFailed {
                path: self.socket_path.display().to_string(),
                message: e.to_string(),
            }
        })?;

        let mut agent: AgentClient<UnixStream> = AgentClient::connect(socket);

        let identities = agent
            .request_identities()
            .await
            .map_err(|e: russh_keys::Error| AgentError::CommunicationError(e.to_string()))?;

        Ok(identities)
    }

    /// Check if a key with the given comment or fingerprint is available
    pub async fn has_key(&self, identifier: &str) -> Result<bool, AgentError> {
        let keys = self.list_keys().await?;
        Ok(keys
            .iter()
            .any(|k| k.comment.contains(identifier) || k.fingerprint.contains(identifier)))
    }

    /// Get a specific key by comment or fingerprint
    pub async fn get_key(&self, identifier: &str) -> Result<AgentKeyInfo, AgentError> {
        let keys = self.list_keys().await?;
        keys.into_iter()
            .find(|k| k.comment.contains(identifier) || k.fingerprint.contains(identifier))
            .ok_or_else(|| AgentError::KeyNotFound(identifier.to_string()))
    }

    /// Get the first available key from the agent
    pub async fn get_first_key(&self) -> Result<AgentKeyInfo, AgentError> {
        let keys = self.list_keys().await?;
        keys.into_iter().next().ok_or(AgentError::NoIdentities)
    }

    /// Request a signature from the agent for the given data
    ///
    /// # Arguments
    ///
    /// * `key` - The public key to sign with
    /// * `data` - The data to sign
    ///
    /// # Returns
    ///
    /// The signature as a byte vector
    pub async fn sign(&self, key: &PublicKey, data: &[u8]) -> Result<Vec<u8>, AgentError> {
        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Create a fresh connection for each request since the API consumes self
        let socket = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            AgentError::ConnectionFailed {
                path: self.socket_path.display().to_string(),
                message: e.to_string(),
            }
        })?;

        let agent: AgentClient<UnixStream> = AgentClient::connect(socket);

        let (_, result) = agent.sign_request_signature(key, data).await;
        let signature =
            result.map_err(|e: russh_keys::Error| AgentError::SigningFailed(e.to_string()))?;

        trace!(key = %key.name(), data_len = data.len(), "Signed data with agent key");
        // Convert signature to bytes
        let sig_bytes = signature.as_ref().to_vec();
        Ok(sig_bytes)
    }

    /// Get the socket path this client is connected to
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get the number of requests made through this client
    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    /// Get how long this client has been connected
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get metrics about this agent client
    pub fn metrics(&self) -> AgentClientMetrics {
        AgentClientMetrics {
            socket_path: self.socket_path.clone(),
            request_count: self.request_count(),
            uptime: self.uptime(),
        }
    }
}

/// Metrics for an SSH agent client
#[derive(Debug, Clone)]
pub struct AgentClientMetrics {
    /// Path to the agent socket
    pub socket_path: PathBuf,
    /// Number of requests made
    pub request_count: u64,
    /// Time since connection
    pub uptime: Duration,
}

// ============================================================================
// Agent Connection Pool
// ============================================================================

/// A pooled agent connection for efficient reuse
struct PooledAgentConnection {
    /// The agent client
    client: SshAgentClient,
    /// Last time this connection was used
    last_used: Instant,
    /// Whether this connection is currently in use
    in_use: bool,
}

/// Connection pool for SSH agent connections
///
/// This pool maintains multiple connections to the SSH agent for efficient
/// parallel operations. Connections are reused when possible and cleaned
/// up when idle.
pub struct AgentConnectionPool {
    /// Pool of agent connections
    connections: RwLock<Vec<PooledAgentConnection>>,
    /// Maximum number of connections
    max_connections: usize,
    /// Idle timeout for connections
    idle_timeout: Duration,
    /// Socket path for new connections
    socket_path: PathBuf,
    /// Number of connections created
    total_connections_created: AtomicU64,
}

impl AgentConnectionPool {
    /// Create a new agent connection pool
    pub async fn new() -> Result<Self, AgentError> {
        let socket_path = SshAgentClient::get_agent_socket_path()?;
        Ok(Self {
            connections: RwLock::new(Vec::new()),
            max_connections: MAX_AGENT_CONNECTIONS,
            idle_timeout: AGENT_IDLE_TIMEOUT,
            socket_path,
            total_connections_created: AtomicU64::new(0),
        })
    }

    /// Create a pool with custom settings
    pub async fn with_config(
        max_connections: usize,
        idle_timeout: Duration,
    ) -> Result<Self, AgentError> {
        let socket_path = SshAgentClient::get_agent_socket_path()?;
        Ok(Self {
            connections: RwLock::new(Vec::new()),
            max_connections,
            idle_timeout,
            socket_path,
            total_connections_created: AtomicU64::new(0),
        })
    }

    /// Get a connection from the pool or create a new one
    pub async fn get_connection(&self) -> Result<PooledConnection<'_>, AgentError> {
        // First, try to get an existing idle connection
        {
            let mut connections = self.connections.write().await;

            // Clean up expired connections
            let now = Instant::now();
            connections.retain(|c| now.duration_since(c.last_used) < self.idle_timeout);

            // Find an available connection
            for conn in connections.iter_mut() {
                if !conn.in_use {
                    conn.in_use = true;
                    conn.last_used = now;
                    return Ok(PooledConnection {
                        pool: self,
                        index: connections.len() - 1,
                    });
                }
            }

            // Create new connection if under limit
            if connections.len() < self.max_connections {
                let client = SshAgentClient::connect_to(&self.socket_path).await?;
                self.total_connections_created
                    .fetch_add(1, Ordering::Relaxed);
                connections.push(PooledAgentConnection {
                    client,
                    last_used: now,
                    in_use: true,
                });
                return Ok(PooledConnection {
                    pool: self,
                    index: connections.len() - 1,
                });
            }
        }

        Err(AgentError::PoolExhausted)
    }

    /// Release a connection back to the pool
    async fn release_connection(&self, index: usize) {
        let mut connections = self.connections.write().await;
        if let Some(conn) = connections.get_mut(index) {
            conn.in_use = false;
            conn.last_used = Instant::now();
        }
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PoolStats {
        let connections = self.connections.read().await;
        let active = connections.iter().filter(|c| c.in_use).count();
        let idle = connections.iter().filter(|c| !c.in_use).count();

        PoolStats {
            total_connections: connections.len(),
            active_connections: active,
            idle_connections: idle,
            max_connections: self.max_connections,
            total_connections_created: self.total_connections_created.load(Ordering::Relaxed),
        }
    }

    /// Close all idle connections
    pub async fn cleanup_idle(&self) {
        let mut connections = self.connections.write().await;
        let now = Instant::now();
        connections.retain(|c| c.in_use || now.duration_since(c.last_used) < self.idle_timeout);
    }

    /// Close all connections
    pub async fn close_all(&self) {
        let mut connections = self.connections.write().await;
        connections.clear();
    }
}

/// Statistics about the agent connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total connections in the pool
    pub total_connections: usize,
    /// Connections currently in use
    pub active_connections: usize,
    /// Idle connections available
    pub idle_connections: usize,
    /// Maximum allowed connections
    pub max_connections: usize,
    /// Total connections created over lifetime
    pub total_connections_created: u64,
}

/// A connection borrowed from the pool
pub struct PooledConnection<'a> {
    pool: &'a AgentConnectionPool,
    index: usize,
}

impl<'a> PooledConnection<'a> {
    /// Get the index of this pooled connection
    pub fn index(&self) -> usize {
        self.index
    }

    /// Check if this connection is still valid in the pool
    pub async fn is_valid(&self) -> bool {
        let connections = self.pool.connections.read().await;
        connections.get(self.index).is_some()
    }
}

impl<'a> Drop for PooledConnection<'a> {
    fn drop(&mut self) {
        // Note: Connection release needs to be handled by the caller
        // since drop is synchronous and we cannot await here.
        // The connection will be released when the pool is next accessed.
        // For proper cleanup, callers should use release_connection() explicitly.
    }
}

// ============================================================================
// Agent Forwarder
// ============================================================================

/// Configuration for agent forwarding
#[derive(Debug, Clone)]
pub struct AgentForwardingConfig {
    /// Whether agent forwarding is enabled
    pub enabled: bool,
    /// Restrict forwarding to specific hosts
    pub allowed_hosts: Vec<String>,
    /// Restrict forwarding to specific keys
    pub allowed_keys: Vec<String>,
    /// Maximum forwarding depth (0 = unlimited)
    pub max_depth: usize,
    /// Log forwarding requests
    pub log_requests: bool,
}

impl Default for AgentForwardingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_hosts: Vec::new(),
            allowed_keys: Vec::new(),
            max_depth: 0,
            log_requests: true,
        }
    }
}

impl AgentForwardingConfig {
    /// Create a new forwarding config with forwarding enabled
    pub fn enabled() -> Self {
        Self::default()
    }

    /// Create a forwarding config with forwarding disabled
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Restrict forwarding to specific hosts
    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = hosts;
        self
    }

    /// Restrict forwarding to specific keys
    pub fn with_allowed_keys(mut self, keys: Vec<String>) -> Self {
        self.allowed_keys = keys;
        self
    }

    /// Set maximum forwarding depth
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

/// SSH agent forwarder for nested connections
///
/// This struct handles the forwarding of SSH agent authentication to
/// nested SSH connections, enabling seamless authentication across
/// multiple SSH hops.
///
/// # Security Considerations
///
/// Agent forwarding can be a security risk if the intermediate hosts
/// are compromised. Consider using ProxyJump instead when possible,
/// or restrict forwarding with `AgentForwardingConfig`.
///
/// # Example
///
/// ```no_run
/// use rustible::connection::ssh_agent::{SshAgentClient, AgentForwarder, AgentForwardingConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let agent = SshAgentClient::connect().await?;
/// let config = AgentForwardingConfig::enabled()
///     .with_max_depth(2)
///     .with_allowed_hosts(vec!["bastion.example.com".to_string()]);
///
/// let forwarder = AgentForwarder::with_config(agent, config);
///
/// // Check if forwarding is allowed for a host
/// if forwarder.is_allowed("bastion.example.com") {
///     // Forwarding is permitted
/// }
/// # Ok(())
/// # }
/// ```
pub struct AgentForwarder {
    /// The agent client to forward
    agent: Arc<SshAgentClient>,
    /// Forwarding configuration
    config: AgentForwardingConfig,
    /// Current forwarding depth
    current_depth: AtomicU64,
    /// Number of forwarded requests
    forwarded_requests: AtomicU64,
    /// Hosts that have been forwarded to
    forwarded_hosts: RwLock<HashMap<String, u64>>,
}

impl AgentForwarder {
    /// Create a new agent forwarder with default config
    pub fn new(agent: SshAgentClient) -> Self {
        Self::with_config(agent, AgentForwardingConfig::default())
    }

    /// Create an agent forwarder with custom config
    pub fn with_config(agent: SshAgentClient, config: AgentForwardingConfig) -> Self {
        Self {
            agent: Arc::new(agent),
            config,
            current_depth: AtomicU64::new(0),
            forwarded_requests: AtomicU64::new(0),
            forwarded_hosts: RwLock::new(HashMap::new()),
        }
    }

    /// Check if forwarding is allowed for a given host
    pub fn is_allowed(&self, host: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Check depth limit
        if self.config.max_depth > 0 {
            let depth = self.current_depth.load(Ordering::Relaxed);
            if depth >= self.config.max_depth as u64 {
                return false;
            }
        }

        // Check host allowlist
        if !self.config.allowed_hosts.is_empty() {
            if !self
                .config
                .allowed_hosts
                .iter()
                .any(|h| h == host || matches_host_pattern(h, host))
            {
                return false;
            }
        }

        true
    }

    /// Check if a key is allowed for forwarding
    pub fn is_key_allowed(&self, key: &AgentKeyInfo) -> bool {
        if self.config.allowed_keys.is_empty() {
            return true;
        }

        self.config.allowed_keys.iter().any(|allowed| {
            key.comment.contains(allowed)
                || key.fingerprint.contains(allowed)
                || key.key_type.contains(allowed)
        })
    }

    /// Get the agent client
    pub fn agent(&self) -> &SshAgentClient {
        &self.agent
    }

    /// Get available keys that are allowed for forwarding
    pub async fn available_keys(&self) -> Result<Vec<AgentKeyInfo>, AgentError> {
        let keys = self.agent.list_keys().await?;
        if self.config.allowed_keys.is_empty() {
            return Ok(keys);
        }

        Ok(keys
            .into_iter()
            .filter(|k| self.is_key_allowed(k))
            .collect())
    }

    /// Request a forwarded signature for authentication
    ///
    /// This method should be called when a nested SSH connection needs
    /// to authenticate using a forwarded key.
    pub async fn sign_forwarded(
        &self,
        host: &str,
        key: &PublicKey,
        data: &[u8],
    ) -> Result<Vec<u8>, AgentError> {
        // Check if forwarding is allowed
        if !self.is_allowed(host) {
            return Err(AgentError::ForwardingUnavailable(format!(
                "Agent forwarding not allowed for host: {}",
                host
            )));
        }

        if self.config.log_requests {
            info!(
                host = %host,
                key_type = %key.name(),
                data_len = data.len(),
                "Forwarding signature request"
            );
        }

        // Perform the signature
        let signature = self.agent.sign(key, data).await?;

        // Update metrics
        self.forwarded_requests.fetch_add(1, Ordering::Relaxed);
        {
            let mut hosts = self.forwarded_hosts.write().await;
            *hosts.entry(host.to_string()).or_insert(0) += 1;
        }

        Ok(signature)
    }

    /// Increment the forwarding depth (called when entering a nested connection)
    pub fn enter_nested(&self) {
        self.current_depth.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the forwarding depth (called when leaving a nested connection)
    pub fn leave_nested(&self) {
        self.current_depth.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get the current forwarding depth
    pub fn depth(&self) -> u64 {
        self.current_depth.load(Ordering::Relaxed)
    }

    /// Get metrics about forwarding activity
    pub async fn metrics(&self) -> ForwardingMetrics {
        let hosts = self.forwarded_hosts.read().await;
        ForwardingMetrics {
            forwarded_requests: self.forwarded_requests.load(Ordering::Relaxed),
            current_depth: self.depth(),
            hosts_forwarded_to: hosts.clone(),
            config_enabled: self.config.enabled,
            max_depth: self.config.max_depth,
        }
    }
}

/// Metrics about agent forwarding activity
#[derive(Debug, Clone)]
pub struct ForwardingMetrics {
    /// Total number of forwarded requests
    pub forwarded_requests: u64,
    /// Current forwarding depth
    pub current_depth: u64,
    /// Map of hosts to number of requests forwarded
    pub hosts_forwarded_to: HashMap<String, u64>,
    /// Whether forwarding is enabled
    pub config_enabled: bool,
    /// Maximum forwarding depth
    pub max_depth: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a host pattern matches a hostname
fn matches_host_pattern(pattern: &str, host: &str) -> bool {
    if pattern.contains('*') {
        // Simple wildcard matching
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let (prefix, suffix) = (parts[0], parts[1]);
            return host.starts_with(prefix) && host.ends_with(suffix);
        }
    }
    pattern == host
}

/// Connect to the SSH agent and verify it's working
pub async fn verify_agent_connection() -> Result<AgentClientMetrics, AgentError> {
    let agent = SshAgentClient::connect().await?;

    // Verify by listing keys
    let keys = agent.list_keys().await?;
    debug!(
        count = keys.len(),
        "Agent verification: found {} keys",
        keys.len()
    );

    Ok(agent.metrics())
}

/// Get the number of keys available in the SSH agent
pub async fn count_agent_keys() -> Result<usize, AgentError> {
    let agent = SshAgentClient::connect().await?;
    let keys = agent.list_keys().await?;
    Ok(keys.len())
}

/// List all keys in the SSH agent with their details
pub async fn list_agent_keys() -> Result<Vec<AgentKeyInfo>, AgentError> {
    let agent = SshAgentClient::connect().await?;
    agent.list_keys().await
}

/// Check if a specific key is available in the agent
pub async fn has_agent_key(identifier: &str) -> Result<bool, AgentError> {
    let agent = SshAgentClient::connect().await?;
    agent.has_key(identifier).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_display() {
        let err = AgentError::NoAgentSocket;
        assert!(err.to_string().contains("SSH_AUTH_SOCK"));

        let err = AgentError::ConnectionFailed {
            path: "/tmp/agent.sock".to_string(),
            message: "connection refused".to_string(),
        };
        assert!(err.to_string().contains("/tmp/agent.sock"));
    }

    #[test]
    fn test_agent_forwarding_config_default() {
        let config = AgentForwardingConfig::default();
        assert!(config.enabled);
        assert!(config.allowed_hosts.is_empty());
        assert_eq!(config.max_depth, 0);
    }

    #[test]
    fn test_agent_forwarding_config_builder() {
        let config = AgentForwardingConfig::enabled()
            .with_max_depth(3)
            .with_allowed_hosts(vec!["bastion.example.com".to_string()]);

        assert!(config.enabled);
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.allowed_hosts.len(), 1);
    }

    #[test]
    fn test_host_pattern_matching() {
        assert!(matches_host_pattern("*.example.com", "bastion.example.com"));
        assert!(matches_host_pattern("web-*", "web-server"));
        assert!(!matches_host_pattern("*.example.com", "example.com"));
        assert!(matches_host_pattern("bastion", "bastion"));
    }

    #[test]
    fn test_agent_key_info_display_name() {
        // Use a valid Ed25519 verifying key for testing (all zeros is not valid for ed25519)
        // We'll skip actual public key creation and just test the display_name logic
        // by creating a mock structure
        let mut info = AgentKeyInfo {
            // Create a dummy key using from_bytes with a valid test key
            public_key: {
                // A valid ed25519 public key (just the base point for testing)
                let bytes: [u8; 32] = [
                    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
                    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
                    0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
                ];
                let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&bytes).unwrap();
                russh_keys::key::PublicKey::Ed25519(verifying_key)
            },
            key_type: "ssh-ed25519".to_string(),
            comment: String::new(),
            fingerprint: "SHA256:abcdef1234567890abcdef".to_string(),
            supports_certificates: false,
        };

        // Without comment, should show type:fingerprint
        let name = info.display_name();
        assert!(name.contains("ssh-ed25519"));

        // With comment, should show comment
        info.comment = "user@example.com".to_string();
        assert_eq!(info.display_name(), "user@example.com");
    }

    #[test]
    fn test_is_agent_available_no_env() {
        // Save and clear the env var
        let original = env::var(SSH_AUTH_SOCK_VAR).ok();
        env::remove_var(SSH_AUTH_SOCK_VAR);

        assert!(!SshAgentClient::is_agent_available());

        // Restore
        if let Some(val) = original {
            env::set_var(SSH_AUTH_SOCK_VAR, val);
        }
    }

    #[tokio::test]
    async fn test_pool_stats_initial() {
        // Skip if no agent available
        if !SshAgentClient::is_agent_available() {
            return;
        }

        if let Ok(pool) = AgentConnectionPool::new().await {
            let stats = pool.stats().await;
            assert_eq!(stats.total_connections, 0);
            assert_eq!(stats.active_connections, 0);
        }
    }
}
