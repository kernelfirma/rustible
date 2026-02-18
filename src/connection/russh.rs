//! Russh connection module
//!
//! This module provides SSH connectivity using the russh crate.
//! Russh is a modern, async-native SSH library that provides better
//! performance and integration with Tokio compared to ssh2.

use async_trait::async_trait;
use russh::client::{AuthResult, Handle, Handler, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::AgentClient;
use russh::keys::load_secret_key;
use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::PublicKeyBase64;
use russh::keys::{Algorithm, HashAlg, PublicKey};
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, trace, warn};

use crate::security::BecomeValidator;

/// Threshold for using streaming uploads (1MB)
const STREAM_THRESHOLD: u64 = 1024 * 1024;

/// Chunk size for streaming transfers (64KB)
const CHUNK_SIZE: usize = 64 * 1024;

/// Maximum number of concurrent transfers for batch/directory operations
const MAX_CONCURRENT_TRANSFERS: usize = 10;

// ============================================================================
// Performance Constants
// ============================================================================

/// Default keepalive interval in seconds (0 = disabled)
const DEFAULT_KEEPALIVE_INTERVAL: u64 = 15;

/// Connection warmup timeout
#[allow(dead_code)]
const WARMUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Minimum time between keepalive pings
const MIN_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const JUMP_ORIGINATOR_ADDRESS: &str = "127.0.0.1";
const JUMP_ORIGINATOR_PORT: u32 = 0;

use super::config::{ConnectionConfig, HostConfig};
use super::ssh_common;
use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    JumpHostChain, JumpHostConfig, JumpHostResolver, RusshError, TransferOptions,
};

// ============================================================================
// Progress Callback Types for File Transfers
// ============================================================================

/// Progress information for a file transfer
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// Path of the file being transferred
    pub path: PathBuf,
    /// Total size of the file in bytes (0 if unknown)
    pub total_bytes: u64,
    /// Number of bytes transferred so far
    pub transferred_bytes: u64,
    /// Transfer direction
    pub direction: TransferDirection,
    /// Current transfer phase
    pub phase: TransferPhase,
}

impl TransferProgress {
    /// Create a new transfer progress for upload
    pub fn upload(path: impl Into<PathBuf>, total_bytes: u64) -> Self {
        Self {
            path: path.into(),
            total_bytes,
            transferred_bytes: 0,
            direction: TransferDirection::Upload,
            phase: TransferPhase::Starting,
        }
    }

    /// Create a new transfer progress for download
    pub fn download(path: impl Into<PathBuf>, total_bytes: u64) -> Self {
        Self {
            path: path.into(),
            total_bytes,
            transferred_bytes: 0,
            direction: TransferDirection::Download,
            phase: TransferPhase::Starting,
        }
    }

    /// Get the percentage completed (0-100)
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.transferred_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }

    /// Check if the transfer is complete
    pub fn is_complete(&self) -> bool {
        matches!(self.phase, TransferPhase::Completed)
    }

    /// Update the transferred bytes
    pub fn update(&mut self, transferred: u64) {
        self.transferred_bytes = transferred;
        self.phase = if self.transferred_bytes >= self.total_bytes && self.total_bytes > 0 {
            TransferPhase::Completed
        } else {
            TransferPhase::Transferring
        };
    }
}

/// Direction of the transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Current phase of the transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPhase {
    Starting,
    Transferring,
    Completed,
    Finalizing,
}

/// Callback type for progress updates
pub type ProgressCallback = Arc<dyn Fn(&TransferProgress) + Send + Sync>;

/// Batch progress information for multiple file transfers
#[derive(Debug, Clone)]
pub struct BatchTransferProgress {
    /// Total number of files to transfer
    pub total_files: usize,
    /// Number of files completed
    pub completed_files: usize,
    /// Number of files that succeeded
    pub successful_files: usize,
    /// Number of files that failed
    pub failed_files: usize,
    /// Total bytes across all files
    pub total_bytes: u64,
    /// Bytes transferred so far
    pub transferred_bytes: u64,
    /// Current file being transferred
    pub current_file: Option<TransferProgress>,
}

impl BatchTransferProgress {
    /// Create a new batch progress tracker
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        Self {
            total_files,
            completed_files: 0,
            successful_files: 0,
            failed_files: 0,
            total_bytes,
            transferred_bytes: 0,
            current_file: None,
        }
    }

    /// Get the overall percentage completed
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            if self.total_files == 0 {
                100.0
            } else {
                (self.completed_files as f64 / self.total_files as f64) * 100.0
            }
        } else {
            (self.transferred_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }

    /// Check if the batch is complete
    pub fn is_complete(&self) -> bool {
        self.completed_files >= self.total_files
    }
}

/// Callback type for batch progress updates
pub type BatchProgressCallback = Arc<dyn Fn(&BatchTransferProgress) + Send + Sync>;

/// Result of a batch transfer operation
#[derive(Debug)]
pub struct BatchTransferResult {
    /// Number of successful transfers
    pub successful: usize,
    /// Number of failed transfers
    pub failed: usize,
    /// Individual results for each file
    pub results: Vec<SingleTransferResult>,
}

impl BatchTransferResult {
    /// Check if all transfers succeeded
    pub fn all_succeeded(&self) -> bool {
        self.failed == 0
    }
    /// Get all errors
    pub fn errors(&self) -> Vec<&ConnectionError> {
        self.results
            .iter()
            .filter_map(|r| r.error.as_ref())
            .collect()
    }
}

/// Result of a single file transfer within a batch
#[derive(Debug)]
pub struct SingleTransferResult {
    /// Local path
    pub local_path: PathBuf,
    /// Remote path
    pub remote_path: PathBuf,
    /// Whether the transfer succeeded
    pub success: bool,
    /// Error if the transfer failed
    pub error: Option<ConnectionError>,
    /// Number of bytes transferred
    pub bytes_transferred: u64,
}

/// Options for directory transfer operations
#[derive(Debug, Clone, Default)]
pub struct DirectoryTransferOptions {
    /// Base transfer options
    pub transfer_options: TransferOptions,
    /// Whether to preserve directory structure
    pub preserve_structure: bool,
    /// Whether to follow symbolic links
    pub follow_symlinks: bool,
    /// Pattern to exclude files
    pub exclude_patterns: Vec<String>,
    /// Pattern to include only matching files
    pub include_patterns: Vec<String>,
    /// Maximum recursion depth (None for unlimited)
    pub max_depth: Option<usize>,
    /// Number of parallel transfers
    pub parallelism: Option<usize>,
}

impl DirectoryTransferOptions {
    /// Create new directory transfer options
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the base transfer options
    pub fn with_transfer_options(mut self, options: TransferOptions) -> Self {
        self.transfer_options = options;
        self
    }
    /// Enable/disable preserving directory structure
    pub fn with_preserve_structure(mut self, preserve: bool) -> Self {
        self.preserve_structure = preserve;
        self
    }
    /// Enable/disable following symbolic links
    pub fn with_follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }
    /// Add an exclude pattern
    pub fn with_exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude_patterns.push(pattern.into());
        self
    }
    /// Add an include pattern
    pub fn with_include(mut self, pattern: impl Into<String>) -> Self {
        self.include_patterns.push(pattern.into());
        self
    }
    /// Set maximum recursion depth
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }
    /// Set parallelism level
    pub fn with_parallelism(mut self, parallelism: usize) -> Self {
        self.parallelism = Some(parallelism);
        self
    }
    /// Get the effective parallelism
    pub fn effective_parallelism(&self) -> usize {
        self.parallelism.unwrap_or(MAX_CONCURRENT_TRANSFERS)
    }
}

/// Escape a path for safe use in shell commands
///
/// Uses single quotes and escapes any single quotes within the string.
/// This is the safest way to pass arbitrary paths to shell commands.
fn escape_shell_arg(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Result of host key verification
#[derive(Debug, Clone, PartialEq)]
enum HostKeyStatus {
    /// Key matches known_hosts entry
    Verified,
    /// Host not found in known_hosts (first connection)
    Unknown,
    /// Key doesn't match known_hosts entry (potential MITM attack)
    Mismatch,
}

/// Client handler for russh with host key verification
struct ClientHandler {
    /// The hostname we're connecting to (for known_hosts lookup)
    host: String,
    /// The port we're connecting to
    port: u16,
    /// Known hosts entries loaded from ~/.ssh/known_hosts
    known_hosts: Vec<KnownHostEntry>,
    /// Path to known_hosts file
    known_hosts_path: Option<PathBuf>,
    /// Whether to accept unknown hosts (first connection)
    accept_unknown: bool,
    /// Whether agent forwarding is enabled for this connection
    forward_agent: bool,
}

/// A parsed entry from known_hosts file
#[derive(Debug, Clone)]
struct KnownHostEntry {
    /// Hostnames/patterns this entry applies to
    patterns: Vec<String>,
    /// The public key
    key: PublicKey,
}

impl ClientHandler {
    /// Create a new client handler with host key verification
    fn new(host: &str, port: u16, accept_unknown: bool, known_hosts_path: Option<PathBuf>) -> Self {
        let path = known_hosts_path
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts")));

        let known_hosts = Self::load_known_hosts(path.as_deref());

        Self {
            host: host.to_string(),
            port,
            known_hosts,
            known_hosts_path: path,
            accept_unknown,
            forward_agent: false,
        }
    }

    /// Set whether agent forwarding is enabled
    fn with_forward_agent(mut self, forward_agent: bool) -> Self {
        self.forward_agent = forward_agent;
        self
    }

    /// Load and parse ~/.ssh/known_hosts file
    fn load_known_hosts(path: Option<&Path>) -> Vec<KnownHostEntry> {
        let mut entries = Vec::new();

        let path = match path {
            Some(p) if p.exists() => p,
            _ => return entries,
        };

        // Read and parse the file
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "Failed to read known_hosts file");
                return entries;
            }
        };

        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse the line: hostname[,hostname...] keytype base64key [comment]
            if let Some(entry) = Self::parse_known_hosts_line(line) {
                entries.push(entry);
            }
        }

        debug!(entry_count = %entries.len(), "Loaded known_hosts entries");
        entries
    }

    /// Parse a single line from known_hosts
    fn parse_known_hosts_line(line: &str) -> Option<KnownHostEntry> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }

        // First part is comma-separated hostnames/patterns
        let patterns: Vec<String> = parts[0].split(',').map(|s| s.to_string()).collect();

        // Second and third parts are key type and base64 key
        let key_type = parts[1];
        let key_data = parts[2];

        // Decode the base64 key
        let key_bytes =
            match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key_data) {
                Ok(b) => b,
                Err(_) => return None,
            };

        // Parse the public key
        // The key format is: 4-byte length + key type string + key data
        let key = match russh::keys::key::parse_public_key(&key_bytes) {
            Ok(k) => k,
            Err(_) => {
                // Try alternative parsing based on key type
                trace!(key_type = %key_type, "Failed to parse key, skipping entry");
                return None;
            }
        };

        Some(KnownHostEntry { patterns, key })
    }

    /// Check if a pattern matches the host
    fn pattern_matches(pattern: &str, host: &str, port: u16) -> bool {
        // Handle [host]:port format
        if pattern.starts_with('[') {
            if let Some(end_bracket) = pattern.find(']') {
                let pattern_host = &pattern[1..end_bracket];
                let pattern_port = pattern
                    .get(end_bracket + 2..)
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(22);
                return pattern_host == host && pattern_port == port;
            }
        }

        // Simple hostname match (port 22 implied)
        if port == 22 && pattern == host {
            return true;
        }

        // Wildcard matching
        if pattern.contains('*') || pattern.contains('?') {
            return Self::wildcard_match(pattern, host);
        }

        false
    }

    /// Simple wildcard matching for known_hosts patterns
    fn wildcard_match(pattern: &str, text: &str) -> bool {
        let mut pattern_chars = pattern.chars().peekable();
        let mut text_chars = text.chars().peekable();

        while let Some(pc) = pattern_chars.next() {
            match pc {
                '*' => {
                    // * matches zero or more characters
                    if pattern_chars.peek().is_none() {
                        return true; // trailing * matches everything
                    }
                    // Try matching rest of pattern at each position
                    let rest_pattern: String = pattern_chars.collect();
                    let rest_text: String = text_chars.collect();
                    for i in 0..=rest_text.len() {
                        if Self::wildcard_match(&rest_pattern, &rest_text[i..]) {
                            return true;
                        }
                    }
                    return false;
                }
                '?' => {
                    // ? matches exactly one character
                    if text_chars.next().is_none() {
                        return false;
                    }
                }
                c => {
                    if text_chars.next() != Some(c) {
                        return false;
                    }
                }
            }
        }

        text_chars.next().is_none()
    }

    /// Verify a server key against known_hosts
    fn verify_host_key(&self, server_key: &PublicKey) -> HostKeyStatus {
        for entry in &self.known_hosts {
            for pattern in &entry.patterns {
                if Self::pattern_matches(pattern, &self.host, self.port) {
                    // Found a matching host entry - compare keys
                    if Self::keys_equal(&entry.key, server_key) {
                        return HostKeyStatus::Verified;
                    }
                    // Key mismatch - potential MITM attack
                    warn!(
                        host = %self.host,
                        "Host key mismatch! The server's key differs from known_hosts"
                    );
                    return HostKeyStatus::Mismatch;
                }
            }
        }

        // Host not found in known_hosts
        HostKeyStatus::Unknown
    }

    /// Compare two public keys for equality
    fn keys_equal(a: &PublicKey, b: &PublicKey) -> bool {
        // Compare the key fingerprints using SHA-256
        a.fingerprint(HashAlg::Sha256) == b.fingerprint(HashAlg::Sha256)
    }

    /// Add a new host key to known_hosts file
    fn add_to_known_hosts(&self, server_key: &PublicKey) {
        let path = match &self.known_hosts_path {
            Some(p) => p,
            None => return,
        };

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    warn!(error = %e, "Failed to create .ssh directory");
                    return;
                }
            }
        }

        // Format the host string
        let host_str = if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        };

        // Format the key
        let key_type = server_key.algorithm().to_string();
        let key_base64 = server_key.public_key_base64();

        let entry_line = format!("{} {} {}\n", host_str, key_type, key_base64);

        // Append to file
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(entry_line.as_bytes()) {
                    warn!(error = %e, "Failed to write to known_hosts file");
                } else {
                    info!(host = %self.host, path = %path.display(), "Added new host key to known_hosts");
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to open known_hosts file for writing");
            }
        }
    }
}

impl Handler for ClientHandler {
    type Error = RusshError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match self.verify_host_key(server_public_key) {
            HostKeyStatus::Verified => {
                debug!(host = %self.host, "Host key verified against known_hosts");
                Ok(true)
            }
            HostKeyStatus::Unknown => {
                if self.accept_unknown {
                    warn!(
                        host = %self.host,
                        "Host not found in known_hosts, accepting (first connection)"
                    );
                    self.add_to_known_hosts(server_public_key);
                    Ok(true)
                } else {
                    warn!(
                        host = %self.host,
                        "Host not found in known_hosts, rejecting"
                    );
                    Ok(false)
                }
            }
            HostKeyStatus::Mismatch => {
                // This is a security issue - key has changed
                warn!(
                    host = %self.host,
                    "HOST KEY VERIFICATION FAILED! Server key does not match known_hosts entry."
                );
                Ok(false)
            }
        }
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        if !self.forward_agent {
            warn!(host = %self.host, "Server opened agent channel but forwarding is disabled, ignoring");
            return Ok(());
        }

        debug!(host = %self.host, "Server opened agent forwarding channel, starting proxy");

        // Spawn a task to proxy agent protocol between the channel and local agent
        let host = self.host.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy_agent_channel(channel).await {
                warn!(host = %host, error = %e, "Agent forwarding channel error");
            }
        });

        Ok(())
    }
}

/// Proxy SSH agent protocol messages between a russh channel and the local SSH agent.
///
/// The SSH agent protocol uses length-prefixed messages (4-byte big-endian length + body).
/// This function connects to the local agent via `SSH_AUTH_SOCK` and bidirectionally
/// relays complete agent protocol messages.
async fn proxy_agent_channel(
    mut channel: russh::Channel<russh::client::Msg>,
) -> Result<(), ConnectionError> {
    let socket_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
        ConnectionError::InvalidConfig("SSH_AUTH_SOCK not set, cannot proxy agent".into())
    })?;

    let agent_stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .map_err(|e| {
            ConnectionError::ConnectionFailed(format!(
                "Failed to connect to local SSH agent at {}: {}",
                socket_path, e
            ))
        })?;

    let (agent_read, agent_write) = tokio::io::split(agent_stream);
    let agent_read = Arc::new(Mutex::new(agent_read));
    let agent_write = Arc::new(Mutex::new(agent_write));

    // Forward data from channel to agent and responses back
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => {
                // Write request data to the local agent
                {
                    let mut writer = agent_write.lock().await;
                    if let Err(e) = writer.write_all(data).await {
                        debug!(error = %e, "Failed to write to agent socket");
                        break;
                    }
                    if let Err(e) = writer.flush().await {
                        debug!(error = %e, "Failed to flush agent socket");
                        break;
                    }
                }

                // Read response from agent (length-prefixed: 4 bytes length + body)
                let mut reader = agent_read.lock().await;
                let mut len_buf = [0u8; 4];
                if let Err(e) = reader.read_exact(&mut len_buf).await {
                    debug!(error = %e, "Failed to read agent response length");
                    break;
                }
                let response_len = u32::from_be_bytes(len_buf) as usize;

                // Read the response body
                let mut response_body = vec![0u8; response_len];
                if let Err(e) = reader.read_exact(&mut response_body).await {
                    debug!(error = %e, "Failed to read agent response body");
                    break;
                }

                // Send length + body back through the channel
                let mut response = Vec::with_capacity(4 + response_len);
                response.extend_from_slice(&len_buf);
                response.extend_from_slice(&response_body);

                let mut cursor = std::io::Cursor::new(response);
                if let Err(e) = channel.data(&mut cursor).await {
                    debug!(error = %e, "Failed to send agent response to channel");
                    break;
                }
            }
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                break;
            }
            _ => {
                // Ignore other message types
            }
        }
    }

    let _ = channel.eof().await;
    Ok(())
}

/// Russh connection implementation with performance optimizations
///
/// This implementation uses RwLock instead of Mutex for the handle to reduce
/// lock contention during parallel operations. Most operations only need read
/// access to get a reference to the Handle for opening channels - only close()
/// needs write access to take ownership of the handle.
///
/// # Performance Features
///
/// - **Keepalive Support**: Prevents connection drops from idle timeouts
/// - **Connection Metrics**: Tracks latency and operation counts for monitoring
/// - **Fast Ciphers**: Prefers ChaCha20-Poly1305 and AES-GCM
/// - **TCP_NODELAY**: Reduces latency by disabling Nagle's algorithm
pub struct RusshConnection {
    /// Session identifier
    identifier: String,
    /// Russh client handle - uses RwLock for better parallel performance
    /// Read lock: channel operations (execute, upload, download, etc.)
    /// Write lock: close operation only
    handle: Arc<RwLock<Option<Handle<ClientHandler>>>>,
    /// Host configuration (kept for future connection pooling improvements)
    #[allow(dead_code)]
    host_config: HostConfig,
    /// Whether the connection is established
    connected: Arc<AtomicBool>,
    /// Last keepalive time (nanos since epoch for atomic ops)
    last_keepalive: AtomicU64,
    /// Connection creation time
    created_at: Instant,
    /// Total commands executed (for metrics)
    commands_executed: AtomicU64,
    /// Keepalive interval (0 = disabled)
    keepalive_interval: Duration,
    /// Jump host handles kept alive for ProxyJump connections
    jump_handles: Arc<Mutex<Vec<Handle<ClientHandler>>>>,
}

/// Connection performance metrics
#[derive(Debug, Clone)]
pub struct ConnectionMetrics {
    /// Connection identifier
    pub identifier: String,
    /// Time since connection was established
    pub uptime: Duration,
    /// Number of commands executed
    pub commands_executed: u64,
    /// Whether connection is still active
    pub is_connected: bool,
}

impl RusshConnection {
    /// Build command string with options (no environment variables)
    fn build_command(command: &str, options: &ExecuteOptions) -> ConnectionResult<String> {
        let mut parts = Vec::new();

        // Add working directory
        if let Some(cwd) = &options.cwd {
            parts.push(format!("cd {} && ", cwd));
        }

        // Handle privilege escalation
        if options.escalate {
            let escalate_method = options.escalate_method.as_deref().unwrap_or("sudo");
            let escalate_user = options.escalate_user.as_deref().unwrap_or("root");

            BecomeValidator::new()
                .validate_username(escalate_user)
                .map_err(|e| {
                    ConnectionError::InvalidConfig(format!(
                        "Invalid escalation user '{}': {}",
                        escalate_user, e
                    ))
                })?;

            match escalate_method {
                "sudo" => {
                    if options.escalate_password.is_some() {
                        parts.push(format!("sudo -S -u {} -- ", escalate_user));
                    } else {
                        parts.push(format!("sudo -u {} -- ", escalate_user));
                    }
                }
                "su" => {
                    parts.push(format!("su - {} -c ", escalate_user));
                }
                "doas" => {
                    parts.push(format!("doas -u {} ", escalate_user));
                }
                _ => {
                    parts.push(format!("sudo -u {} -- ", escalate_user));
                }
            }
        }

        parts.push(command.to_string());
        Ok(parts.concat())
    }

    /// Build command string with options, including environment variables
    ///
    /// Since russh doesn't support the SSH request_env protocol, we prepend
    /// environment variable exports to the command.
    fn build_command_with_env(command: &str, options: &ExecuteOptions) -> ConnectionResult<String> {
        let mut parts = Vec::new();

        // Prepend environment variables as exports
        if !options.env.is_empty() {
            for (key, value) in &options.env {
                // Use export to set environment variables
                // Escape the value to handle special characters
                let escaped_value = value.replace('\'', "'\\''");
                parts.push(format!("export {}='{}'; ", key, escaped_value));
            }
        }

        // Add the rest of the command using the base build_command
        parts.push(Self::build_command(command, options)?);
        Ok(parts.concat())
    }

    /// Open an SFTP session
    async fn open_sftp(handle: &Handle<ClientHandler>) -> ConnectionResult<SftpSession> {
        let channel = handle.channel_open_session().await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to open channel: {}", e))
        })?;

        channel.request_subsystem(true, "sftp").await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to request SFTP subsystem: {}", e))
        })?;

        SftpSession::new(channel.into_stream()).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to create SFTP session: {}", e))
        })
    }

    /// Create remote directories recursively via SFTP
    async fn create_remote_dirs_sftp(sftp: &SftpSession, path: &Path) -> ConnectionResult<()> {
        let mut current = PathBuf::new();

        for component in path.components() {
            current.push(component);

            // Skip root
            if current.to_string_lossy() == "/" {
                continue;
            }

            // Try to create directory (ignore error if it already exists)
            let _ = sftp.create_dir(current.to_string_lossy().to_string()).await;
        }

        Ok(())
    }

    // ========================================================================
    // Performance Optimization Methods
    // ========================================================================

    /// Send a keepalive ping if enough time has passed
    ///
    /// This method checks if the keepalive interval has elapsed and sends
    /// a keepalive message to prevent connection timeouts from firewalls/NAT.
    ///
    /// Returns true if a keepalive was sent, false otherwise.
    pub async fn send_keepalive_if_needed(&self) -> ConnectionResult<bool> {
        if self.keepalive_interval.is_zero() {
            return Ok(false);
        }

        let now = Instant::now();
        let last_keepalive_nanos = self.last_keepalive.load(Ordering::Relaxed);

        // Check if enough time has passed since last keepalive
        let elapsed = if last_keepalive_nanos == 0 {
            self.created_at.elapsed()
        } else {
            Duration::from_nanos(now.elapsed().as_nanos() as u64 - last_keepalive_nanos)
        };

        if elapsed < self.keepalive_interval.max(MIN_KEEPALIVE_INTERVAL) {
            return Ok(false);
        }

        // Send keepalive by opening and immediately closing a channel
        let handle_guard = self.handle.read().await;
        if let Some(handle) = handle_guard.as_ref() {
            match handle.channel_open_session().await {
                Ok(channel) => {
                    let _ = channel.exec(true, "true").await;
                    let _ = channel.eof().await;
                    self.last_keepalive
                        .store(now.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    trace!(identifier = %self.identifier, "Sent keepalive");
                    Ok(true)
                }
                Err(e) => {
                    warn!(error = %e, "Keepalive failed, connection may be dead");
                    Err(ConnectionError::ConnectionFailed(format!(
                        "Keepalive failed: {}",
                        e
                    )))
                }
            }
        } else {
            Err(ConnectionError::ConnectionClosed)
        }
    }

    /// Get connection metrics for monitoring
    pub fn metrics(&self) -> ConnectionMetrics {
        ConnectionMetrics {
            identifier: self.identifier.clone(),
            uptime: self.created_at.elapsed(),
            commands_executed: self.commands_executed.load(Ordering::Relaxed),
            is_connected: self.connected.load(Ordering::Relaxed),
        }
    }

    /// Pre-warm the connection by validating it's ready for commands
    ///
    /// Call this method after connecting to prepare the connection for
    /// operations. This validates the connection is healthy.
    pub async fn warm_up(&self) -> ConnectionResult<()> {
        debug!(identifier = %self.identifier, "Warming up connection");

        // Validate connection by opening a test channel
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        let channel = handle.channel_open_session().await.map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to open warmup channel: {}", e))
        })?;

        // Execute a simple command to validate
        channel.exec(true, "true").await.map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Warmup command failed: {}", e))
        })?;

        let _ = channel.eof().await;

        info!(identifier = %self.identifier, "Connection warmed up");
        Ok(())
    }
}

impl RusshConnection {
    /// Connect to a remote host via SSH using russh
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<Self> {
        let resolved =
            ssh_common::resolve_connection_params(host, port, user, host_config, global_config);
        let host_config = resolved.host_config.clone();
        let retry_config = resolved.retry_config.clone();
        let actual_host = resolved.host.clone();
        let actual_port = resolved.port;
        let actual_user = resolved.user.clone();
        let timeout = resolved.timeout;
        let identifier = resolved.identifier.clone();

        debug!(
            host = %actual_host,
            port = %actual_port,
            user = %actual_user,
            "Connecting via SSH (russh)"
        );

        let mut jump_resolver = JumpHostResolver::new(global_config);
        let jump_chain = jump_resolver.resolve_from_config(&host_config)?;

        let (handle, jump_handles) = if jump_chain.is_empty() {
            // Connect with retry logic
            let handle =
                ssh_common::connect_with_retry_async(&retry_config, "SSH connection", || {
                    Self::do_connect(
                        &actual_host,
                        actual_port,
                        &actual_user,
                        &host_config,
                        global_config,
                        timeout,
                    )
                })
                .await?;
            (handle, Vec::new())
        } else {
            debug!(
                identifier = %identifier,
                jump_chain = %jump_chain,
                "Connecting via jump host chain"
            );
            Self::connect_via_jump_chain(&jump_chain, &resolved, global_config).await?
        };

        // Determine keepalive interval from host config or use default
        let keepalive_interval = host_config
            .server_alive_interval
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(DEFAULT_KEEPALIVE_INTERVAL));

        let conn = Self {
            identifier,
            handle: Arc::new(RwLock::new(Some(handle))),
            host_config,
            connected: Arc::new(AtomicBool::new(true)),
            last_keepalive: AtomicU64::new(0),
            created_at: Instant::now(),
            commands_executed: AtomicU64::new(0),
            keepalive_interval,
            jump_handles: Arc::new(Mutex::new(jump_handles)),
        };

        debug!(
            identifier = %conn.identifier,
            keepalive_interval_secs = %keepalive_interval.as_secs(),
            "SSH connection established with performance optimizations"
        );

        Ok(conn)
    }

    fn build_client_config(timeout: Duration) -> Arc<russh::client::Config> {
        // Create optimized russh client configuration.
        // Modern servers typically support these fast algorithms.
        let config = russh::client::Config {
            inactivity_timeout: Some(timeout),
            // Optimize preferred algorithms for faster negotiation
            // Prefer fast key exchange algorithms
            preferred: russh::Preferred {
                kex: std::borrow::Cow::Borrowed(&[
                    russh::kex::CURVE25519,
                    russh::kex::CURVE25519_PRE_RFC_8731,
                ]),
                // Prefer fast ciphers (only use AES-256-GCM as AES-128-GCM isn't available)
                cipher: std::borrow::Cow::Borrowed(&[
                    russh::cipher::CHACHA20_POLY1305,
                    russh::cipher::AES_256_GCM,
                ]),
                // Prefer fast key types
                key: std::borrow::Cow::Borrowed(&[
                    Algorithm::Ed25519,
                    Algorithm::Rsa {
                        hash: Some(HashAlg::Sha256),
                    },
                    Algorithm::Rsa {
                        hash: Some(HashAlg::Sha512),
                    },
                ]),
                // Prefer fast MACs (not used with AEAD ciphers but needed for fallback)
                mac: std::borrow::Cow::Borrowed(&[
                    russh::mac::HMAC_SHA256,
                    russh::mac::HMAC_SHA512,
                ]),
                // No compression for speed
                compression: std::borrow::Cow::Borrowed(&[russh::compression::NONE]),
            },
            ..Default::default()
        };
        Arc::new(config)
    }

    fn build_client_handler(host: &str, port: u16, host_config: &HostConfig) -> ClientHandler {
        // Determine strict host key checking setting
        // If strict_host_key_checking is:
        // - Some(true): reject unknown hosts (accept_unknown = false)
        // - Some(false): accept unknown hosts (accept_unknown = true)
        // - None: default to accepting unknown hosts (accept_unknown = true)
        let accept_unknown = !host_config.strict_host_key_checking.unwrap_or(false);

        // Use configured known_hosts file if provided
        let known_hosts_path = ssh_common::user_known_hosts_path(host_config);

        ClientHandler::new(host, port, accept_unknown, known_hosts_path)
            .with_forward_agent(host_config.forward_agent)
    }

    /// Perform the actual connection
    async fn do_connect(
        host: &str,
        port: u16,
        user: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
        timeout: Duration,
    ) -> ConnectionResult<Handle<ClientHandler>> {
        let config = Self::build_client_config(timeout);

        // Connect to the SSH server
        let addr = format!("{}:{}", host, port);
        let socket = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr))
            .await
            .map_err(|_| ConnectionError::Timeout(timeout.as_secs()))?
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!("Failed to connect to {}: {}", addr, e))
            })?;

        // Enable TCP_NODELAY for lower latency
        socket.set_nodelay(true).map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to set TCP_NODELAY: {}", e))
        })?;

        let handler = Self::build_client_handler(host, port, host_config);

        let mut session = russh::client::connect_stream(config, socket, handler)
            .await
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!("SSH handshake failed: {}", e))
            })?;

        // Authenticate
        Self::authenticate(&mut session, user, host_config, global_config).await?;

        debug!("SSH connection established successfully");
        Ok(session)
    }

    async fn do_connect_stream<S>(
        stream: S,
        host: &str,
        port: u16,
        user: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
        timeout: Duration,
    ) -> ConnectionResult<Handle<ClientHandler>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let config = Self::build_client_config(timeout);
        let handler = Self::build_client_handler(host, port, host_config);

        let mut session = russh::client::connect_stream(config, stream, handler)
            .await
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!("SSH handshake failed: {}", e))
            })?;

        // Authenticate
        Self::authenticate(&mut session, user, host_config, global_config).await?;

        debug!(
            host = %host,
            port = %port,
            user = %user,
            "SSH connection established successfully (stream)"
        );
        Ok(session)
    }

    fn resolve_jump_params(
        jump: &JumpHostConfig,
        default_user: &str,
        global_config: &ConnectionConfig,
    ) -> ssh_common::ResolvedConnectionParams {
        let mut host_config = global_config.get_host_merged(&jump.host);

        if let Some(user) = &jump.user {
            host_config.user = Some(user.clone());
        }
        if jump.port != 22 {
            host_config.port = Some(jump.port);
        }
        if let Some(identity_file) = &jump.identity_file {
            host_config.identity_file = Some(identity_file.clone());
        }

        ssh_common::resolve_connection_params(
            &jump.host,
            jump.port,
            default_user,
            Some(host_config),
            global_config,
        )
    }

    async fn connect_via_jump_channel(
        handle: &Handle<ClientHandler>,
        params: &ssh_common::ResolvedConnectionParams,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<Handle<ClientHandler>> {
        let channel = handle
            .channel_open_direct_tcpip(
                params.host.as_str(),
                params.port as u32,
                JUMP_ORIGINATOR_ADDRESS,
                JUMP_ORIGINATOR_PORT,
            )
            .await
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!(
                    "Failed to open jump channel to {}:{}: {}",
                    params.host, params.port, e
                ))
            })?;

        let stream = channel.into_stream();

        Self::do_connect_stream(
            stream,
            &params.host,
            params.port,
            &params.user,
            &params.host_config,
            global_config,
            params.timeout,
        )
        .await
    }

    async fn connect_via_jump_chain(
        jump_chain: &JumpHostChain,
        target: &ssh_common::ResolvedConnectionParams,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<(Handle<ClientHandler>, Vec<Handle<ClientHandler>>)> {
        let mut jump_handles = Vec::new();
        let jump_params: Vec<_> = jump_chain
            .iter()
            .map(|jump| Self::resolve_jump_params(jump, &target.user, global_config))
            .collect();

        let first = jump_params.first().ok_or_else(|| {
            ConnectionError::InvalidConfig("ProxyJump chain resolved to empty".to_string())
        })?;

        let mut current_handle = ssh_common::connect_with_retry_async(
            &first.retry_config,
            "SSH jump host connection",
            || {
                Self::do_connect(
                    &first.host,
                    first.port,
                    &first.user,
                    &first.host_config,
                    global_config,
                    first.timeout,
                )
            },
        )
        .await?;

        for params in jump_params.iter().skip(1) {
            let next_handle = ssh_common::connect_with_retry_async(
                &params.retry_config,
                "SSH jump host hop",
                || Self::connect_via_jump_channel(&current_handle, params, global_config),
            )
            .await?;

            jump_handles.push(current_handle);
            current_handle = next_handle;
        }

        let target_handle = ssh_common::connect_with_retry_async(
            &target.retry_config,
            "SSH jump host target connection",
            || Self::connect_via_jump_channel(&current_handle, target, global_config),
        )
        .await?;

        jump_handles.push(current_handle);

        Ok((target_handle, jump_handles))
    }

    /// Perform SSH authentication
    async fn authenticate(
        session: &mut Handle<ClientHandler>,
        user: &str,
        host_config: &HostConfig,
        global_config: &ConnectionConfig,
    ) -> ConnectionResult<()> {
        // Try SSH agent first if enabled
        if global_config.defaults.use_agent && Self::try_agent_auth(session, user).await.is_ok() {
            debug!("Authenticated using SSH agent");
            return Ok(());
        }

        // Try key-based authentication
        for key_path in ssh_common::identity_file_candidates(host_config, global_config) {
            if Self::try_key_auth(session, user, &key_path, host_config.password.as_deref())
                .await
                .is_ok()
            {
                debug!(key = %key_path.display(), "Authenticated using key");
                return Ok(());
            }
        }

        // Try password authentication
        if let Some(password) = &host_config.password {
            let result = session
                .authenticate_password(user, password)
                .await
                .map_err(|e| {
                    ConnectionError::AuthenticationFailed(format!(
                        "Password authentication failed: {}",
                        e
                    ))
                })?;

            if matches!(result, AuthResult::Success) {
                debug!("Authenticated using password");
                return Ok(());
            }
        }

        // Try keyboard-interactive authentication
        if let Some(password) = &host_config.password {
            if Self::try_keyboard_interactive_auth(session, user, password)
                .await
                .is_ok()
            {
                debug!("Authenticated using keyboard-interactive");
                return Ok(());
            }
        }

        Err(ConnectionError::AuthenticationFailed(
            "All authentication methods failed".to_string(),
        ))
    }

    /// Try SSH agent authentication
    ///
    /// Connects to the SSH agent via SSH_AUTH_SOCK environment variable,
    /// retrieves available identities, and attempts authentication with each.
    async fn try_agent_auth(
        session: &mut Handle<ClientHandler>,
        user: &str,
    ) -> ConnectionResult<()> {
        // Connect to SSH agent using SSH_AUTH_SOCK environment variable
        let mut agent = AgentClient::connect_env().await.map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to connect to SSH agent: {}", e))
        })?;

        // Get available identities from the agent
        let identities = agent.request_identities().await.map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to get agent identities: {}", e))
        })?;

        if identities.is_empty() {
            return Err(ConnectionError::AuthenticationFailed(
                "SSH agent has no identities".to_string(),
            ));
        }

        debug!(identity_count = %identities.len(), "Found SSH agent identities");

        // Try each identity until one works
        for identity in identities {
            trace!("Trying SSH agent identity");

            // Use authenticate_publickey_with which accepts a Signer trait (russh 0.54+ API)
            // AgentClient implements Signer
            let result = session
                .authenticate_publickey_with(user, identity.clone(), None, &mut agent)
                .await;

            match result {
                Ok(AuthResult::Success) => {
                    debug!("SSH agent authentication successful");
                    return Ok(());
                }
                Ok(AuthResult::Failure { .. }) => {
                    // Key was rejected, try the next one
                    trace!("Identity rejected, trying next");
                }
                Err(e) => {
                    // Log the error but continue trying other keys
                    trace!(error = %e, "Agent authentication attempt failed");
                }
            }
        }

        Err(ConnectionError::AuthenticationFailed(
            "All SSH agent identities rejected".to_string(),
        ))
    }

    /// Try key-based authentication
    ///
    /// Supports Ed25519 and RSA keys, with or without passphrases.
    /// The key is loaded using russh::keys::load_secret_key which automatically
    /// detects the key type (Ed25519, RSA, etc.) based on the file format.
    async fn try_key_auth(
        session: &mut Handle<ClientHandler>,
        user: &str,
        key_path: &Path,
        passphrase: Option<&str>,
    ) -> ConnectionResult<()> {
        if !key_path.exists() {
            return Err(ConnectionError::AuthenticationFailed(format!(
                "Key file not found: {}",
                key_path.display()
            )));
        }

        // Load the private key
        let key_pair = if let Some(pass) = passphrase {
            // Load with passphrase
            load_secret_key(key_path, Some(pass)).map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Failed to load key {} with passphrase: {}",
                    key_path.display(),
                    e
                ))
            })?
        } else {
            // Try loading without passphrase first
            load_secret_key(key_path, None).map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Failed to load key {}: {}",
                    key_path.display(),
                    e
                ))
            })?
        };

        // Wrap key with hash algorithm for authentication (russh 0.54+ API)
        // For RSA keys, use SHA-256; for other keys, hash_alg is ignored
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key_pair), Some(HashAlg::Sha256));

        // Authenticate with the key
        let result = session
            .authenticate_publickey(user, key_with_alg)
            .await
            .map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Key authentication failed for {}: {}",
                    key_path.display(),
                    e
                ))
            })?;

        if matches!(result, AuthResult::Success) {
            Ok(())
        } else {
            Err(ConnectionError::AuthenticationFailed(
                "Key authentication failed".to_string(),
            ))
        }
    }

    /// Try keyboard-interactive authentication
    async fn try_keyboard_interactive_auth(
        session: &mut Handle<ClientHandler>,
        user: &str,
        password: &str,
    ) -> ConnectionResult<()> {
        let mut response = session
            .authenticate_keyboard_interactive_start(user, None::<String>)
            .await
            .map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Keyboard-interactive authentication start failed: {}",
                    e
                ))
            })?;

        loop {
            match response {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure { .. } => {
                    return Err(ConnectionError::AuthenticationFailed(
                        "Keyboard-interactive authentication failed".to_string(),
                    ))
                }
                KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                    let responses = prompts.iter().map(|_| password.to_string()).collect();
                    response = session
                        .authenticate_keyboard_interactive_respond(responses)
                        .await
                        .map_err(|e| {
                            ConnectionError::AuthenticationFailed(format!(
                                "Keyboard-interactive authentication failed: {}",
                                e
                            ))
                        })?;
                }
            }
        }
    }
}

#[async_trait]
impl Connection for RusshConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        // Check if we're marked as connected (lock-free check)
        if !self.connected.load(Ordering::SeqCst) {
            return false;
        }

        // Check if we have a handle using read lock (allows concurrent checks)
        let has_handle = self.handle.read().await.is_some();
        if !has_handle {
            return false;
        }

        // We consider the connection alive if it's marked as connected and has a handle
        // A full health check would require opening a channel, but that's expensive
        // The connection will be marked as dead when an operation fails
        true
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        // Build the full command with options
        // Prepend environment variables to the command since russh doesn't have request_env
        let full_command = Self::build_command_with_env(command, &options)?;

        trace!(command = %full_command, "Executing remote command");

        // Increment command counter for metrics
        self.commands_executed.fetch_add(1, Ordering::Relaxed);

        // Execute the command with optional timeout
        let execute_future = async {
            // Get the handle using read lock - allows concurrent channel opens
            // We only hold the lock briefly to open a channel
            let handle_guard = self.handle.read().await;
            let handle: &Handle<ClientHandler> = handle_guard
                .as_ref()
                .ok_or(ConnectionError::ConnectionClosed)?;

            // 1. Open a channel (while holding read lock)
            let mut channel = handle.channel_open_session().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to open channel: {}", e))
            })?;

            // Drop the handle guard to release the read lock
            drop(handle_guard);

            // Request agent forwarding if enabled
            if self.host_config.forward_agent {
                if let Err(e) = channel.agent_forward(true).await {
                    warn!("Failed to request agent forwarding: {}", e);
                }
            }

            // 2. Execute the command
            channel.exec(true, full_command).await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to execute command: {}", e))
            })?;

            // Handle escalation password if needed
            if options.escalate && options.escalate_password.is_some() {
                let password = options.escalate_password.as_ref().unwrap();
                let password_data = format!("{}\n", password);
                let mut cursor = tokio::io::BufReader::new(password_data.as_bytes());
                channel.data(&mut cursor).await.map_err(|e| {
                    ConnectionError::ExecutionFailed(format!("Failed to write password: {}", e))
                })?;
            }

            // 3. Capture stdout/stderr
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_code = None;

            // Read all messages from the channel
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        stdout.extend_from_slice(data);
                    }
                    ChannelMsg::ExtendedData { ref data, ext } => {
                        // Extended data type 1 is stderr
                        if ext == 1 {
                            stderr.extend_from_slice(data);
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = Some(exit_status);
                    }
                    ChannelMsg::Eof => {
                        // End of file, continue reading until channel closes
                    }
                    ChannelMsg::Close => {
                        // Channel closed, we're done
                        break;
                    }
                    _ => {
                        // Ignore other message types
                    }
                }
            }

            // Wait for channel to close
            let _ = channel.eof().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to send EOF: {}", e))
            });

            // 4. Return CommandResult
            // Exit status from SSH is u32, but we need i32 for CommandResult
            // Use i32::MAX for unknown exit code (None case) as it indicates an error
            let exit_code: i32 = exit_code.map(|e| e as i32).unwrap_or(i32::MAX);
            let stdout_str = String::from_utf8_lossy(&stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr).to_string();

            trace!(exit_code = %exit_code, "Command completed");

            if exit_code == 0 {
                Ok(CommandResult::success(stdout_str, stderr_str))
            } else {
                Ok(CommandResult::failure(exit_code, stdout_str, stderr_str))
            }
        };

        // Apply timeout if specified
        if let Some(timeout_secs) = options.timeout {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), execute_future).await {
                Ok(result) => result,
                Err(_) => Err(ConnectionError::Timeout(timeout_secs)),
            }
        } else {
            execute_future.await
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
            "Uploading file via SFTP"
        );

        // Get handle using read lock - allows concurrent uploads
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                Self::create_remote_dirs_sftp(&sftp, parent).await?;
            }
        }

        // Read local file
        let content = tokio::fs::read(local_path).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to read local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        // Create/open remote file for writing
        // Use open() with explicit flags and attributes to set mode atomically if provided
        let remote_path_str = remote_path.to_string_lossy().to_string();

        let flags = russh_sftp::protocol::OpenFlags::WRITE
            | russh_sftp::protocol::OpenFlags::CREATE
            | russh_sftp::protocol::OpenFlags::TRUNCATE;
        let mut attrs = russh_sftp::protocol::FileAttributes::default();

        if let Some(mode) = options.mode {
            attrs.permissions = Some(mode);
        }

        let mut remote_file = sftp
            .open_with_flags_and_attributes(&remote_path_str, flags, attrs)
            .await
            .map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

        // Write content to remote file
        remote_file.write_all(&content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write to remote file: {}", e))
        })?;

        // Close the file
        drop(remote_file);

        // Drop the SFTP session before using execute()
        drop(sftp);

        // Set owner/group if specified using chown command
        if options.owner.is_some() || options.group.is_some() {
            let escaped_path = escape_shell_arg(&remote_path.to_string_lossy());
            let owner_group = match (&options.owner, &options.group) {
                (Some(owner), Some(group)) => format!("{}:{}", owner, group),
                (Some(owner), None) => owner.clone(),
                (None, Some(group)) => format!(":{}", group),
                (None, None) => unreachable!(),
            };
            let chown_cmd = format!("chown {} {}", owner_group, escaped_path);
            let result = self.execute(&chown_cmd, None).await?;
            if !result.success {
                warn!(
                    "Failed to set owner/group on {}: {}",
                    remote_path.display(),
                    result.stderr
                );
            }
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

        debug!(
            remote = %remote_path.display(),
            size = %content.len(),
            "Uploading content via SFTP"
        );

        // Get handle using read lock - allows concurrent uploads
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                Self::create_remote_dirs_sftp(&sftp, parent).await?;
            }
        }

        // Create/open remote file for writing
        let remote_path_str = remote_path.to_string_lossy().to_string();

        let flags = russh_sftp::protocol::OpenFlags::WRITE
            | russh_sftp::protocol::OpenFlags::CREATE
            | russh_sftp::protocol::OpenFlags::TRUNCATE;
        let mut attrs = russh_sftp::protocol::FileAttributes::default();

        if let Some(mode) = options.mode {
            attrs.permissions = Some(mode);
        }

        let mut remote_file = sftp
            .open_with_flags_and_attributes(&remote_path_str, flags, attrs)
            .await
            .map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create remote file {}: {}",
                    remote_path.display(),
                    e
                ))
            })?;

        // Write content to remote file
        remote_file.write_all(content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write to remote file: {}", e))
        })?;

        // Close the file
        drop(remote_file);

        // Drop the SFTP session before using execute()
        drop(sftp);

        // Set owner/group if specified using chown command
        if options.owner.is_some() || options.group.is_some() {
            let escaped_path = escape_shell_arg(&remote_path.to_string_lossy());
            let owner_group = match (&options.owner, &options.group) {
                (Some(owner), Some(group)) => format!("{}:{}", owner, group),
                (Some(owner), None) => owner.clone(),
                (None, Some(group)) => format!(":{}", group),
                (None, None) => unreachable!(),
            };
            let chown_cmd = format!("chown {} {}", owner_group, escaped_path);
            let result = self.execute(&chown_cmd, None).await?;
            if !result.success {
                warn!(
                    "Failed to set owner/group on {}: {}",
                    remote_path.display(),
                    result.stderr
                );
            }
        }

        Ok(())
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            "Downloading file via russh SFTP"
        );

        // Get handle using read lock - allows concurrent downloads
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Open remote file for reading
        let remote_path_str = remote_path.to_string_lossy().to_string();
        let mut remote_file = sftp.open(&remote_path_str).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to open remote file {}: {}",
                remote_path.display(),
                e
            ))
        })?;

        // Read content from remote file
        let mut content = Vec::new();
        remote_file.read_to_end(&mut content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read remote file: {}", e))
        })?;

        // Create parent directories for local file
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ConnectionError::TransferFailed(format!(
                    "Failed to create local directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Write local file
        tokio::fs::write(local_path, &content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to write local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        debug!("Download completed successfully");
        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(remote = %remote_path.display(), "Downloading content via russh SFTP");

        // Get handle using read lock - allows concurrent downloads
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Open remote file for reading
        let remote_path_str = remote_path.to_string_lossy().to_string();
        let mut remote_file = sftp.open(&remote_path_str).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to open remote file {}: {}",
                remote_path.display(),
                e
            ))
        })?;

        // Read content from remote file
        let mut content = Vec::new();
        remote_file.read_to_end(&mut content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read remote file: {}", e))
        })?;

        debug!(size = %content.len(), "Content download completed successfully");
        Ok(content)
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        trace!(path = %path.display(), "Checking if path exists via SFTP");

        // Get handle using read lock - allows concurrent checks
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Use try_exists to check if path exists
        let path_str = path.to_string_lossy().to_string();
        match sftp.try_exists(&path_str).await {
            Ok(exists) => Ok(exists),
            Err(e) => {
                // Log the error but treat certain errors as "does not exist"
                debug!(path = %path.display(), error = %e, "Error checking path existence");
                Ok(false)
            }
        }
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        trace!(path = %path.display(), "Checking if path is directory via SFTP");

        // Get handle using read lock - allows concurrent checks
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        // Get metadata and check if it's a directory
        let path_str = path.to_string_lossy().to_string();
        match sftp.metadata(&path_str).await {
            Ok(attrs) => Ok(attrs.is_dir()),
            Err(_) => Ok(false),
        }
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        trace!(path = %path.display(), "Getting file stats via SFTP");

        // Get handle using read lock - allows concurrent stat calls
        let handle_guard = self.handle.read().await;
        let handle = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;

        // Open SFTP session (while holding read lock)
        let sftp = Self::open_sftp(handle).await?;

        // Release the read lock immediately after opening SFTP session
        drop(handle_guard);

        let path_str = path.to_string_lossy().to_string();

        // First get symlink metadata to determine if it's a symlink
        let is_symlink = match sftp.symlink_metadata(&path_str).await {
            Ok(attrs) => attrs.is_symlink(),
            Err(_) => false,
        };

        // Get regular metadata (follows symlinks)
        let attrs = sftp.metadata(&path_str).await.map_err(|e| {
            // Check for common SFTP error conditions
            let error_str = e.to_string().to_lowercase();
            if error_str.contains("no such file") || error_str.contains("not found") {
                ConnectionError::TransferFailed(format!("File not found: {}", path.display()))
            } else if error_str.contains("permission denied") {
                ConnectionError::TransferFailed(format!("Permission denied: {}", path.display()))
            } else {
                ConnectionError::TransferFailed(format!("Failed to stat {}: {}", path.display(), e))
            }
        })?;

        // Extract file attributes from russh-sftp FileAttributes
        let size = attrs.size.unwrap_or(0);
        let mode = attrs.permissions.unwrap_or(0);
        let uid = attrs.uid.unwrap_or(0);
        let gid = attrs.gid.unwrap_or(0);
        let atime = attrs.atime.map(|t| t as i64).unwrap_or(0);
        let mtime = attrs.mtime.map(|t| t as i64).unwrap_or(0);

        Ok(FileStat {
            size,
            mode,
            uid,
            gid,
            atime,
            mtime,
            is_dir: attrs.is_dir(),
            is_file: attrs.is_regular(),
            is_symlink,
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        let metrics = self.metrics();
        debug!(
            identifier = %metrics.identifier,
            uptime_secs = %metrics.uptime.as_secs(),
            commands_executed = %metrics.commands_executed,
            "Closing SSH connection"
        );

        // Mark as disconnected first (lock-free)
        self.connected.store(false, Ordering::SeqCst);

        // Take the handle out using write lock - this is the only write operation
        let handle = {
            let mut handle_guard = self.handle.write().await;
            handle_guard.take()
        };

        // Close the connection if we had one
        if let Some(handle) = handle {
            // Request disconnect from the SSH server
            let _ = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Connection closed by client",
                    "en",
                )
                .await;
        }

        let mut jump_handles = self.jump_handles.lock().await;
        for handle in jump_handles.drain(..).rev() {
            let _ = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Connection closed by client",
                    "en",
                )
                .await;
        }

        Ok(())
    }

    /// Execute multiple commands in batch with channel multiplexing
    ///
    /// Overrides the default sequential implementation to use SSH channel
    /// multiplexing for parallel command execution. This significantly
    /// improves performance when running multiple commands.
    async fn execute_batch(
        &self,
        commands: &[&str],
        options: Option<ExecuteOptions>,
    ) -> Vec<ConnectionResult<CommandResult>> {
        if commands.is_empty() {
            return Vec::new();
        }

        // Convert &str slice to String slice for the internal method
        let cmd_strings: Vec<String> = commands.iter().map(|s| s.to_string()).collect();

        self.execute_batch_internal(&cmd_strings, options).await
    }
}

/// Maximum number of concurrent SSH channels to use for batch execution.
/// SSH spec allows many more, but we stay conservative to avoid overwhelming servers.
const MAX_CONCURRENT_CHANNELS: usize = 10;

impl RusshConnection {
    /// Execute multiple commands in parallel using channel multiplexing.
    ///
    /// This method opens multiple SSH channels on the same connection and executes
    /// commands concurrently. Results are returned in the same order as the input commands.
    ///
    /// # Arguments
    ///
    /// * `commands` - A slice of command strings to execute
    /// * `options` - Optional execution options applied to all commands
    ///
    /// # Returns
    ///
    /// A vector of results in the same order as the input commands. Each command
    /// either succeeds with a `CommandResult` or fails with a `ConnectionError`.
    /// If one command fails, others continue executing.
    ///
    /// # Limits
    ///
    /// * Maximum 10 concurrent channels to avoid overwhelming SSH servers
    /// * Per-command timeout (from options), not total timeout
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// # use rustible::connection::Connection;
    /// # let conn = rustible::connection::RusshConnectionBuilder::new("localhost").connect().await?;
    /// let commands = vec!["hostname", "uptime", "date"];
    /// let results = conn.execute_batch(&commands, None).await;
    /// for (cmd, result) in commands.iter().zip(results.iter()) {
    ///     match result {
    ///         Ok(r) => println!("{}: {}", cmd, r.stdout),
    ///         Err(e) => eprintln!("{}: error: {}", cmd, e),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn execute_batch_internal(
        &self,
        commands: &[String],
        options: Option<ExecuteOptions>,
    ) -> Vec<ConnectionResult<CommandResult>> {
        if commands.is_empty() {
            return Vec::new();
        }

        // Quick check if connection is closed
        if !self.connected.load(Ordering::SeqCst) {
            return commands
                .iter()
                .map(|_| Err(ConnectionError::ConnectionClosed))
                .collect();
        }

        let options = options.unwrap_or_default();
        let timeout_duration = options.timeout.map(Duration::from_secs);

        debug!(
            command_count = %commands.len(),
            "Executing batch of commands with channel multiplexing"
        );

        // Get a clone of the handle Arc for spawning tasks
        let handle_arc = self.handle.clone();

        // Prepare all command strings upfront
        let prepared_commands: Vec<(usize, String)> = match commands
            .iter()
            .enumerate()
            .map(|(idx, cmd)| Self::build_command_with_env(cmd, &options).map(|full| (idx, full)))
            .collect::<ConnectionResult<Vec<_>>>()
        {
            Ok(prepared) => prepared,
            Err(e) => {
                let err_msg = e.to_string();
                return commands
                    .iter()
                    .map(|_| Err(ConnectionError::InvalidConfig(err_msg.clone())))
                    .collect();
            }
        };

        // Use semaphore to limit concurrent channels
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CHANNELS));

        // Spawn tasks for each command
        let mut tasks: Vec<tokio::task::JoinHandle<(usize, ConnectionResult<CommandResult>)>> =
            Vec::with_capacity(commands.len());

        for (idx, full_command) in prepared_commands {
            let sem = semaphore.clone();
            let handle_arc = handle_arc.clone();
            let escalate = options.escalate;
            let escalate_password = options.escalate_password.clone();
            let timeout_dur = timeout_duration;

            let task = tokio::spawn(async move {
                // Acquire semaphore permit (limits concurrent channels)
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        return (
                            idx,
                            Err(ConnectionError::ExecutionFailed(
                                "Semaphore closed".to_string(),
                            )),
                        );
                    }
                };

                let result = Self::execute_single_channel(
                    &handle_arc,
                    &full_command,
                    escalate,
                    escalate_password,
                    timeout_dur,
                )
                .await;

                (idx, result)
            });

            tasks.push(task);
        }

        // Wait for all tasks to complete and collect results
        let task_results = futures::future::join_all(tasks).await;

        // Collect results in order
        let mut results: Vec<ConnectionResult<CommandResult>> = Vec::with_capacity(commands.len());

        // Initialize with error placeholders
        for idx in 0..commands.len() {
            results.push(Err(ConnectionError::ExecutionFailed(format!(
                "Command {} failed to execute (task error)",
                idx
            ))));
        }

        // Fill in actual results
        for task_result in task_results {
            match task_result {
                Ok((idx, result)) => {
                    results[idx] = result;
                }
                Err(join_error) => {
                    // This happens if the task panicked - shouldn't normally occur
                    warn!(error = %join_error, "Task panicked during batch execution");
                }
            }
        }

        results
    }

    /// Execute a single command on a new channel.
    ///
    /// This is a helper method used by `execute_batch` to run one command
    /// on its own SSH channel. It opens a new channel, executes the command,
    /// collects output, and returns the result.
    async fn execute_single_channel(
        handle_arc: &Arc<RwLock<Option<Handle<ClientHandler>>>>,
        full_command: &str,
        escalate: bool,
        escalate_password: Option<String>,
        timeout_duration: Option<Duration>,
    ) -> ConnectionResult<CommandResult> {
        let execute_future = async {
            // Get the handle using read lock - allows concurrent channel opens
            let handle_guard = handle_arc.read().await;
            let handle = handle_guard
                .as_ref()
                .ok_or(ConnectionError::ConnectionClosed)?;

            // Open a new channel
            let mut channel = handle.channel_open_session().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to open channel: {}", e))
            })?;

            // Release the lock immediately after opening the channel
            drop(handle_guard);

            // Execute the command on this channel
            channel.exec(true, full_command).await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to execute command: {}", e))
            })?;

            // Handle escalation password if needed
            if escalate {
                if let Some(password) = escalate_password.as_ref() {
                    let password_data = format!("{password}\n");
                    let mut cursor = tokio::io::BufReader::new(password_data.as_bytes());
                    channel.data(&mut cursor).await.map_err(|e| {
                        ConnectionError::ExecutionFailed(format!("Failed to write password: {}", e))
                    })?;
                }
            }

            // Collect stdout, stderr, and exit code
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_code = None;

            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        stdout.extend_from_slice(data);
                    }
                    ChannelMsg::ExtendedData { ref data, ext } => {
                        if ext == 1 {
                            stderr.extend_from_slice(data);
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = Some(exit_status);
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => {
                        if matches!(msg, ChannelMsg::Close) {
                            break;
                        }
                    }
                    _ => {}
                }
            }

            // Send EOF to cleanly close our side
            let _ = channel.eof().await;

            // Build result
            let exit_code = exit_code.map(|e| e as i32).unwrap_or(i32::MAX);
            let stdout_str = String::from_utf8_lossy(&stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr).to_string();

            trace!(exit_code = %exit_code, "Channel command completed");

            if exit_code == 0 {
                Ok(CommandResult::success(stdout_str, stderr_str))
            } else {
                Ok(CommandResult::failure(exit_code, stdout_str, stderr_str))
            }
        };

        // Apply per-command timeout
        if let Some(timeout) = timeout_duration {
            match tokio::time::timeout(timeout, execute_future).await {
                Ok(result) => result,
                Err(_) => Err(ConnectionError::Timeout(timeout.as_secs())),
            }
        } else {
            execute_future.await
        }
    }
}

impl std::fmt::Debug for RusshConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let connected = self.connected.load(Ordering::Relaxed);
        f.debug_struct("RusshConnection")
            .field("identifier", &self.identifier)
            .field("connected", &connected)
            .finish()
    }
}

/// Builder for Russh connections
pub struct RusshConnectionBuilder {
    /// Target host
    pub host: String,
    /// SSH port (default: 22)
    pub port: u16,
    /// Username for authentication
    pub user: String,
    /// Password for authentication (optional)
    pub password: Option<String>,
    /// Path to private key file (optional)
    pub private_key: Option<String>,
    /// Connection timeout in seconds (optional)
    pub timeout: Option<u64>,
    /// Enable compression
    pub compression: bool,
}

impl RusshConnectionBuilder {
    /// Create a new Russh connection builder
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 22,
            user: std::env::var("USER").unwrap_or_else(|_| "root".to_string()),
            password: None,
            private_key: None,
            timeout: Some(30),
            compression: false,
        }
    }

    /// Set the port
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the username
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the password
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the private key path
    pub fn private_key(mut self, path: impl Into<String>) -> Self {
        self.private_key = Some(path.into());
        self
    }

    /// Set the connection timeout
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Enable compression
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Build and connect
    pub async fn connect(self) -> ConnectionResult<RusshConnection> {
        let host_config = HostConfig {
            hostname: Some(self.host.clone()),
            port: Some(self.port),
            user: Some(self.user.clone()),
            password: self.password,
            identity_file: self.private_key,
            connect_timeout: self.timeout,
            compression: self.compression,
            ..Default::default()
        };

        let config = ConnectionConfig::default();
        RusshConnection::connect(
            &self.host,
            self.port,
            &self.user,
            Some(host_config),
            &config,
        )
        .await
    }
}

// ============================================================================
// SSH Request Pipelining
// ============================================================================

/// A pending command in the pipeline
#[derive(Debug, Clone)]
pub struct PendingCommand {
    /// The command string to execute
    command: String,
    /// Options for command execution
    options: ExecuteOptions,
}

impl PendingCommand {
    /// Create a new pending command
    pub fn new(command: impl Into<String>, options: Option<ExecuteOptions>) -> Self {
        Self {
            command: command.into(),
            options: options.unwrap_or_default(),
        }
    }

    /// Get the command string
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Get the execution options
    pub fn options(&self) -> &ExecuteOptions {
        &self.options
    }
}

/// SSH request pipelining executor
///
/// This struct enables true SSH pipelining by opening multiple channels
/// before previous commands finish, executing all commands, and then
/// collecting all results. This significantly reduces latency when
/// executing multiple commands on a remote host.
///
/// # How It Works
///
/// SSH allows multiple channels to be opened on a single connection.
/// Traditional execution waits for each command to complete before
/// starting the next. With pipelining:
///
/// 1. All SSH channels are opened in parallel (without waiting)
/// 2. All commands are executed on their respective channels
/// 3. All outputs are collected concurrently
///
/// This eliminates the round-trip latency between commands.
///
/// # Difference from `execute_batch`
///
/// While `execute_batch` executes commands in parallel using spawned tasks,
/// `PipelinedExecutor` provides a builder pattern for queuing commands
/// without any network activity until `flush()` is called. This allows
/// for more efficient batching when commands are added incrementally.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::connection::{PipelinedExecutor, RusshConnectionBuilder};
///
/// # let conn = RusshConnectionBuilder::new("localhost").connect().await?;
/// let mut pipeline = conn.pipeline();
///
/// // Queue commands - these don't execute yet
/// pipeline.queue("echo 'hello'", None);
/// pipeline.queue("echo 'world'", None);
/// pipeline.queue("date", None);
///
/// // Flush executes all commands with pipelining
/// let results = pipeline.flush().await;
/// for result in results {
///     println!("{:?}", result);
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Memory Usage
///
/// The pipeline stores commands in memory until flush() is called.
/// For very large numbers of commands, consider batching into smaller
/// pipelines to limit memory usage.
pub struct PipelinedExecutor<'a> {
    /// Reference to the underlying SSH connection
    connection: &'a RusshConnection,
    /// Queue of pending commands to execute
    pending: Vec<PendingCommand>,
    /// Default timeout for all commands (in seconds)
    default_timeout: Option<u64>,
}

impl<'a> PipelinedExecutor<'a> {
    /// Create a new pipelined executor for the given connection
    pub fn new(connection: &'a RusshConnection) -> Self {
        Self {
            connection,
            pending: Vec::new(),
            default_timeout: None,
        }
    }

    /// Create a new pipelined executor with a default timeout
    pub fn with_timeout(connection: &'a RusshConnection, timeout_secs: u64) -> Self {
        Self {
            connection,
            pending: Vec::new(),
            default_timeout: Some(timeout_secs),
        }
    }

    /// Create a new pipelined executor with pre-allocated capacity
    pub fn with_capacity(connection: &'a RusshConnection, capacity: usize) -> Self {
        Self {
            connection,
            pending: Vec::with_capacity(capacity),
            default_timeout: None,
        }
    }

    /// Queue a command for execution without blocking
    ///
    /// This method adds a command to the internal queue. The command
    /// will not be executed until `flush()` is called.
    ///
    /// # Arguments
    ///
    /// * `command` - The shell command to execute
    /// * `options` - Optional execution options (cwd, env, timeout, etc.)
    pub fn queue(&mut self, command: impl Into<String>, options: Option<ExecuteOptions>) {
        self.pending.push(PendingCommand::new(command, options));
    }

    /// Queue multiple commands at once
    ///
    /// All commands will use default execution options.
    pub fn queue_all<I, S>(&mut self, commands: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for cmd in commands {
            self.queue(cmd, None);
        }
    }

    /// Queue a command with specific options
    pub fn queue_with_options(&mut self, command: impl Into<String>, options: ExecuteOptions) {
        self.pending
            .push(PendingCommand::new(command, Some(options)));
    }

    /// Get the number of pending commands
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Check if there are any pending commands
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Clear all pending commands without executing them
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Get a reference to pending commands
    pub fn pending(&self) -> &[PendingCommand] {
        &self.pending
    }

    /// Set the default timeout for all commands
    pub fn set_default_timeout(&mut self, timeout_secs: Option<u64>) {
        self.default_timeout = timeout_secs;
    }

    /// Flush the pipeline: send all commands and collect all responses
    ///
    /// This is the core pipelining method. It works by:
    /// 1. Opening all SSH channels in parallel (without waiting for previous ones)
    /// 2. Executing all commands on their respective channels
    /// 3. Collecting all outputs concurrently
    ///
    /// Returns a vector of results in the same order as commands were queued.
    ///
    /// # Errors
    ///
    /// Individual command failures are returned in the result vector.
    /// If the connection is closed, all commands will return `ConnectionClosed` errors.
    pub async fn flush(mut self) -> Vec<ConnectionResult<CommandResult>> {
        if self.pending.is_empty() {
            return Vec::new();
        }

        // Take ownership of pending commands (leaves empty vec to satisfy Drop)
        let commands = std::mem::take(&mut self.pending);
        let num_commands = commands.len();
        let default_timeout = self.default_timeout;

        debug!(
            num_commands = %num_commands,
            "Flushing pipelined commands"
        );

        // Get the SSH handle - use read() since we only need to open channels
        let handle_guard = self.connection.handle.read().await;
        let handle = match handle_guard.as_ref() {
            Some(h) => h,
            None => {
                // Connection is closed, return errors for all commands
                return (0..num_commands)
                    .map(|_| Err(ConnectionError::ConnectionClosed))
                    .collect();
            }
        };

        // Phase 1: Open all channels in parallel
        // This is the key insight for pipelining - we can open channels
        // before the previous ones complete their command execution
        trace!("Phase 1: Opening {} channels in parallel", num_commands);

        let channel_futures: Vec<_> = (0..num_commands)
            .map(|_| handle.channel_open_session())
            .collect();

        let channel_results = futures::future::join_all(channel_futures).await;

        // Drop the handle guard early to allow other operations
        drop(handle_guard);

        // Collect opened channels, tracking which ones failed
        let mut channels: Vec<Option<russh::Channel<russh::client::Msg>>> =
            Vec::with_capacity(num_commands);
        let mut channel_errors: Vec<Option<ConnectionError>> =
            (0..num_commands).map(|_| None).collect();

        for (idx, result) in channel_results.into_iter().enumerate() {
            match result {
                Ok(channel) => {
                    channels.push(Some(channel));
                }
                Err(e) => {
                    channels.push(None);
                    channel_errors[idx] = Some(ConnectionError::ExecutionFailed(format!(
                        "Failed to open channel: {}",
                        e
                    )));
                }
            }
        }

        // Phase 2: Execute commands on all channels
        // Build the full command strings and execute them
        trace!("Phase 2: Executing {} commands", num_commands);

        for (idx, cmd) in commands.iter().enumerate() {
            if channel_errors[idx].is_some() {
                continue; // Skip if channel open failed
            }

            if let Some(Some(channel)) = channels.get_mut(idx) {
                let full_command =
                    match RusshConnection::build_command_with_env(&cmd.command, &cmd.options) {
                        Ok(full_command) => full_command,
                        Err(e) => {
                            channels[idx] = None;
                            channel_errors[idx] = Some(e);
                            continue;
                        }
                    };

                if let Err(e) = channel.exec(true, full_command).await {
                    // Mark this channel as failed
                    channels[idx] = None;
                    channel_errors[idx] = Some(ConnectionError::ExecutionFailed(format!(
                        "Failed to execute command: {}",
                        e
                    )));
                }
            }
        }

        // Handle escalation passwords if needed (for commands that require it)
        for (idx, cmd) in commands.iter().enumerate() {
            if channel_errors[idx].is_some() {
                continue;
            }

            if cmd.options.escalate && cmd.options.escalate_password.is_some() {
                if let Some(Some(channel)) = channels.get_mut(idx) {
                    let password = cmd.options.escalate_password.as_ref().unwrap();
                    let password_data = format!("{}\n", password);
                    let mut cursor = tokio::io::BufReader::new(password_data.as_bytes());

                    if let Err(e) = channel.data(&mut cursor).await {
                        channels[idx] = None;
                        channel_errors[idx] = Some(ConnectionError::ExecutionFailed(format!(
                            "Failed to write password: {}",
                            e
                        )));
                    }
                }
            }
        }

        // Phase 3: Collect outputs from all channels concurrently
        trace!("Phase 3: Collecting outputs from {} channels", num_commands);

        let collect_futures: Vec<_> = channels
            .into_iter()
            .zip(channel_errors.into_iter())
            .zip(commands.iter())
            .map(|((channel_opt, error_opt), cmd)| {
                let timeout = cmd.options.timeout.or(default_timeout);

                async move {
                    // If we already have an error, return it
                    if let Some(e) = error_opt {
                        return Err(e);
                    }

                    // If channel is None, something went wrong
                    let Some(mut channel) = channel_opt else {
                        return Err(ConnectionError::ExecutionFailed(
                            "Channel not available".to_string(),
                        ));
                    };

                    // Collect output with optional timeout
                    let collect_output = async {
                        let mut stdout = Vec::new();
                        let mut stderr = Vec::new();
                        let mut exit_code = None;

                        while let Some(msg) = channel.wait().await {
                            match msg {
                                ChannelMsg::Data { ref data } => {
                                    stdout.extend_from_slice(data);
                                }
                                ChannelMsg::ExtendedData { ref data, ext } => {
                                    if ext == 1 {
                                        stderr.extend_from_slice(data);
                                    }
                                }
                                ChannelMsg::ExitStatus { exit_status } => {
                                    exit_code = Some(exit_status);
                                }
                                ChannelMsg::Eof | ChannelMsg::Close => {
                                    if matches!(msg, ChannelMsg::Close) {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }

                        // Send EOF to properly close the channel
                        let _ = channel.eof().await;

                        let exit_code = exit_code.map(|e| e as i32).unwrap_or(i32::MAX);
                        let stdout_str = String::from_utf8_lossy(&stdout).to_string();
                        let stderr_str = String::from_utf8_lossy(&stderr).to_string();

                        if exit_code == 0 {
                            Ok(CommandResult::success(stdout_str, stderr_str))
                        } else {
                            Ok(CommandResult::failure(exit_code, stdout_str, stderr_str))
                        }
                    };

                    if let Some(timeout_secs) = timeout {
                        match tokio::time::timeout(
                            Duration::from_secs(timeout_secs),
                            collect_output,
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(ConnectionError::Timeout(timeout_secs)),
                        }
                    } else {
                        collect_output.await
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(collect_futures).await;

        debug!(
            num_commands = %num_commands,
            successful = %results.iter().filter(|r| r.is_ok()).count(),
            "Pipeline flush completed"
        );

        results
    }

    /// Flush the pipeline and return results only for successful commands
    ///
    /// This is a convenience method that filters out failed commands
    /// and returns only successful results.
    pub async fn flush_ok(self) -> Vec<CommandResult> {
        self.flush()
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect()
    }

    /// Flush the pipeline and return the first error if any command fails
    ///
    /// Returns Ok with all results if all commands succeed, or the first
    /// error encountered.
    pub async fn flush_all_ok(self) -> ConnectionResult<Vec<CommandResult>> {
        let results = self.flush().await;
        let mut ok_results = Vec::with_capacity(results.len());

        for result in results {
            match result {
                Ok(r) => ok_results.push(r),
                Err(e) => return Err(e),
            }
        }

        Ok(ok_results)
    }

    /// Flush and collect results with their original commands
    ///
    /// Returns tuples of (command, result) for easy correlation.
    pub async fn flush_with_commands(self) -> Vec<(String, ConnectionResult<CommandResult>)> {
        let commands: Vec<String> = self.pending.iter().map(|c| c.command.clone()).collect();
        let results = self.flush().await;

        commands.into_iter().zip(results).collect()
    }
}

impl RusshConnection {
    /// Create a new pipelined executor for this connection
    ///
    /// This allows executing multiple commands with minimal latency
    /// by leveraging SSH channel pipelining.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// # let connection = RusshConnectionBuilder::new("localhost").connect().await?;
    /// let mut pipeline = connection.pipeline();
    /// pipeline.queue("ls -la", None);
    /// pipeline.queue("df -h", None);
    /// pipeline.queue("free -m", None);
    /// let results = pipeline.flush().await;
    /// # Ok(())
    /// # }
    /// ```
    pub fn pipeline(&self) -> PipelinedExecutor<'_> {
        PipelinedExecutor::new(self)
    }

    /// Create a new pipelined executor with a default timeout
    ///
    /// All commands will use this timeout unless overridden in their options.
    pub fn pipeline_with_timeout(&self, timeout_secs: u64) -> PipelinedExecutor<'_> {
        PipelinedExecutor::with_timeout(self, timeout_secs)
    }

    /// Create a new pipelined executor with pre-allocated capacity
    ///
    /// Use this when you know approximately how many commands you'll execute.
    pub fn pipeline_with_capacity(&self, capacity: usize) -> PipelinedExecutor<'_> {
        PipelinedExecutor::with_capacity(self, capacity)
    }

    /// Execute multiple commands with pipelining (convenience method)
    ///
    /// This is a convenience method that creates a pipeline, queues all commands,
    /// and flushes in one call.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// # let connection = RusshConnectionBuilder::new("localhost").connect().await?;
    /// let results = connection.execute_pipelined([
    ///     "echo hello",
    ///     "echo world",
    ///     "date",
    /// ]).await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute_pipelined<I, S>(&self, commands: I) -> Vec<ConnectionResult<CommandResult>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut pipeline = self.pipeline();
        pipeline.queue_all(commands);
        pipeline.flush().await
    }
}

/// Drop implementation ensures pending commands are logged if not flushed
impl<'a> Drop for PipelinedExecutor<'a> {
    fn drop(&mut self) {
        if !self.pending.is_empty() {
            warn!(
                pending_count = %self.pending.len(),
                "PipelinedExecutor dropped with pending commands that were not flushed"
            );
        }
    }
}

// ============================================================================
// Directory Transfer and Progress Operations
// ============================================================================

impl RusshConnection {
    /// Upload a directory recursively with parallel transfers
    pub async fn upload_directory(
        &self,
        local_dir: &Path,
        remote_dir: &Path,
        options: Option<DirectoryTransferOptions>,
        progress: Option<BatchProgressCallback>,
    ) -> ConnectionResult<BatchTransferResult> {
        let options = options.unwrap_or_default();
        if !local_dir.is_dir() {
            return Err(ConnectionError::TransferFailed(format!(
                "Not a directory: {}",
                local_dir.display()
            )));
        }
        debug!(local = %local_dir.display(), remote = %remote_dir.display(), "Starting directory upload");
        let files = self
            .collect_local_files(local_dir, remote_dir, &options, 0)
            .await?;
        if files.is_empty() {
            return Ok(BatchTransferResult {
                successful: 0,
                failed: 0,
                results: Vec::new(),
            });
        }
        let handle_guard = self.handle.read().await;
        let h = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        Self::create_remote_dirs_sftp(&sftp, remote_dir).await?;
        drop(handle_guard);
        self.upload_batch_with_progress(&files, Some(options.transfer_options), progress)
            .await
    }

    async fn collect_local_files(
        &self,
        local_dir: &Path,
        remote_dir: &Path,
        options: &DirectoryTransferOptions,
        depth: usize,
    ) -> ConnectionResult<Vec<(PathBuf, PathBuf)>> {
        if options.max_depth.is_some_and(|max| depth > max) {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(local_dir).await.map_err(|e| {
            ConnectionError::TransferFailed(format!(
                "Failed to read directory {}: {}",
                local_dir.display(),
                e
            ))
        })?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if options
                .exclude_patterns
                .iter()
                .any(|p| glob::Pattern::new(p).is_ok_and(|pat| pat.matches(&name)))
            {
                continue;
            }
            if !options.include_patterns.is_empty()
                && !options
                    .include_patterns
                    .iter()
                    .any(|p| glob::Pattern::new(p).is_ok_and(|pat| pat.matches(&name)))
            {
                continue;
            }
            let remote_path = remote_dir.join(&name);
            let meta = entry.metadata().await.map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to get metadata: {}", e))
            })?;
            if meta.is_dir() {
                files.extend(
                    Box::pin(self.collect_local_files(&path, &remote_path, options, depth + 1))
                        .await?,
                );
            } else if meta.is_file() && (options.follow_symlinks || !meta.file_type().is_symlink())
            {
                files.push((path, remote_path));
            }
        }
        Ok(files)
    }

    /// Download a directory recursively with parallel transfers
    pub async fn download_directory(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        options: Option<DirectoryTransferOptions>,
        progress: Option<BatchProgressCallback>,
    ) -> ConnectionResult<BatchTransferResult> {
        let options = options.unwrap_or_default();
        debug!(remote = %remote_dir.display(), local = %local_dir.display(), "Starting directory download");
        let files = self
            .collect_remote_files(remote_dir, local_dir, &options, 0)
            .await?;
        if files.is_empty() {
            return Ok(BatchTransferResult {
                successful: 0,
                failed: 0,
                results: Vec::new(),
            });
        }
        tokio::fs::create_dir_all(local_dir).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to create directory: {}", e))
        })?;
        self.download_batch_with_progress(&files, progress).await
    }

    async fn collect_remote_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        options: &DirectoryTransferOptions,
        depth: usize,
    ) -> ConnectionResult<Vec<(PathBuf, PathBuf)>> {
        if options.max_depth.is_some_and(|max| depth > max) {
            return Ok(Vec::new());
        }
        let handle_guard = self.handle.read().await;
        let h = handle_guard
            .as_ref()
            .ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        drop(handle_guard);
        let read_dir = sftp
            .read_dir(remote_dir.to_string_lossy().to_string())
            .await
            .map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to read remote directory: {}", e))
            })?;
        let mut files = Vec::new();
        for entry in read_dir {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            if options
                .exclude_patterns
                .iter()
                .any(|p| glob::Pattern::new(p).is_ok_and(|pat| pat.matches(&name)))
            {
                continue;
            }
            if !options.include_patterns.is_empty()
                && !options
                    .include_patterns
                    .iter()
                    .any(|p| glob::Pattern::new(p).is_ok_and(|pat| pat.matches(&name)))
            {
                continue;
            }
            let remote_path = remote_dir.join(&name);
            let local_path = local_dir.join(&name);
            let meta = entry.metadata();
            if meta.is_dir() {
                files.extend(
                    Box::pin(self.collect_remote_files(
                        &remote_path,
                        &local_path,
                        options,
                        depth + 1,
                    ))
                    .await?,
                );
            } else if !meta.is_symlink() || options.follow_symlinks {
                files.push((remote_path, local_path));
            }
        }
        Ok(files)
    }

    /// Upload multiple files with batch progress reporting
    pub async fn upload_batch_with_progress(
        &self,
        files: &[(PathBuf, PathBuf)],
        options: Option<TransferOptions>,
        progress: Option<BatchProgressCallback>,
    ) -> ConnectionResult<BatchTransferResult> {
        if files.is_empty() {
            return Ok(BatchTransferResult {
                successful: 0,
                failed: 0,
                results: Vec::new(),
            });
        }
        let options = Arc::new(options.unwrap_or_default());
        let total_files = files.len();
        let total_bytes: u64 = files
            .iter()
            .filter_map(|(local, _)| std::fs::metadata(local).ok())
            .map(|m| m.len())
            .sum();
        let batch_progress = Arc::new(tokio::sync::Mutex::new(BatchTransferProgress::new(
            total_files,
            total_bytes,
        )));
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TRANSFERS));
        debug!(file_count = %total_files, total_bytes = %total_bytes, "Starting batch upload with progress");
        let mut tasks: Vec<tokio::task::JoinHandle<(usize, SingleTransferResult)>> =
            Vec::with_capacity(files.len());
        for (idx, (local, remote)) in files.iter().enumerate() {
            let (sem, local, remote, opts, bp, prog, handle, conn) = (
                semaphore.clone(),
                local.clone(),
                remote.clone(),
                options.clone(),
                batch_progress.clone(),
                progress.clone(),
                self.handle.clone(),
                self.connected.clone(),
            );
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok();
                if !conn.load(Ordering::SeqCst) {
                    return (
                        idx,
                        SingleTransferResult {
                            local_path: local,
                            remote_path: remote,
                            success: false,
                            error: Some(ConnectionError::ConnectionClosed),
                            bytes_transferred: 0,
                        },
                    );
                }
                let size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
                {
                    let mut b = bp.lock().await;
                    b.current_file = Some(TransferProgress::upload(&local, size));
                    if let Some(ref c) = prog {
                        c(&b);
                    }
                }
                let result = Self::upload_single_internal(&handle, &local, &remote, &opts).await;
                let (success, error, bytes) = match result {
                    Ok(b) => (true, None, b),
                    Err(e) => (false, Some(e), 0),
                };
                {
                    let mut b = bp.lock().await;
                    b.completed_files += 1;
                    if success {
                        b.successful_files += 1;
                        b.transferred_bytes += bytes;
                    } else {
                        b.failed_files += 1;
                    }
                    b.current_file = None;
                    if let Some(ref c) = prog {
                        c(&b);
                    }
                }
                (
                    idx,
                    SingleTransferResult {
                        local_path: local,
                        remote_path: remote,
                        success,
                        error,
                        bytes_transferred: bytes,
                    },
                )
            }));
        }
        let results_vec = futures::future::join_all(tasks).await;
        let mut results: Vec<SingleTransferResult> = (0..files.len())
            .map(|_| SingleTransferResult {
                local_path: PathBuf::new(),
                remote_path: PathBuf::new(),
                success: false,
                error: Some(ConnectionError::ExecutionFailed("Task error".to_string())),
                bytes_transferred: 0,
            })
            .collect();
        let (mut successful, mut failed) = (0, 0);
        for r in results_vec {
            if let Ok((idx, res)) = r {
                if res.success {
                    successful += 1;
                } else {
                    failed += 1;
                }
                results[idx] = res;
            } else {
                failed += 1;
            }
        }
        Ok(BatchTransferResult {
            successful,
            failed,
            results,
        })
    }

    async fn upload_single_internal(
        handle: &Arc<RwLock<Option<Handle<ClientHandler>>>>,
        local: &Path,
        remote: &Path,
        opts: &TransferOptions,
    ) -> ConnectionResult<u64> {
        let content = tokio::fs::read(local).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read {}: {}", local.display(), e))
        })?;
        let size = content.len() as u64;
        let guard = handle.read().await;
        let h = guard.as_ref().ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        drop(guard);
        if opts.create_dirs {
            if let Some(p) = remote.parent() {
                Self::create_remote_dirs_sftp(&sftp, p).await?;
            }
        }
        let path_str = remote.to_string_lossy().to_string();
        let mut file = sftp.create(&path_str).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to create {}: {}", remote.display(), e))
        })?;
        file.write_all(&content)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to write: {}", e)))?;
        if let Some(mode) = opts.mode {
            let attrs = russh_sftp::protocol::FileAttributes {
                permissions: Some(mode),
                ..Default::default()
            };
            let _ = sftp.set_metadata(&path_str, attrs).await;
        }
        Ok(size)
    }

    /// Download multiple files with batch progress reporting
    pub async fn download_batch_with_progress(
        &self,
        files: &[(PathBuf, PathBuf)],
        progress: Option<BatchProgressCallback>,
    ) -> ConnectionResult<BatchTransferResult> {
        if files.is_empty() {
            return Ok(BatchTransferResult {
                successful: 0,
                failed: 0,
                results: Vec::new(),
            });
        }
        let total_files = files.len();
        let batch_progress = Arc::new(tokio::sync::Mutex::new(BatchTransferProgress::new(
            total_files,
            0,
        )));
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TRANSFERS));
        debug!(file_count = %total_files, "Starting batch download with progress");
        let mut tasks: Vec<tokio::task::JoinHandle<(usize, SingleTransferResult)>> =
            Vec::with_capacity(files.len());
        for (idx, (remote, local)) in files.iter().enumerate() {
            let (sem, remote, local, bp, prog, handle, conn) = (
                semaphore.clone(),
                remote.clone(),
                local.clone(),
                batch_progress.clone(),
                progress.clone(),
                self.handle.clone(),
                self.connected.clone(),
            );
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok();
                if !conn.load(Ordering::SeqCst) {
                    return (
                        idx,
                        SingleTransferResult {
                            local_path: local,
                            remote_path: remote,
                            success: false,
                            error: Some(ConnectionError::ConnectionClosed),
                            bytes_transferred: 0,
                        },
                    );
                }
                {
                    let mut b = bp.lock().await;
                    b.current_file = Some(TransferProgress::download(&remote, 0));
                    if let Some(ref c) = prog {
                        c(&b);
                    }
                }
                let result = Self::download_single_internal(&handle, &remote, &local).await;
                let (success, error, bytes) = match result {
                    Ok(b) => (true, None, b),
                    Err(e) => (false, Some(e), 0),
                };
                {
                    let mut b = bp.lock().await;
                    b.completed_files += 1;
                    if success {
                        b.successful_files += 1;
                        b.transferred_bytes += bytes;
                    } else {
                        b.failed_files += 1;
                    }
                    b.current_file = None;
                    if let Some(ref c) = prog {
                        c(&b);
                    }
                }
                (
                    idx,
                    SingleTransferResult {
                        local_path: local,
                        remote_path: remote,
                        success,
                        error,
                        bytes_transferred: bytes,
                    },
                )
            }));
        }
        let results_vec = futures::future::join_all(tasks).await;
        let mut results: Vec<SingleTransferResult> = (0..files.len())
            .map(|_| SingleTransferResult {
                local_path: PathBuf::new(),
                remote_path: PathBuf::new(),
                success: false,
                error: Some(ConnectionError::ExecutionFailed("Task error".to_string())),
                bytes_transferred: 0,
            })
            .collect();
        let (mut successful, mut failed) = (0, 0);
        for r in results_vec {
            if let Ok((idx, res)) = r {
                if res.success {
                    successful += 1;
                } else {
                    failed += 1;
                }
                results[idx] = res;
            } else {
                failed += 1;
            }
        }
        Ok(BatchTransferResult {
            successful,
            failed,
            results,
        })
    }

    async fn download_single_internal(
        handle: &Arc<RwLock<Option<Handle<ClientHandler>>>>,
        remote: &Path,
        local: &Path,
    ) -> ConnectionResult<u64> {
        let guard = handle.read().await;
        let h = guard.as_ref().ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        drop(guard);
        let path_str = remote.to_string_lossy().to_string();
        let mut file = sftp.open(&path_str).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to open {}: {}", remote.display(), e))
        })?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to read: {}", e)))?;
        let size = content.len() as u64;
        if let Some(p) = local.parent() {
            tokio::fs::create_dir_all(p).await.map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create dir: {}", e))
            })?;
        }
        tokio::fs::write(local, &content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write {}: {}", local.display(), e))
        })?;
        Ok(size)
    }

    /// Upload a file with progress callback
    pub async fn upload_with_progress(
        &self,
        local: &Path,
        remote: &Path,
        options: Option<TransferOptions>,
        progress: ProgressCallback,
    ) -> ConnectionResult<()> {
        let opts = options.unwrap_or_default();
        let size = tokio::fs::metadata(local)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to get metadata: {}", e)))?
            .len();
        let mut prog = TransferProgress::upload(local, size);
        progress(&prog);
        if size < STREAM_THRESHOLD {
            self.upload(local, remote, Some(opts)).await?;
            prog.phase = TransferPhase::Completed;
            prog.transferred_bytes = size;
            progress(&prog);
            return Ok(());
        }
        let guard = self.handle.read().await;
        let h = guard.as_ref().ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        drop(guard);
        if opts.create_dirs {
            if let Some(p) = remote.parent() {
                Self::create_remote_dirs_sftp(&sftp, p).await?;
            }
        }
        let mut local_file = tokio::fs::File::open(local)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to open: {}", e)))?;
        let path_str = remote.to_string_lossy().to_string();
        let mut remote_file = sftp
            .create(&path_str)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to create: {}", e)))?;
        prog.phase = TransferPhase::Transferring;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut written = 0u64;
        loop {
            let n = local_file
                .read(&mut buf)
                .await
                .map_err(|e| ConnectionError::TransferFailed(format!("Read error: {}", e)))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .await
                .map_err(|e| ConnectionError::TransferFailed(format!("Write error: {}", e)))?;
            written += n as u64;
            prog.update(written);
            progress(&prog);
        }
        drop(remote_file);
        prog.phase = TransferPhase::Finalizing;
        progress(&prog);
        if let Some(mode) = opts.mode {
            let attrs = russh_sftp::protocol::FileAttributes {
                permissions: Some(mode),
                ..Default::default()
            };
            let _ = sftp.set_metadata(&path_str, attrs).await;
        }
        if opts.owner.is_some() || opts.group.is_some() {
            let og = match (&opts.owner, &opts.group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                _ => String::new(),
            };
            if !og.is_empty() {
                let _ = self
                    .execute(
                        &format!(
                            "chown {} {}",
                            og,
                            escape_shell_arg(&remote.to_string_lossy())
                        ),
                        None,
                    )
                    .await;
            }
        }
        prog.phase = TransferPhase::Completed;
        progress(&prog);
        Ok(())
    }

    /// Download a file with progress callback
    pub async fn download_with_progress(
        &self,
        remote: &Path,
        local: &Path,
        progress: ProgressCallback,
    ) -> ConnectionResult<()> {
        let guard = self.handle.read().await;
        let h = guard.as_ref().ok_or(ConnectionError::ConnectionClosed)?;
        let sftp = Self::open_sftp(h).await?;
        drop(guard);
        let path_str = remote.to_string_lossy().to_string();
        let attrs = sftp.metadata(&path_str).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to get metadata: {}", e))
        })?;
        let size = attrs.size.unwrap_or(0);
        let mut prog = TransferProgress::download(remote, size);
        progress(&prog);
        let mut remote_file = sftp
            .open(&path_str)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to open: {}", e)))?;
        if let Some(p) = local.parent() {
            tokio::fs::create_dir_all(p).await.map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create dir: {}", e))
            })?;
        }
        let mut local_file = tokio::fs::File::create(local)
            .await
            .map_err(|e| ConnectionError::TransferFailed(format!("Failed to create: {}", e)))?;
        prog.phase = TransferPhase::Transferring;
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut read_total = 0u64;
        loop {
            let n = remote_file
                .read(&mut buf)
                .await
                .map_err(|e| ConnectionError::TransferFailed(format!("Read error: {}", e)))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&buf[..n])
                .await
                .map_err(|e| ConnectionError::TransferFailed(format!("Write error: {}", e)))?;
            read_total += n as u64;
            prog.update(read_total);
            progress(&prog);
        }
        prog.phase = TransferPhase::Completed;
        progress(&prog);
        Ok(())
    }
}

// ============================================================================
// High-Performance Connection Factory
// ============================================================================

/// High-performance connection factory for parallel SSH connections
///
/// This factory provides optimized connection establishment with:
/// - Parallel connection to multiple hosts
/// - Connection pre-warming
/// - Automatic keepalive management
/// - Connection health monitoring
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::connection::{ConnectionConfig, HighPerformanceConnectionFactory};
///
/// let factory = HighPerformanceConnectionFactory::new();
///
/// // Connect to multiple hosts in parallel
/// let hosts = vec![
///     ("host1.example.com", 22, "user"),
///     ("host2.example.com", 22, "user"),
///     ("host3.example.com", 22, "user"),
/// ];
///
/// let config = ConnectionConfig::default();
/// let connections = factory.connect_parallel(&hosts, &config).await;
/// # Ok(())
/// # }
/// ```
pub struct HighPerformanceConnectionFactory {
    /// Maximum concurrent connection attempts
    max_concurrent_connects: usize,
    /// Whether to pre-warm connections after establishing
    pre_warm: bool,
    /// Connection timeout override (uses config default if None)
    timeout_override: Option<Duration>,
}

impl Default for HighPerformanceConnectionFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl HighPerformanceConnectionFactory {
    /// Create a new high-performance connection factory
    pub fn new() -> Self {
        Self {
            max_concurrent_connects: 10,
            pre_warm: true,
            timeout_override: None,
        }
    }

    /// Set maximum concurrent connection attempts
    pub fn max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent_connects = max.max(1);
        self
    }

    /// Enable or disable connection pre-warming
    pub fn pre_warm(mut self, enable: bool) -> Self {
        self.pre_warm = enable;
        self
    }

    /// Set connection timeout override
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout_override = Some(timeout);
        self
    }

    /// Connect to a single host with optimizations
    pub async fn connect(
        &self,
        host: &str,
        port: u16,
        user: &str,
        config: &ConnectionConfig,
    ) -> ConnectionResult<RusshConnection> {
        let start = Instant::now();

        let conn = RusshConnection::connect(host, port, user, None, config).await?;

        // Pre-warm the connection if enabled
        if self.pre_warm {
            if let Err(e) = conn.warm_up().await {
                warn!(host = %host, error = %e, "Failed to pre-warm connection");
            }
        }

        info!(
            host = %host,
            elapsed_ms = %start.elapsed().as_millis(),
            "Connection established"
        );

        Ok(conn)
    }

    /// Connect to multiple hosts in parallel
    ///
    /// Returns a vector of results in the same order as the input hosts.
    /// Failed connections are represented as errors in the result vector.
    pub async fn connect_parallel(
        &self,
        hosts: &[(&str, u16, &str)], // (host, port, user)
        config: &ConnectionConfig,
    ) -> Vec<ConnectionResult<RusshConnection>> {
        if hosts.is_empty() {
            return Vec::new();
        }

        let start = Instant::now();
        info!(host_count = %hosts.len(), "Starting parallel connection establishment");

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent_connects));
        let pre_warm = self.pre_warm;

        let futures: Vec<_> = hosts
            .iter()
            .enumerate()
            .map(|(idx, (host, port, user))| {
                let sem = semaphore.clone();
                let host = host.to_string();
                let user = user.to_string();
                let port = *port;
                let config = config.clone();

                async move {
                    let _permit = sem.acquire().await.map_err(|_| {
                        ConnectionError::ConnectionFailed("Semaphore closed".to_string())
                    })?;

                    let conn_start = Instant::now();
                    let conn = RusshConnection::connect(&host, port, &user, None, &config).await?;

                    if pre_warm {
                        let _ = conn.warm_up().await;
                    }

                    debug!(
                        host = %host,
                        elapsed_ms = %conn_start.elapsed().as_millis(),
                        "Parallel connection established"
                    );

                    Ok::<(usize, RusshConnection), ConnectionError>((idx, conn))
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        // Reconstruct results in order
        let mut ordered: Vec<ConnectionResult<RusshConnection>> = (0..hosts.len())
            .map(|_| {
                Err(ConnectionError::ConnectionFailed(
                    "Not connected".to_string(),
                ))
            })
            .collect();

        for result in results {
            match result {
                Ok((idx, conn)) => {
                    ordered[idx] = Ok(conn);
                }
                Err(e) => {
                    // Error already logged
                    warn!(error = %e, "Parallel connection failed");
                }
            }
        }

        info!(
            total_hosts = %hosts.len(),
            successful = %ordered.iter().filter(|r| r.is_ok()).count(),
            elapsed_ms = %start.elapsed().as_millis(),
            "Parallel connection establishment completed"
        );

        ordered
    }

    /// Connect and warm up connections for a list of host configs
    pub async fn connect_with_configs(
        &self,
        host_configs: &[(&str, HostConfig)],
        global_config: &ConnectionConfig,
    ) -> Vec<ConnectionResult<RusshConnection>> {
        if host_configs.is_empty() {
            return Vec::new();
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent_connects));
        let pre_warm = self.pre_warm;

        let futures: Vec<_> = host_configs
            .iter()
            .enumerate()
            .map(|(idx, (host, host_config))| {
                let sem = semaphore.clone();
                let host = host.to_string();
                let host_config = host_config.clone();
                let global_config = global_config.clone();

                async move {
                    let _permit = sem.acquire().await.map_err(|_| {
                        ConnectionError::ConnectionFailed("Semaphore closed".to_string())
                    })?;

                    let port = host_config.port.unwrap_or(22);
                    let user = host_config
                        .user
                        .clone()
                        .unwrap_or_else(|| global_config.defaults.user.clone());

                    let conn = RusshConnection::connect(
                        &host,
                        port,
                        &user,
                        Some(host_config),
                        &global_config,
                    )
                    .await?;

                    if pre_warm {
                        let _ = conn.warm_up().await;
                    }

                    Ok::<(usize, RusshConnection), ConnectionError>((idx, conn))
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        // Reconstruct results in order
        let mut ordered: Vec<ConnectionResult<RusshConnection>> = (0..host_configs.len())
            .map(|_| {
                Err(ConnectionError::ConnectionFailed(
                    "Not connected".to_string(),
                ))
            })
            .collect();

        for result in results {
            match result {
                Ok((idx, conn)) => {
                    ordered[idx] = Ok(conn);
                }
                Err(e) => {
                    warn!(error = %e, "Connection with config failed");
                }
            }
        }

        ordered
    }
}

/// Helper struct for managing a group of connections with keepalive
pub struct ConnectionGroup {
    connections: Vec<RusshConnection>,
    keepalive_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ConnectionGroup {
    /// Create a new connection group from established connections
    pub fn new(connections: Vec<RusshConnection>) -> Self {
        Self {
            connections,
            keepalive_handle: None,
        }
    }

    /// Start background keepalive task for all connections
    ///
    /// This spawns a background task that periodically sends keepalive
    /// messages to all connections in the group.
    pub fn start_keepalive(&mut self, interval: Duration) {
        if self.keepalive_handle.is_some() {
            return; // Already running
        }

        let connections: Vec<_> = self
            .connections
            .iter()
            .map(|c| (c.handle.clone(), c.identifier.clone()))
            .collect();

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                for (handle, identifier) in &connections {
                    let handle_guard = handle.read().await;
                    if let Some(h) = handle_guard.as_ref() {
                        match h.channel_open_session().await {
                            Ok(channel) => {
                                let _ = channel.exec(true, "true").await;
                                let _ = channel.eof().await;
                                trace!(identifier = %identifier, "Group keepalive sent");
                            }
                            Err(e) => {
                                warn!(
                                    identifier = %identifier,
                                    error = %e,
                                    "Group keepalive failed"
                                );
                            }
                        }
                    }
                }
            }
        });

        self.keepalive_handle = Some(handle);
    }

    /// Stop the background keepalive task
    pub fn stop_keepalive(&mut self) {
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
        }
    }

    /// Get a reference to the connections
    pub fn connections(&self) -> &[RusshConnection] {
        &self.connections
    }

    /// Get a mutable reference to the connections
    pub fn connections_mut(&mut self) -> &mut [RusshConnection] {
        &mut self.connections
    }

    /// Take ownership of the connections
    pub fn into_connections(mut self) -> Vec<RusshConnection> {
        self.stop_keepalive();
        std::mem::take(&mut self.connections)
    }

    /// Get the number of connections in the group
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Check if the group is empty
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Get metrics for all connections
    pub fn metrics(&self) -> Vec<ConnectionMetrics> {
        self.connections.iter().map(|c| c.metrics()).collect()
    }
}

impl Drop for ConnectionGroup {
    fn drop(&mut self) {
        self.stop_keepalive();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_russh_connection_builder() {
        let builder = RusshConnectionBuilder::new("example.com")
            .port(2222)
            .user("admin")
            .compression(true);

        assert_eq!(builder.host, "example.com");
        assert_eq!(builder.port, 2222);
        assert_eq!(builder.user, "admin");
        assert!(builder.compression);
    }

    #[test]
    fn test_build_command_basic() {
        let options = ExecuteOptions::default();
        let cmd = RusshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_build_command_with_cwd() {
        let options = ExecuteOptions::new().with_cwd("/tmp");
        let cmd = RusshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "cd /tmp && echo hello");
    }

    #[test]
    fn test_build_command_with_escalation() {
        let options = ExecuteOptions::new().with_escalation(Some("admin".to_string()));
        let cmd = RusshConnection::build_command("echo hello", &options).unwrap();
        assert_eq!(cmd, "sudo -u admin -- echo hello");
    }

    #[test]
    fn test_build_command_with_cwd_and_escalation() {
        let options = ExecuteOptions::new()
            .with_cwd("/var/log")
            .with_escalation(None);
        let cmd = RusshConnection::build_command("cat syslog", &options).unwrap();
        assert_eq!(cmd, "cd /var/log && sudo -u root -- cat syslog");
    }

    #[test]
    fn test_build_command_rejects_invalid_user() {
        let options = ExecuteOptions::new().with_escalation(Some("root; rm -rf /".to_string()));
        let result = RusshConnection::build_command("echo hello", &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_shell_arg_simple() {
        assert_eq!(escape_shell_arg("hello"), "'hello'");
    }

    #[test]
    fn test_escape_shell_arg_with_spaces() {
        assert_eq!(
            escape_shell_arg("/path/with spaces/file.txt"),
            "'/path/with spaces/file.txt'"
        );
    }

    #[test]
    fn test_escape_shell_arg_with_quotes() {
        assert_eq!(escape_shell_arg("it's a test"), "'it'\\''s a test'");
    }

    #[test]
    fn test_escape_shell_arg_with_special_chars() {
        assert_eq!(escape_shell_arg("test$var`cmd`"), "'test$var`cmd`'");
    }

    #[test]
    fn test_max_concurrent_channels_constant() {
        // Ensure we have a reasonable limit on concurrent channels
        assert_eq!(MAX_CONCURRENT_CHANNELS, 10);
    }

    #[test]
    fn test_pending_command_new() {
        let cmd = PendingCommand::new("echo hello", None);
        assert_eq!(cmd.command(), "echo hello");
        assert_eq!(cmd.options().cwd, None);
        assert!(!cmd.options().escalate);
    }

    #[test]
    fn test_pending_command_with_options() {
        let options = ExecuteOptions::new().with_cwd("/tmp").with_timeout(30);
        let cmd = PendingCommand::new("echo hello", Some(options));
        assert_eq!(cmd.command(), "echo hello");
        assert_eq!(cmd.options().cwd, Some("/tmp".to_string()));
        assert_eq!(cmd.options().timeout, Some(30));
    }
}
#[cfg(test)]
mod verification_tests {
    use super::*;
    use russh::keys::PrivateKey;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper to generate a dummy Ed25519 key (russh 0.54+ API)
    fn generate_key() -> PrivateKey {
        use rand::SeedableRng;
        // Use a seeded RNG for reproducibility in tests
        let mut rng = rand::rngs::StdRng::from_entropy();
        PrivateKey::random(&mut rng, Algorithm::Ed25519).expect("Failed to generate key")
    }

    #[tokio::test]
    async fn test_verify_host_key_verified() {
        let private_key = generate_key();
        // In russh 0.54+, public_key() returns a reference
        let public_key = private_key.public_key().clone();

        // Setup known_hosts file
        let mut temp_file = NamedTempFile::new().unwrap();

        // Format entry
        let key_type = public_key.algorithm().to_string();
        let key_base64 = public_key.public_key_base64();
        let entry = format!("example.com {} {}\n", key_type, key_base64);

        temp_file.write_all(entry.as_bytes()).unwrap();

        let path = temp_file.path().to_path_buf();
        let mut handler = ClientHandler::new("example.com", 22, false, Some(path));

        let result = handler.check_server_key(&public_key).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_verify_host_key_mismatch() {
        let private_key1 = generate_key();
        let private_key2 = generate_key();

        let public_key1 = private_key1.public_key().clone();
        let public_key2 = private_key2.public_key().clone();

        // Write key1 to known_hosts
        let mut temp_file = NamedTempFile::new().unwrap();
        let key_type = public_key1.algorithm().to_string();
        let key_base64 = public_key1.public_key_base64();
        let entry = format!("example.com {} {}\n", key_type, key_base64);
        temp_file.write_all(entry.as_bytes()).unwrap();

        let path = temp_file.path().to_path_buf();

        // Check with key2 (mismatch)
        let mut handler = ClientHandler::new("example.com", 22, false, Some(path));
        let result = handler.check_server_key(&public_key2).await;

        // Should return false (reject) because of mismatch, even if accept_unknown is true (mismatch != unknown)
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_unknown_host_accept() {
        let private_key = generate_key();
        let public_key = private_key.public_key().clone();

        // Empty known_hosts
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // accept_unknown = true
        let mut handler = ClientHandler::new("example.com", 22, true, Some(path.clone()));

        let result = handler.check_server_key(&public_key).await;
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify it was written to file
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("example.com"));
        assert!(content.contains(&public_key.algorithm().to_string()));
    }

    #[tokio::test]
    async fn test_unknown_host_reject() {
        let private_key = generate_key();
        let public_key = private_key.public_key().clone();

        // Empty known_hosts
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // accept_unknown = false
        let mut handler = ClientHandler::new("example.com", 22, false, Some(path.clone()));

        let result = handler.check_server_key(&public_key).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());

        // Verify it was NOT written to file
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.is_empty());
    }
}
