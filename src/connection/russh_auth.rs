//! Russh authentication module
//!
//! This module provides authentication support for the russh SSH client library.
//! It implements the `russh::client::Handler` trait and provides utilities for:
//! - Password authentication
//! - Public key authentication (from file)
//! - SSH agent authentication
//! - Keyboard-interactive authentication
//!
//! ## Key Loading
//!
//! The module supports loading private keys in multiple formats:
//! - **OpenSSH format**: The modern format starting with `-----BEGIN OPENSSH PRIVATE KEY-----`
//! - **PEM format**: Traditional RSA keys starting with `-----BEGIN RSA PRIVATE KEY-----`
//! - **PKCS#8 format**: Standard format `-----BEGIN PRIVATE KEY-----` or encrypted variant
//!
//! ## Supported Key Types
//!
//! - **Ed25519**: Modern, fast, and secure. Recommended for new keys.
//! - **RSA**: Legacy but widely supported. Use at least 2048 bits.
//! - **ECDSA**: Elliptic curve keys (P-256, P-384, P-521 curves).
//!
//! ## Encrypted Keys
//!
//! Encrypted private keys are supported with passphrase decryption:
//! - OpenSSH encrypted format (bcrypt-pbkdf + aes256-ctr)
//! - PEM encrypted format (Proc-Type: 4,ENCRYPTED)
//! - PKCS#8 encrypted format
//!
//! The authentication system supports multiple fallback methods and works
//! asynchronously with the russh library.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

use russh::client::{self, Handle, Handler, KeyboardInteractiveAuthResponse, Session};
use russh::keys::agent::client::AgentClient;
use russh::keys::{
    self, check_known_hosts_path, Algorithm, EcdsaCurve, PrivateKey, PrivateKeyWithHashAlg,
    PublicKey,
};
use russh::ChannelId;

use super::config::{expand_path, HostConfig};
use super::ConnectionError;

/// Alias to preserve the previous KeyPair naming in public APIs.
pub type KeyPair = PrivateKey;

// ============================================================================
// Key Types and Detection
// ============================================================================

/// Supported SSH key types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// Ed25519 key (recommended, modern, fast)
    Ed25519,
    /// RSA key (legacy, widely supported)
    Rsa,
    /// ECDSA with NIST P-256 curve
    EcdsaP256,
    /// ECDSA with NIST P-384 curve
    EcdsaP384,
    /// ECDSA with NIST P-521 curve
    EcdsaP521,
}

impl KeyType {
    /// Get the SSH algorithm identifier for this key type
    pub fn algorithm_name(&self) -> &'static str {
        match self {
            KeyType::Ed25519 => "ssh-ed25519",
            KeyType::Rsa => "ssh-rsa",
            KeyType::EcdsaP256 => "ecdsa-sha2-nistp256",
            KeyType::EcdsaP384 => "ecdsa-sha2-nistp384",
            KeyType::EcdsaP521 => "ecdsa-sha2-nistp521",
        }
    }

    /// Get the default filename for this key type
    pub fn default_filename(&self) -> &'static str {
        match self {
            KeyType::Ed25519 => "id_ed25519",
            KeyType::Rsa => "id_rsa",
            KeyType::EcdsaP256 | KeyType::EcdsaP384 | KeyType::EcdsaP521 => "id_ecdsa",
        }
    }

    /// Detect key type from PEM/OpenSSH header content
    ///
    /// Returns `None` if the key type cannot be determined from the header alone.
    pub fn detect_from_content(content: &str) -> Option<Self> {
        // Check for specific PEM format markers
        if content.contains("-----BEGIN RSA PRIVATE KEY-----") {
            return Some(KeyType::Rsa);
        }
        if content.contains("-----BEGIN EC PRIVATE KEY-----") {
            // Could be any ECDSA curve, need further parsing
            return None;
        }

        // OpenSSH and PKCS#8 formats require parsing to determine algorithm
        None
    }

    /// Get key type from a loaded key pair
    pub fn from_key_pair(key: &KeyPair) -> Option<Self> {
        match key.algorithm() {
            Algorithm::Ed25519 | Algorithm::SkEd25519 => Some(KeyType::Ed25519),
            Algorithm::Rsa { .. } => Some(KeyType::Rsa),
            Algorithm::Ecdsa { curve } => match curve {
                EcdsaCurve::NistP256 => Some(KeyType::EcdsaP256),
                EcdsaCurve::NistP384 => Some(KeyType::EcdsaP384),
                EcdsaCurve::NistP521 => Some(KeyType::EcdsaP521),
            },
            Algorithm::SkEcdsaSha2NistP256 => Some(KeyType::EcdsaP256),
            _ => None,
        }
    }
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.algorithm_name())
    }
}

// ============================================================================
// Key Loading Errors
// ============================================================================

/// Errors specific to SSH key operations
#[derive(Error, Debug)]
pub enum KeyError {
    /// Key file not found
    #[error("Key file not found: {0}")]
    NotFound(PathBuf),

    /// Failed to read key file
    #[error("Failed to read key file {path}: {source}")]
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Key decoding failed (wrong format or corrupted)
    #[error("Failed to decode key {path}: {message}")]
    DecodeError { path: PathBuf, message: String },

    /// Passphrase required but not provided
    #[error("Key {0} is encrypted - passphrase required")]
    PassphraseRequired(PathBuf),

    /// Wrong passphrase for encrypted key
    #[error("Wrong passphrase for key {0}")]
    WrongPassphrase(PathBuf),

    /// Unsupported key type
    #[error("Unsupported key type: {0}")]
    UnsupportedKeyType(String),

    /// Key loading error from russh-keys
    #[error("Key loading error: {0}")]
    LoadError(String),

    /// No valid keys found in search paths
    #[error("No valid SSH key found in search paths")]
    NoKeysFound,
}

impl From<KeyError> for ConnectionError {
    fn from(err: KeyError) -> Self {
        ConnectionError::AuthenticationFailed(err.to_string())
    }
}

// ============================================================================
// Key Loading Utilities
// ============================================================================

/// Information about a loaded key
#[derive(Debug, Clone)]
pub struct KeyInfo {
    /// Path the key was loaded from
    pub path: PathBuf,
    /// Type of the key
    pub key_type: Option<KeyType>,
    /// Whether the key was encrypted
    pub was_encrypted: bool,
    /// Comment from the key file (if present)
    pub comment: Option<String>,
}

/// SSH private key loader with support for multiple formats and encryption
///
/// # Example
///
/// ```no_run
/// use rustible::connection::russh_auth::KeyLoader;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create a key loader with custom settings
/// let loader = KeyLoader::new()
///     .with_passphrase("my_secret_passphrase")
///     .with_key_path("/home/user/.ssh/id_ed25519");
///
/// // Load the first available key
/// let (key, info) = loader.find_and_load_key()?;
/// println!("Loaded key from: {:?}", info.path);
/// # Ok(())
/// # }
/// ```
pub struct KeyLoader {
    /// Paths to search for keys
    search_paths: Vec<PathBuf>,
    /// Passphrase for encrypted keys
    passphrase: Option<String>,
    /// Whether to try SSH agent
    use_agent: bool,
}

impl Default for KeyLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyLoader {
    /// Create a new key loader with default search paths
    ///
    /// Default search paths include:
    /// - `~/.ssh/id_ed25519`
    /// - `~/.ssh/id_ecdsa`
    /// - `~/.ssh/id_rsa`
    /// - `~/.ssh/id_dsa`
    pub fn new() -> Self {
        Self {
            search_paths: standard_key_locations(),
            passphrase: None,
            use_agent: true,
        }
    }

    /// Create a key loader from host configuration
    pub fn from_host_config(host_config: &HostConfig) -> Self {
        let mut loader = Self::new();

        // Add identity file from config if specified (at the front for priority)
        if let Some(identity_file) = &host_config.identity_file {
            let path = expand_path(identity_file);
            loader.search_paths.insert(0, path);
        }

        // Use password as passphrase if provided (common pattern)
        if let Some(password) = &host_config.password {
            loader.passphrase = Some(password.clone());
        }

        loader
    }

    /// Set the passphrase for encrypted keys
    pub fn with_passphrase(mut self, passphrase: impl Into<String>) -> Self {
        self.passphrase = Some(passphrase.into());
        self
    }

    /// Add a path to search for keys (will be searched first)
    pub fn with_key_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if !self.search_paths.contains(&path) {
            self.search_paths.insert(0, path);
        }
        self
    }

    /// Add multiple paths to search for keys
    pub fn with_key_paths(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        for path in paths {
            if !self.search_paths.contains(&path) {
                self.search_paths.push(path);
            }
        }
        self
    }

    /// Set whether to try SSH agent
    pub fn with_agent(mut self, use_agent: bool) -> Self {
        self.use_agent = use_agent;
        self
    }

    /// Get the configured search paths
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Check if a passphrase is configured
    pub fn has_passphrase(&self) -> bool {
        self.passphrase.is_some()
    }

    /// Load a private key from a specific path
    ///
    /// This function:
    /// 1. Reads the key file
    /// 2. Detects if the key is encrypted
    /// 3. Decrypts using the configured passphrase if needed
    /// 4. Returns the loaded key
    pub fn load_key(&self, path: &Path) -> Result<KeyPair, KeyError> {
        if !path.exists() {
            return Err(KeyError::NotFound(path.to_path_buf()));
        }

        debug!(path = %path.display(), "Loading SSH private key");

        // Try loading with passphrase first if we have one
        let result = if let Some(passphrase) = &self.passphrase {
            keys::load_secret_key(path, Some(passphrase)).or_else(|e| {
                let error_msg = e.to_string().to_lowercase();
                // If passphrase was wrong, try without (maybe key isn't encrypted)
                if error_msg.contains("decrypt") || error_msg.contains("mac") {
                    keys::load_secret_key(path, None)
                } else {
                    Err(e)
                }
            })
        } else {
            // Try without passphrase first
            keys::load_secret_key(path, None)
        };

        result.map_err(|e| {
            let error_msg = e.to_string().to_lowercase();
            if error_msg.contains("encrypt")
                || error_msg.contains("passphrase")
                || error_msg.contains("password")
                || error_msg.contains("decrypt")
            {
                if self.passphrase.is_some() {
                    KeyError::WrongPassphrase(path.to_path_buf())
                } else {
                    KeyError::PassphraseRequired(path.to_path_buf())
                }
            } else {
                KeyError::DecodeError {
                    path: path.to_path_buf(),
                    message: e.to_string(),
                }
            }
        })
    }

    /// Load a key and return additional info about it
    pub fn load_key_with_info(&self, path: &Path) -> Result<(KeyPair, KeyInfo), KeyError> {
        let key = self.load_key(path)?;

        let key_type = KeyType::from_key_pair(&key);
        let was_encrypted = self.passphrase.is_some();

        let comment = key.comment();
        let comment = if comment.is_empty() {
            None
        } else {
            Some(comment.to_string())
        };

        let info = KeyInfo {
            path: path.to_path_buf(),
            key_type,
            was_encrypted,
            comment,
        };

        Ok((key, info))
    }

    /// Find and load the first available key from search paths
    pub fn find_and_load_key(&self) -> Result<(KeyPair, KeyInfo), KeyError> {
        let mut last_error = None;

        for path in &self.search_paths {
            if !path.exists() {
                trace!(path = %path.display(), "Key file does not exist, skipping");
                continue;
            }

            match self.load_key_with_info(path) {
                Ok((key, info)) => {
                    debug!(
                        path = %path.display(),
                        key_type = ?info.key_type,
                        "Successfully loaded SSH key"
                    );
                    return Ok((key, info));
                }
                Err(e) => {
                    trace!(path = %path.display(), error = %e, "Failed to load key");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(KeyError::NoKeysFound))
    }

    /// Load all available keys from search paths
    pub fn load_all_keys(&self) -> Vec<(KeyPair, KeyInfo)> {
        let mut keys = Vec::new();

        for path in &self.search_paths {
            if !path.exists() {
                continue;
            }

            match self.load_key_with_info(path) {
                Ok(result) => {
                    debug!(path = %path.display(), "Loaded key");
                    keys.push(result);
                }
                Err(e) => {
                    trace!(path = %path.display(), error = %e, "Skipping key");
                }
            }
        }

        keys
    }
}

/// Get the standard SSH key locations for the current user
///
/// Returns paths to common key files in `~/.ssh/` directory,
/// in order of preference (Ed25519 > ECDSA > RSA > DSA).
///
/// Only returns paths to files that actually exist.
pub fn standard_key_locations() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");

        // Standard key files in order of preference
        // Ed25519 is preferred for security and performance
        let potential_paths = [
            ssh_dir.join("id_ed25519"),
            ssh_dir.join("id_ecdsa"),
            ssh_dir.join("id_ecdsa_sk"),   // Security key
            ssh_dir.join("id_ed25519_sk"), // Security key
            ssh_dir.join("id_rsa"),
            ssh_dir.join("id_dsa"), // Deprecated, but still supported
        ];

        for path in potential_paths {
            if path.exists() {
                paths.push(path);
            }
        }
    }

    paths
}

/// Check if a key file is encrypted by reading its header
///
/// Returns `Ok(true)` if the key appears to be encrypted,
/// `Ok(false)` if it appears unencrypted,
/// or an error if the file cannot be read.
pub fn is_key_encrypted(path: &Path) -> Result<bool, KeyError> {
    let content = std::fs::read_to_string(path).map_err(|e| KeyError::ReadError {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(detect_encryption_from_content(&content))
}

/// Detect if key content indicates encryption
fn detect_encryption_from_content(content: &str) -> bool {
    // Check for explicit ENCRYPTED marker
    if content.contains("ENCRYPTED") {
        return true;
    }

    // Check for Proc-Type header (OpenSSL encrypted format)
    if content.contains("Proc-Type: 4,ENCRYPTED") {
        return true;
    }

    // Check for DEK-Info header (encryption info)
    if content.contains("DEK-Info:") {
        return true;
    }

    false
}

/// Authentication methods supported by the russh connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethod {
    /// No authentication (for testing or special cases)
    None,
    /// Password-based authentication
    Password(String),
    /// Public key authentication from file
    PublicKey {
        /// Path to the private key file
        key_path: String,
        /// Optional passphrase for encrypted keys
        passphrase: Option<String>,
    },
    /// SSH agent authentication
    Agent,
    /// Keyboard-interactive authentication
    KeyboardInteractive {
        /// Responses to provide for prompts
        responses: Vec<String>,
    },
}

/// Configuration for authentication attempts
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Username for authentication
    pub username: String,
    /// Ordered list of authentication methods to try
    pub methods: Vec<AuthMethod>,
    /// Whether to accept unknown host keys (not recommended for production)
    pub accept_unknown_hosts: bool,
    /// Path to known_hosts file for host key verification
    pub known_hosts_file: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            username: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string()),
            methods: vec![AuthMethod::Agent],
            accept_unknown_hosts: false,
            known_hosts_file: None,
        }
    }
}

impl AuthConfig {
    /// Create a new authentication configuration
    pub fn new(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            ..Default::default()
        }
    }

    /// Build authentication config from HostConfig
    pub fn from_host_config(host_config: &HostConfig, use_agent: bool) -> Self {
        let username = host_config
            .user
            .clone()
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "root".to_string()));

        let mut methods = Vec::new();

        // Add agent auth first if enabled
        if use_agent {
            methods.push(AuthMethod::Agent);
        }

        // Add public key auth if identity file is specified
        if let Some(ref identity_file) = host_config.identity_file {
            methods.push(AuthMethod::PublicKey {
                key_path: identity_file.clone(),
                passphrase: host_config.password.clone(),
            });
        }

        // Add password auth if password is specified
        if let Some(ref password) = host_config.password {
            methods.push(AuthMethod::Password(password.clone()));
        }

        // Default: try agent if no methods specified
        if methods.is_empty() {
            methods.push(AuthMethod::Agent);
        }

        let accept_unknown_hosts = host_config
            .strict_host_key_checking
            .map(|strict| !strict)
            .unwrap_or(false);

        Self {
            username,
            methods,
            accept_unknown_hosts,
            known_hosts_file: host_config.user_known_hosts_file.clone(),
        }
    }

    /// Add a password authentication method
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.methods.push(AuthMethod::Password(password.into()));
        self
    }

    /// Add a public key authentication method
    pub fn with_public_key(
        mut self,
        key_path: impl Into<String>,
        passphrase: Option<String>,
    ) -> Self {
        self.methods.push(AuthMethod::PublicKey {
            key_path: key_path.into(),
            passphrase,
        });
        self
    }

    /// Add SSH agent authentication
    pub fn with_agent(mut self) -> Self {
        if !self
            .methods
            .iter()
            .any(|method| matches!(method, AuthMethod::Agent))
        {
            self.methods.push(AuthMethod::Agent);
        }
        self
    }

    /// Add keyboard-interactive authentication
    pub fn with_keyboard_interactive(mut self, responses: Vec<String>) -> Self {
        self.methods
            .push(AuthMethod::KeyboardInteractive { responses });
        self
    }

    /// Set whether to accept unknown host keys
    pub fn accept_unknown_hosts(mut self, accept: bool) -> Self {
        self.accept_unknown_hosts = accept;
        self
    }
}

/// Result of an authentication attempt
#[derive(Debug)]
pub enum AuthResult {
    /// Authentication succeeded
    Success,
    /// Authentication failed but can try another method
    Failure,
    /// Authentication partially succeeded (e.g., need more methods)
    Partial,
    /// Server disconnected during authentication
    Disconnected,
}

fn map_auth_result(result: client::AuthResult) -> AuthResult {
    match result {
        client::AuthResult::Success => AuthResult::Success,
        client::AuthResult::Failure {
            partial_success, ..
        } => map_partial_success(partial_success),
    }
}

fn map_partial_success(partial_success: bool) -> AuthResult {
    if partial_success {
        AuthResult::Partial
    } else {
        AuthResult::Failure
    }
}

/// Russh client handler implementation
///
/// This struct implements the `russh::client::Handler` trait and manages
/// the client-side SSH protocol handling, including server key verification
/// and handling of unsolicited server messages.
pub struct RusshClientHandler {
    /// Authentication configuration
    auth_config: AuthConfig,
    /// Server public key (populated after key exchange)
    server_key: Arc<Mutex<Option<PublicKey>>>,
    /// Whether the server key has been verified
    key_verified: Arc<Mutex<bool>>,
    /// The hostname we're connecting to
    host: String,
    /// The port we're connecting to
    port: u16,
}

impl RusshClientHandler {
    /// Create a new client handler
    pub fn new(auth_config: AuthConfig, host: String, port: u16) -> Self {
        Self {
            auth_config,
            server_key: Arc::new(Mutex::new(None)),
            key_verified: Arc::new(Mutex::new(false)),
            host,
            port,
        }
    }

    /// Get the authentication configuration
    pub fn auth_config(&self) -> &AuthConfig {
        &self.auth_config
    }

    /// Check if the server key has been verified
    pub async fn is_key_verified(&self) -> bool {
        *self.key_verified.lock().await
    }

    /// Get the server's public key (if available)
    pub async fn server_key(&self) -> Option<PublicKey> {
        self.server_key.lock().await.clone()
    }
}

impl Handler for RusshClientHandler {
    type Error = anyhow::Error;

    /// Called when the server presents its authentication banner
    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        debug!("Server authentication banner: {}", banner.trim());
        Ok(())
    }

    /// Verify the server's public key
    ///
    /// This is a critical security function. In production, you should:
    /// 1. Check the key against known_hosts
    /// 2. Prompt the user for unknown keys
    /// 3. Never blindly accept all keys
    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        trace!(
            "Checking server key: {}",
            server_public_key.algorithm().as_str()
        );

        // Store the server key
        *self.server_key.lock().await = Some(server_public_key.clone());

        // If configured to accept unknown hosts, accept the key
        if self.auth_config.accept_unknown_hosts {
            debug!("Accepting server key (accept_unknown_hosts is enabled)");
            *self.key_verified.lock().await = true;
            return Ok(true);
        }

        // Try to verify against known_hosts
        if let Some(ref known_hosts_path) = self.auth_config.known_hosts_file {
            let path = expand_path(known_hosts_path);
            if path.exists() {
                debug!("Known hosts file found at {:?}", path);
                match check_known_hosts_path(&self.host, self.port, server_public_key, &path) {
                    Ok(true) => {
                        debug!("Host key verified against configured known_hosts");
                        *self.key_verified.lock().await = true;
                        return Ok(true);
                    }
                    Ok(false) => {
                        warn!("Host key verification failed against configured known_hosts");
                        return Ok(false);
                    }
                    Err(e) => {
                        warn!("Error verifying host key: {}", e);
                        return Ok(false);
                    }
                }
            }
        }

        // Try default known_hosts location
        if let Some(home) = dirs::home_dir() {
            let default_known_hosts = home.join(".ssh").join("known_hosts");
            if default_known_hosts.exists() {
                debug!("Using default known_hosts at {:?}", default_known_hosts);
                match check_known_hosts_path(
                    &self.host,
                    self.port,
                    server_public_key,
                    &default_known_hosts,
                ) {
                    Ok(true) => {
                        debug!("Host key verified against default known_hosts");
                        *self.key_verified.lock().await = true;
                        return Ok(true);
                    }
                    Ok(false) => {
                        warn!("Host key verification failed against default known_hosts");
                        return Ok(false);
                    }
                    Err(e) => {
                        warn!("Error verifying host key: {}", e);
                        return Ok(false);
                    }
                }
            }
        }

        // If we can't verify, accept with a warning
        warn!("Cannot verify server key - no known_hosts file found");
        *self.key_verified.lock().await = true;
        Ok(true)
    }

    /// Called when the connection is disconnected
    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        debug!("Disconnected: {:?}", reason);
        Ok(())
    }

    /// Handle channel close
    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        trace!("Channel {} closed", channel);
        Ok(())
    }

    /// Handle channel EOF
    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        trace!("Channel {} EOF", channel);
        Ok(())
    }

    /// Handle data received on a channel
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        trace!("Received {} bytes on channel {}", data.len(), channel);
        Ok(())
    }

    /// Handle extended data (stderr) received on a channel
    async fn extended_data(
        &mut self,
        channel: ChannelId,
        ext: u32,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        trace!(
            "Received {} bytes of extended data (type {}) on channel {}",
            data.len(),
            ext,
            channel
        );
        Ok(())
    }
}

/// Authenticator for russh connections
///
/// This struct provides methods to perform various types of SSH authentication
/// using the russh library. It supports multiple authentication methods and
/// automatic fallback.
pub struct RusshAuthenticator {
    /// Authentication configuration
    config: AuthConfig,
    /// SSH agent client (lazily initialized)
    agent: Option<AgentClient<tokio::net::UnixStream>>,
}

impl RusshAuthenticator {
    /// Create a new authenticator
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            agent: None,
        }
    }

    /// Create from host configuration
    pub fn from_host_config(host_config: &HostConfig, use_agent: bool) -> Self {
        Self::new(AuthConfig::from_host_config(host_config, use_agent))
    }

    /// Get the username for authentication
    pub fn username(&self) -> &str {
        &self.config.username
    }

    /// Authenticate using all configured methods with fallback
    ///
    /// This method tries each authentication method in order until one succeeds
    /// or all methods have been exhausted.
    pub async fn authenticate<H: Handler>(
        &mut self,
        handle: &mut Handle<H>,
    ) -> Result<(), ConnectionError> {
        let username = self.config.username.clone();
        let methods = self.config.methods.clone();

        for method in &methods {
            match self.try_auth_method(handle, &username, method).await {
                Ok(AuthResult::Success) => {
                    debug!("Authentication succeeded with method: {:?}", method);
                    return Ok(());
                }
                Ok(AuthResult::Partial) => {
                    debug!("Partial authentication with method: {:?}", method);
                    // Continue trying other methods
                }
                Ok(AuthResult::Failure) => {
                    debug!("Authentication failed with method: {:?}", method);
                    // Try next method
                }
                Ok(AuthResult::Disconnected) => {
                    return Err(ConnectionError::AuthenticationFailed(
                        "Server disconnected during authentication".to_string(),
                    ));
                }
                Err(e) => {
                    warn!("Authentication error with method {:?}: {}", method, e);
                    // Try next method
                }
            }
        }

        Err(ConnectionError::AuthenticationFailed(
            "All authentication methods failed".to_string(),
        ))
    }

    /// Try a single authentication method
    async fn try_auth_method<H: Handler>(
        &mut self,
        handle: &mut Handle<H>,
        username: &str,
        method: &AuthMethod,
    ) -> Result<AuthResult, ConnectionError> {
        match method {
            AuthMethod::None => self.auth_none(handle, username).await,
            AuthMethod::Password(password) => self.auth_password(handle, username, password).await,
            AuthMethod::PublicKey {
                key_path,
                passphrase,
            } => {
                self.auth_publickey(handle, username, key_path, passphrase.as_deref())
                    .await
            }
            AuthMethod::Agent => self.auth_agent(handle, username).await,
            AuthMethod::KeyboardInteractive { responses } => {
                self.auth_keyboard_interactive(handle, username, responses.clone())
                    .await
            }
        }
    }

    /// Attempt "none" authentication (usually fails, but useful for testing)
    async fn auth_none<H: Handler>(
        &self,
        handle: &mut Handle<H>,
        username: &str,
    ) -> Result<AuthResult, ConnectionError> {
        debug!("Attempting 'none' authentication for user: {}", username);

        match handle.authenticate_none(username).await {
            Ok(result) => Ok(map_auth_result(result)),
            Err(e) => Err(ConnectionError::AuthenticationFailed(format!(
                "None authentication failed: {}",
                e
            ))),
        }
    }

    /// Attempt password authentication
    pub async fn auth_password<H: Handler>(
        &self,
        handle: &mut Handle<H>,
        username: &str,
        password: &str,
    ) -> Result<AuthResult, ConnectionError> {
        debug!("Attempting password authentication for user: {}", username);

        match handle.authenticate_password(username, password).await {
            Ok(result) => Ok(map_auth_result(result)),
            Err(e) => Err(ConnectionError::AuthenticationFailed(format!(
                "Password authentication failed: {}",
                e
            ))),
        }
    }

    /// Attempt public key authentication from file
    pub async fn auth_publickey<H: Handler>(
        &self,
        handle: &mut Handle<H>,
        username: &str,
        key_path: &str,
        passphrase: Option<&str>,
    ) -> Result<AuthResult, ConnectionError> {
        let expanded_path = expand_path(key_path);
        debug!(
            "Attempting public key authentication for user: {} with key: {:?}",
            username, expanded_path
        );

        // Load the private key
        let key = load_private_key(&expanded_path, passphrase)?;

        // Get the best RSA hash algorithm if applicable
        let hash_alg = handle
            .best_supported_rsa_hash()
            .await
            .ok()
            .flatten()
            .flatten();
        let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);

        match handle.authenticate_publickey(username, key_with_alg).await {
            Ok(result) => Ok(map_auth_result(result)),
            Err(e) => Err(ConnectionError::AuthenticationFailed(format!(
                "Public key authentication failed: {}",
                e
            ))),
        }
    }

    /// Attempt SSH agent authentication
    pub async fn auth_agent<H: Handler>(
        &mut self,
        handle: &mut Handle<H>,
        username: &str,
    ) -> Result<AuthResult, ConnectionError> {
        debug!("Attempting SSH agent authentication for user: {}", username);

        // Connect to the SSH agent if not already connected
        let agent = self.get_or_connect_agent().await?;

        // Get identities from the agent
        let identities = agent.request_identities().await.map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Failed to list agent identities: {}", e))
        })?;

        if identities.is_empty() {
            debug!("No identities found in SSH agent");
            return Ok(AuthResult::Failure);
        }

        debug!("Found {} identities in SSH agent", identities.len());

        let rsa_hash = if identities
            .iter()
            .any(|identity| identity.algorithm().is_rsa())
        {
            handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten()
        } else {
            None
        };

        // Try each identity
        for identity in identities {
            let algorithm = identity.algorithm();
            debug!(key_type = %algorithm.as_str(), "Trying agent identity");

            let hash_alg = if algorithm.is_rsa() { rsa_hash } else { None };
            match handle
                .authenticate_publickey_with(username, identity.clone(), hash_alg, agent)
                .await
            {
                Ok(result) => match map_auth_result(result) {
                    AuthResult::Success => {
                        debug!("SSH agent authentication succeeded");
                        return Ok(AuthResult::Success);
                    }
                    AuthResult::Partial => {
                        debug!("SSH agent authentication partially succeeded");
                        return Ok(AuthResult::Partial);
                    }
                    AuthResult::Failure => {
                        debug!("Agent identity rejected, trying next...");
                    }
                    AuthResult::Disconnected => return Ok(AuthResult::Disconnected),
                },
                Err(e) => {
                    debug!("Agent auth error: {}, trying next identity...", e);
                }
            }
        }

        Ok(AuthResult::Failure)
    }

    /// Attempt keyboard-interactive authentication
    pub async fn auth_keyboard_interactive<H: Handler>(
        &self,
        handle: &mut Handle<H>,
        username: &str,
        responses: Vec<String>,
    ) -> Result<AuthResult, ConnectionError> {
        debug!(
            "Attempting keyboard-interactive authentication for user: {}",
            username
        );

        let mut response = handle
            .authenticate_keyboard_interactive_start(username, None::<String>)
            .await
            .map_err(|e| {
                ConnectionError::AuthenticationFailed(format!(
                    "Keyboard-interactive authentication failed: {}",
                    e
                ))
            })?;

        loop {
            match response {
                KeyboardInteractiveAuthResponse::Success => return Ok(AuthResult::Success),
                KeyboardInteractiveAuthResponse::Failure {
                    partial_success, ..
                } => {
                    return Ok(map_partial_success(partial_success));
                }
                KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                    let mut answers = Vec::with_capacity(prompts.len());
                    if responses.is_empty() {
                        answers.resize(prompts.len(), String::new());
                    } else if responses.len() == 1 && prompts.len() > 1 {
                        answers.extend(std::iter::repeat_n(responses[0].clone(), prompts.len()));
                    } else {
                        for index in 0..prompts.len() {
                            answers.push(responses.get(index).cloned().unwrap_or_default());
                        }
                    }

                    response = handle
                        .authenticate_keyboard_interactive_respond(answers)
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

    /// Get or connect to the SSH agent
    async fn get_or_connect_agent(
        &mut self,
    ) -> Result<&mut AgentClient<tokio::net::UnixStream>, ConnectionError> {
        if self.agent.is_none() {
            self.agent = Some(connect_to_agent().await?);
        }
        Ok(self.agent.as_mut().unwrap())
    }
}

/// Connect to the SSH agent using the SSH_AUTH_SOCK environment variable
pub async fn connect_to_agent() -> Result<AgentClient<tokio::net::UnixStream>, ConnectionError> {
    debug!("Connecting to SSH agent");

    AgentClient::connect_env().await.map_err(|e| {
        ConnectionError::AuthenticationFailed(format!("Failed to connect to SSH agent: {}", e))
    })
}

/// Load a private key from file
///
/// Supports OpenSSH format keys (both encrypted and unencrypted),
/// as well as PEM format RSA keys.
pub fn load_private_key(path: &Path, passphrase: Option<&str>) -> Result<KeyPair, ConnectionError> {
    debug!("Loading private key from: {:?}", path);

    if !path.exists() {
        return Err(ConnectionError::AuthenticationFailed(format!(
            "Private key file not found: {:?}",
            path
        )));
    }

    keys::load_secret_key(path, passphrase).map_err(|e| {
        ConnectionError::AuthenticationFailed(format!(
            "Failed to load private key from {:?}: {}",
            path, e
        ))
    })
}

/// Load a private key from string content
pub fn load_private_key_from_string(
    content: &str,
    passphrase: Option<&str>,
) -> Result<KeyPair, ConnectionError> {
    debug!("Loading private key from string");

    keys::decode_secret_key(content, passphrase).map_err(|e| {
        ConnectionError::AuthenticationFailed(format!("Failed to decode private key: {}", e))
    })
}

/// Get default identity files to try for authentication
pub fn default_identity_files() -> Vec<std::path::PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"));
    let ssh_dir = home.join(".ssh");

    vec![
        ssh_dir.join("id_ed25519"),
        ssh_dir.join("id_ecdsa"),
        ssh_dir.join("id_rsa"),
        ssh_dir.join("id_dsa"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ========================================================================
    // KeyType Tests
    // ========================================================================

    #[test]
    fn test_key_type_algorithm_names() {
        assert_eq!(KeyType::Ed25519.algorithm_name(), "ssh-ed25519");
        assert_eq!(KeyType::Rsa.algorithm_name(), "ssh-rsa");
        assert_eq!(KeyType::EcdsaP256.algorithm_name(), "ecdsa-sha2-nistp256");
        assert_eq!(KeyType::EcdsaP384.algorithm_name(), "ecdsa-sha2-nistp384");
        assert_eq!(KeyType::EcdsaP521.algorithm_name(), "ecdsa-sha2-nistp521");
    }

    #[test]
    fn test_key_type_default_filenames() {
        assert_eq!(KeyType::Ed25519.default_filename(), "id_ed25519");
        assert_eq!(KeyType::Rsa.default_filename(), "id_rsa");
        assert_eq!(KeyType::EcdsaP256.default_filename(), "id_ecdsa");
    }

    #[test]
    fn test_key_type_detect_rsa_pem() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\ndata\n-----END RSA PRIVATE KEY-----";
        assert_eq!(KeyType::detect_from_content(content), Some(KeyType::Rsa));
    }

    #[test]
    fn test_key_type_detect_ec_pem() {
        let content = "-----BEGIN EC PRIVATE KEY-----\ndata\n-----END EC PRIVATE KEY-----";
        // EC keys need further parsing to determine curve
        assert_eq!(KeyType::detect_from_content(content), None);
    }

    #[test]
    fn test_key_type_detect_openssh() {
        let content =
            "-----BEGIN OPENSSH PRIVATE KEY-----\ndata\n-----END OPENSSH PRIVATE KEY-----";
        // OpenSSH format needs parsing
        assert_eq!(KeyType::detect_from_content(content), None);
    }

    #[test]
    fn test_key_type_display() {
        assert_eq!(format!("{}", KeyType::Ed25519), "ssh-ed25519");
        assert_eq!(format!("{}", KeyType::Rsa), "ssh-rsa");
    }

    // ========================================================================
    // KeyError Tests
    // ========================================================================

    #[test]
    fn test_key_error_display() {
        let err = KeyError::NotFound(PathBuf::from("/path/to/key"));
        assert!(err.to_string().contains("/path/to/key"));

        let err = KeyError::PassphraseRequired(PathBuf::from("/encrypted/key"));
        assert!(err.to_string().contains("passphrase"));
        assert!(err.to_string().contains("/encrypted/key"));

        let err = KeyError::WrongPassphrase(PathBuf::from("/key"));
        assert!(err.to_string().contains("Wrong passphrase"));
    }

    #[test]
    fn test_key_error_to_connection_error() {
        let key_error = KeyError::NotFound(PathBuf::from("/missing/key"));
        let conn_error: ConnectionError = key_error.into();
        match conn_error {
            ConnectionError::AuthenticationFailed(msg) => {
                assert!(msg.contains("/missing/key"));
            }
            _ => panic!("Expected AuthenticationFailed"),
        }
    }

    // ========================================================================
    // KeyLoader Tests
    // ========================================================================

    #[test]
    fn test_key_loader_new() {
        let loader = KeyLoader::new();
        assert!(loader.use_agent);
        assert!(loader.passphrase.is_none());
    }

    #[test]
    fn test_key_loader_with_passphrase() {
        let loader = KeyLoader::new().with_passphrase("secret");
        assert_eq!(loader.passphrase, Some("secret".to_string()));
        assert!(loader.has_passphrase());
    }

    #[test]
    fn test_key_loader_with_key_path() {
        let loader = KeyLoader::new().with_key_path("/custom/path/id_rsa");
        assert!(loader
            .search_paths
            .contains(&PathBuf::from("/custom/path/id_rsa")));
        // Custom path should be first
        assert_eq!(loader.search_paths[0], PathBuf::from("/custom/path/id_rsa"));
    }

    #[test]
    fn test_key_loader_with_key_paths() {
        let paths = vec![PathBuf::from("/path1/key"), PathBuf::from("/path2/key")];
        let loader = KeyLoader::new().with_key_paths(paths);
        assert!(loader.search_paths.contains(&PathBuf::from("/path1/key")));
        assert!(loader.search_paths.contains(&PathBuf::from("/path2/key")));
    }

    #[test]
    fn test_key_loader_from_host_config() {
        let host_config = HostConfig {
            identity_file: Some("~/.ssh/custom_key".to_string()),
            password: Some("secret".to_string()),
            ..Default::default()
        };

        let loader = KeyLoader::from_host_config(&host_config);
        assert_eq!(loader.passphrase, Some("secret".to_string()));
        // The custom key path should be at the front
        assert!(!loader.search_paths.is_empty());
    }

    #[test]
    fn test_key_loader_load_nonexistent() {
        let loader = KeyLoader::new();
        let result = loader.load_key(Path::new("/nonexistent/key"));
        assert!(matches!(result, Err(KeyError::NotFound(_))));
    }

    // ========================================================================
    // Encryption Detection Tests
    // ========================================================================

    #[test]
    fn test_detect_encryption_unencrypted() {
        let content =
            "-----BEGIN OPENSSH PRIVATE KEY-----\ndata\n-----END OPENSSH PRIVATE KEY-----";
        assert!(!detect_encryption_from_content(content));
    }

    #[test]
    fn test_detect_encryption_encrypted_pkcs8() {
        let content =
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\ndata\n-----END ENCRYPTED PRIVATE KEY-----";
        assert!(detect_encryption_from_content(content));
    }

    #[test]
    fn test_detect_encryption_proc_type() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-256-CBC,12345\ndata\n-----END RSA PRIVATE KEY-----";
        assert!(detect_encryption_from_content(content));
    }

    #[test]
    fn test_detect_encryption_dek_info() {
        let content = "DEK-Info: AES-128-CBC,abcdef\ndata";
        assert!(detect_encryption_from_content(content));
    }

    #[test]
    fn test_is_key_encrypted_file() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "-----BEGIN ENCRYPTED PRIVATE KEY-----").unwrap();
        writeln!(temp, "data").unwrap();
        writeln!(temp, "-----END ENCRYPTED PRIVATE KEY-----").unwrap();

        let result = is_key_encrypted(temp.path());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_is_key_encrypted_unencrypted_file() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "-----BEGIN OPENSSH PRIVATE KEY-----").unwrap();
        writeln!(temp, "data").unwrap();
        writeln!(temp, "-----END OPENSSH PRIVATE KEY-----").unwrap();

        let result = is_key_encrypted(temp.path());
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_is_key_encrypted_missing_file() {
        let result = is_key_encrypted(Path::new("/nonexistent/file"));
        assert!(matches!(result, Err(KeyError::ReadError { .. })));
    }

    // ========================================================================
    // Standard Key Locations Tests
    // ========================================================================

    #[test]
    fn test_standard_key_locations() {
        // This test just ensures the function doesn't panic
        let locations = standard_key_locations();
        // All returned files should exist
        for path in &locations {
            assert!(path.exists());
        }
    }

    // ========================================================================
    // AuthConfig Tests
    // ========================================================================

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert!(!config.username.is_empty());
        assert_eq!(config.methods.len(), 1);
        assert_eq!(config.methods[0], AuthMethod::Agent);
        assert!(!config.accept_unknown_hosts);
    }

    #[test]
    fn test_auth_config_builder() {
        let config = AuthConfig::new("testuser")
            .with_password("secret")
            .with_public_key("~/.ssh/id_rsa", None)
            .with_agent()
            .accept_unknown_hosts(true);

        assert_eq!(config.username, "testuser");
        assert_eq!(config.methods.len(), 3);
        assert!(config.accept_unknown_hosts);
    }

    #[test]
    fn test_auth_method_equality() {
        assert_eq!(AuthMethod::None, AuthMethod::None);
        assert_eq!(
            AuthMethod::Password("test".to_string()),
            AuthMethod::Password("test".to_string())
        );
        assert_ne!(
            AuthMethod::Password("test".to_string()),
            AuthMethod::Password("other".to_string())
        );
    }

    #[test]
    fn test_auth_config_from_host_config() {
        let host_config = HostConfig {
            user: Some("admin".to_string()),
            password: Some("secret".to_string()),
            identity_file: Some("~/.ssh/custom_key".to_string()),
            strict_host_key_checking: Some(false),
            ..Default::default()
        };

        let config = AuthConfig::from_host_config(&host_config, true);

        assert_eq!(config.username, "admin");
        assert!(config.accept_unknown_hosts);
        // Should have agent, public key, and password methods
        assert!(config.methods.len() >= 2);
    }

    #[test]
    fn test_default_identity_files_function() {
        // This test just ensures the function doesn't panic
        let files = default_identity_files();
        // All returned files should exist
        for file in &files {
            assert!(file.exists());
        }
    }

    #[test]
    fn test_russh_client_handler_initialization() {
        let auth_config = AuthConfig::default();
        let host = "example.com".to_string();
        let port = 2222;

        let handler = RusshClientHandler::new(auth_config.clone(), host.clone(), port);

        assert_eq!(handler.host, host);
        assert_eq!(handler.port, port);
        assert_eq!(handler.auth_config.username, auth_config.username);
    }
}
