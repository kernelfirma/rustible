//! Network security module for enhanced connection security.
//!
//! This module provides:
//! - Host key verification with pinning support
//! - Jump host/bastion server support
//! - Network isolation for module execution
//! - TLS certificate validation for HTTPS modules
//! - Connection encryption audit logging
//!
//! # Security Features
//!
//! ## Host Key Pinning
//!
//! Host key pinning provides defense against MITM attacks by requiring
//! SSH servers to present a pre-approved public key fingerprint.
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::security::*;
//! let policy = HostKeyPolicy::new()
//!     .with_pin("server.example.com", "SHA256:abc123...")
//!     .with_mode(HostKeyVerificationMode::PinnedOnly);
//! # Ok(())
//! # }
//! ```
//!
//! ## Jump Hosts (Bastion Servers)
//!
//! Configure multi-hop SSH connections through bastion hosts:
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::security::*;
//! let jump_config = JumpHostConfig::new("bastion.example.com")
//!     .port(22)
//!     .user("jump_user");
//! # Ok(())
//! # }
//! ```
//!
//! ## Network Isolation
//!
//! Restrict module execution to specific networks and ports:
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::security::*;
//! let isolation = NetworkIsolation::restrictive()
//!     .allow_host("192.168.1.0/24")
//!     .allow_port(22);
//! # Ok(())
//! # }
//! ```
//!
//! ## TLS Validation
//!
//! Configure TLS certificate validation for HTTPS connections:
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::security::*;
//! let tls_config = TlsValidationConfig::new()
//!     .with_require_valid_cert(true)
//!     .with_ca_bundle("/etc/ssl/certs/ca-certificates.crt")
//!     .with_min_tls_version(TlsVersion::Tls12);
//! # Ok(())
//! # }
//! ```
//!
//! ## Encryption Audit Logging
//!
//! Log all encryption-related events for security auditing:
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::security::*;
//! let audit_log = EncryptionAuditLog::new("/var/log/rustible/encryption.log")
//!     .with_level(AuditLevel::Verbose);
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// Re-export jump host types from the jump_host module
pub use super::jump_host::{JumpHostChain, JumpHostConfig};

// ============================================================================
// Error Types
// ============================================================================

/// Security-related errors
#[derive(Error, Debug)]
pub enum SecurityError {
    /// Host key verification failed
    #[error("Host key verification failed for {host}: {reason}")]
    HostKeyVerificationFailed { host: String, reason: String },

    /// Host key mismatch (potential MITM attack)
    #[error("Host key mismatch for {host}: stored fingerprint {expected}, received {actual}")]
    HostKeyMismatch {
        host: String,
        expected: String,
        actual: String,
    },

    /// Jump host connection failed
    #[error("Jump host connection failed: {0}")]
    JumpHostConnectionFailed(String),

    /// Network isolation violation
    #[error("Network isolation violation: {0}")]
    NetworkIsolationViolation(String),

    /// TLS validation failed
    #[error("TLS certificate validation failed: {0}")]
    TlsValidationFailed(String),

    /// Certificate pinning failed
    #[error("Certificate pinning failed for {host}: {reason}")]
    CertificatePinningFailed { host: String, reason: String },

    /// Audit logging error
    #[error("Audit logging error: {0}")]
    AuditLoggingError(String),

    /// Invalid configuration
    #[error("Invalid security configuration: {0}")]
    InvalidConfiguration(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type SecurityResult<T> = Result<T, SecurityError>;

// ============================================================================
// Host Key Verification and Pinning
// ============================================================================

/// Policy for host key verification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum HostKeyVerificationMode {
    /// Accept any host key (insecure, for testing only)
    AcceptAll,
    /// Accept first seen and verify on subsequent connections (TOFU)
    #[default]
    TrustOnFirstUse,
    /// Only accept keys from known_hosts file
    KnownHostsOnly,
    /// Only accept pinned keys (most secure)
    PinnedOnly,
    /// Combination of known_hosts and pinned keys
    KnownHostsOrPinned,
}

/// A pinned host key entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedHostKey {
    /// The host pattern (can include wildcards)
    pub host_pattern: String,
    /// The port (default 22)
    pub port: u16,
    /// SHA256 fingerprint of the public key
    pub fingerprint: String,
    /// Key type (e.g., "ssh-ed25519", "ssh-rsa")
    pub key_type: String,
    /// When this pin was added
    pub added_at: DateTime<Utc>,
    /// Optional expiration time
    pub expires_at: Option<DateTime<Utc>>,
    /// Comment/description
    pub comment: Option<String>,
}

impl PinnedHostKey {
    /// Create a new pinned host key
    pub fn new(host: impl Into<String>, fingerprint: impl Into<String>) -> Self {
        Self {
            host_pattern: host.into(),
            port: 22,
            fingerprint: fingerprint.into(),
            key_type: String::new(),
            added_at: Utc::now(),
            expires_at: None,
            comment: None,
        }
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the key type
    pub fn with_key_type(mut self, key_type: impl Into<String>) -> Self {
        self.key_type = key_type.into();
        self
    }

    /// Set expiration time
    pub fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Add a comment
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    /// Check if this pin has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at.map(|exp| exp < Utc::now()).unwrap_or(false)
    }

    /// Check if this pin matches a host
    pub fn matches_host(&self, host: &str, port: u16) -> bool {
        if self.port != port {
            return false;
        }

        // Simple wildcard matching
        if self.host_pattern.contains('*') {
            let pattern = self
                .host_pattern
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            regex::Regex::new(&format!("^{}$", pattern))
                .map(|re| re.is_match(host))
                .unwrap_or(false)
        } else {
            self.host_pattern == host
        }
    }
}

/// Host key verification policy with pinning support
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct HostKeyPolicy {
    /// Verification mode
    pub mode: HostKeyVerificationMode,
    /// Pinned host keys
    pub pinned_keys: Vec<PinnedHostKey>,
    /// Path to custom known_hosts file
    pub known_hosts_path: Option<PathBuf>,
    /// Whether to automatically add new hosts to known_hosts (TOFU)
    pub auto_add_hosts: bool,
    /// Whether to log verification events
    pub log_verification: bool,
    /// Callback for unknown hosts (return true to accept)
    #[serde(skip)]
    unknown_host_callback: Option<UnknownHostCallback>,
}

type UnknownHostCallback = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

impl std::fmt::Debug for HostKeyPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostKeyPolicy")
            .field("mode", &self.mode)
            .field("pinned_keys", &self.pinned_keys)
            .field("known_hosts_path", &self.known_hosts_path)
            .field("auto_add_hosts", &self.auto_add_hosts)
            .field("log_verification", &self.log_verification)
            .field(
                "unknown_host_callback",
                &self.unknown_host_callback.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Default for HostKeyPolicy {
    fn default() -> Self {
        Self {
            mode: HostKeyVerificationMode::default(),
            pinned_keys: Vec::new(),
            known_hosts_path: None,
            auto_add_hosts: true,
            log_verification: true,
            unknown_host_callback: None,
        }
    }
}

impl HostKeyPolicy {
    /// Create a new host key policy
    pub fn new() -> Self {
        Self::default()
    }

    /// Set verification mode
    pub fn with_mode(mut self, mode: HostKeyVerificationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Add a pinned key
    pub fn with_pin(mut self, host: impl Into<String>, fingerprint: impl Into<String>) -> Self {
        self.pinned_keys.push(PinnedHostKey::new(host, fingerprint));
        self
    }

    /// Add a pinned key with full configuration
    pub fn with_pinned_key(mut self, key: PinnedHostKey) -> Self {
        self.pinned_keys.push(key);
        self
    }

    /// Set custom known_hosts path
    pub fn with_known_hosts_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.known_hosts_path = Some(path.into());
        self
    }

    /// Enable/disable auto-adding new hosts
    pub fn with_auto_add_hosts(mut self, enabled: bool) -> Self {
        self.auto_add_hosts = enabled;
        self
    }

    /// Enable/disable verification logging
    pub fn with_log_verification(mut self, enabled: bool) -> Self {
        self.log_verification = enabled;
        self
    }

    /// Set callback for unknown hosts
    pub fn with_unknown_host_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, &str) -> bool + Send + Sync + 'static,
    {
        self.unknown_host_callback = Some(Arc::new(callback));
        self
    }

    /// Verify a host key against the policy
    pub fn verify(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
    ) -> SecurityResult<HostKeyVerificationResult> {
        if self.log_verification {
            debug!(
                host = %host,
                port = %port,
                key_type = %key_type,
                fingerprint = %fingerprint,
                mode = ?self.mode,
                "Verifying host key"
            );
        }

        match self.mode {
            HostKeyVerificationMode::AcceptAll => {
                warn!(host = %host, "Accepting host key without verification (AcceptAll mode)");
                Ok(HostKeyVerificationResult::Accepted {
                    reason: "AcceptAll mode".to_string(),
                })
            }

            HostKeyVerificationMode::PinnedOnly => self.verify_pinned_only(host, port, fingerprint),

            HostKeyVerificationMode::KnownHostsOnly => {
                self.verify_known_hosts_only(host, port, fingerprint)
            }

            HostKeyVerificationMode::KnownHostsOrPinned => {
                // Try pinned first, then known_hosts
                match self.verify_pinned_only(host, port, fingerprint) {
                    Ok(result) => Ok(result),
                    Err(_) => self.verify_known_hosts_only(host, port, fingerprint),
                }
            }

            HostKeyVerificationMode::TrustOnFirstUse => {
                // Check pinned keys first
                if let Some(pin) = self.find_pin(host, port) {
                    return self.verify_against_pin(host, &pin, fingerprint);
                }

                // Check known_hosts
                match self.check_known_hosts(host, port, fingerprint)? {
                    KnownHostsCheckResult::Verified => Ok(HostKeyVerificationResult::Accepted {
                        reason: "Known host verified".to_string(),
                    }),
                    KnownHostsCheckResult::Mismatch { expected } => {
                        Err(SecurityError::HostKeyMismatch {
                            host: host.to_string(),
                            expected,
                            actual: fingerprint.to_string(),
                        })
                    }
                    KnownHostsCheckResult::Unknown => {
                        // TOFU: Accept and optionally add to known_hosts
                        if self.auto_add_hosts {
                            info!(host = %host, "First connection, adding to known hosts (TOFU)");
                        }
                        Ok(HostKeyVerificationResult::AcceptedTOFU {
                            should_save: self.auto_add_hosts,
                        })
                    }
                }
            }
        }
    }

    /// Verify using pinned keys only
    fn verify_pinned_only(
        &self,
        host: &str,
        port: u16,
        fingerprint: &str,
    ) -> SecurityResult<HostKeyVerificationResult> {
        match self.find_pin(host, port) {
            Some(pin) => self.verify_against_pin(host, &pin, fingerprint),
            None => Err(SecurityError::HostKeyVerificationFailed {
                host: host.to_string(),
                reason: "No pinned key found for this host".to_string(),
            }),
        }
    }

    /// Verify using known_hosts only
    fn verify_known_hosts_only(
        &self,
        host: &str,
        port: u16,
        fingerprint: &str,
    ) -> SecurityResult<HostKeyVerificationResult> {
        match self.check_known_hosts(host, port, fingerprint)? {
            KnownHostsCheckResult::Verified => Ok(HostKeyVerificationResult::Accepted {
                reason: "Known host verified".to_string(),
            }),
            KnownHostsCheckResult::Mismatch { expected } => Err(SecurityError::HostKeyMismatch {
                host: host.to_string(),
                expected,
                actual: fingerprint.to_string(),
            }),
            KnownHostsCheckResult::Unknown => Err(SecurityError::HostKeyVerificationFailed {
                host: host.to_string(),
                reason: "Host not found in known_hosts".to_string(),
            }),
        }
    }

    /// Find a matching pin for a host
    fn find_pin(&self, host: &str, port: u16) -> Option<PinnedHostKey> {
        self.pinned_keys
            .iter()
            .find(|p| p.matches_host(host, port) && !p.is_expired())
            .cloned()
    }

    /// Verify against a specific pin
    fn verify_against_pin(
        &self,
        host: &str,
        pin: &PinnedHostKey,
        fingerprint: &str,
    ) -> SecurityResult<HostKeyVerificationResult> {
        if pin.is_expired() {
            return Err(SecurityError::HostKeyVerificationFailed {
                host: host.to_string(),
                reason: "Pinned key has expired".to_string(),
            });
        }

        // Normalize fingerprints for comparison
        let expected = normalize_fingerprint(&pin.fingerprint);
        let actual = normalize_fingerprint(fingerprint);

        if expected == actual {
            Ok(HostKeyVerificationResult::Accepted {
                reason: "Matched pinned key".to_string(),
            })
        } else {
            Err(SecurityError::HostKeyMismatch {
                host: host.to_string(),
                expected,
                actual,
            })
        }
    }

    /// Check known_hosts file
    fn check_known_hosts(
        &self,
        host: &str,
        port: u16,
        fingerprint: &str,
    ) -> SecurityResult<KnownHostsCheckResult> {
        let known_hosts_path = self.known_hosts_path.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".ssh")
                .join("known_hosts")
        });

        if !known_hosts_path.exists() {
            return Ok(KnownHostsCheckResult::Unknown);
        }

        let file = File::open(&known_hosts_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((stored_host, stored_fingerprint)) = parse_known_hosts_line(line, port) {
                if host_matches(&stored_host, host, port) {
                    let expected = normalize_fingerprint(&stored_fingerprint);
                    let actual = normalize_fingerprint(fingerprint);

                    if expected == actual {
                        return Ok(KnownHostsCheckResult::Verified);
                    }
                    return Ok(KnownHostsCheckResult::Mismatch { expected });
                }
            }
        }

        Ok(KnownHostsCheckResult::Unknown)
    }

    /// Load pinned keys from a TOML file
    pub fn load_pins_from_file(mut self, path: impl AsRef<Path>) -> SecurityResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let pins: Vec<PinnedHostKey> = toml::from_str(&content).map_err(|e| {
            SecurityError::InvalidConfiguration(format!("Failed to parse pins file: {}", e))
        })?;
        self.pinned_keys.extend(pins);
        Ok(self)
    }

    /// Save pinned keys to a TOML file
    pub fn save_pins_to_file(&self, path: impl AsRef<Path>) -> SecurityResult<()> {
        let content = toml::to_string_pretty(&self.pinned_keys).map_err(|e| {
            SecurityError::InvalidConfiguration(format!("Failed to serialize pins: {}", e))
        })?;
        std::fs::write(path.as_ref(), content)?;
        Ok(())
    }
}

/// Result of host key verification
#[derive(Debug, Clone)]
pub enum HostKeyVerificationResult {
    /// Key accepted
    Accepted { reason: String },
    /// Key accepted via Trust On First Use
    AcceptedTOFU { should_save: bool },
    /// Key rejected
    Rejected { reason: String },
}

/// Result of checking known_hosts
#[derive(Debug)]
enum KnownHostsCheckResult {
    Verified,
    Mismatch { expected: String },
    Unknown,
}

/// Normalize a fingerprint for comparison
fn normalize_fingerprint(fp: &str) -> String {
    // Remove common prefixes and normalize
    let fp = fp.trim();
    let fp = fp.strip_prefix("SHA256:").unwrap_or(fp);
    let fp = fp.strip_prefix("MD5:").unwrap_or(fp);
    fp.replace(':', "").to_lowercase()
}

/// Parse a known_hosts line and extract host and key fingerprint
fn parse_known_hosts_line(line: &str, _port: u16) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let host = parts[0].to_string();
    let key_data = parts[2];

    // Compute fingerprint from base64 key data
    if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key_data)
    {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        let fingerprint =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, result);
        Some((host, format!("SHA256:{}", fingerprint)))
    } else {
        None
    }
}

/// Check if a stored host pattern matches the given host
fn host_matches(pattern: &str, host: &str, port: u16) -> bool {
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

    // Handle comma-separated hosts
    for p in pattern.split(',') {
        if p == host {
            return true;
        }
        // Wildcard matching
        if p.contains('*') {
            let regex_pattern = p.replace('.', r"\.").replace('*', ".*").replace('?', ".");
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                if re.is_match(host) {
                    return true;
                }
            }
        }
    }

    // Standard port check
    port == 22 && pattern == host
}

// ============================================================================
// Jump Host / Bastion Support
// ============================================================================
//
// Note: JumpHostConfig and JumpHostChain types are imported from the jump_host module
// and re-exported from this module for convenience. See super::jump_host for the
// implementation details.
//
// The jump_host module provides:
// - JumpHostConfig: Configuration for a single jump/bastion host
// - JumpHostChain: Chain of jump hosts for multi-hop connections
// - JumpHostResolver: Resolution of ProxyJump configurations

// ============================================================================
// Network Isolation
// ============================================================================

/// Network isolation configuration for module execution
#[derive(Debug, Clone, Default)]
pub struct NetworkIsolation {
    /// Allowed hosts/CIDRs for outbound connections
    pub allowed_hosts: HashSet<String>,
    /// Denied hosts/CIDRs (takes precedence over allowed)
    pub denied_hosts: HashSet<String>,
    /// Allowed ports
    pub allowed_ports: HashSet<u16>,
    /// Denied ports (takes precedence over allowed)
    pub denied_ports: HashSet<u16>,
    /// Allow all outbound by default
    pub allow_all_outbound: bool,
    /// Allow DNS resolution
    pub allow_dns: bool,
    /// Log all network access attempts
    pub log_access: bool,
}

impl NetworkIsolation {
    /// Create a new network isolation configuration
    pub fn new() -> Self {
        Self {
            allowed_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            allowed_ports: HashSet::new(),
            denied_ports: HashSet::new(),
            allow_all_outbound: true,
            allow_dns: true,
            log_access: true,
        }
    }

    /// Create a restrictive isolation (deny all by default)
    pub fn restrictive() -> Self {
        Self {
            allow_all_outbound: false,
            allow_dns: false,
            ..Self::new()
        }
    }

    /// Allow a specific host or CIDR
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.insert(host.into());
        self
    }

    /// Deny a specific host or CIDR
    pub fn deny_host(mut self, host: impl Into<String>) -> Self {
        self.denied_hosts.insert(host.into());
        self
    }

    /// Allow a specific port
    pub fn allow_port(mut self, port: u16) -> Self {
        self.allowed_ports.insert(port);
        self
    }

    /// Deny a specific port
    pub fn deny_port(mut self, port: u16) -> Self {
        self.denied_ports.insert(port);
        self
    }

    /// Allow common SSH port
    pub fn allow_ssh(self) -> Self {
        self.allow_port(22)
    }

    /// Allow common HTTP/HTTPS ports
    pub fn allow_http(self) -> Self {
        self.allow_port(80).allow_port(443)
    }

    /// Enable/disable DNS resolution
    pub fn with_dns(mut self, allow: bool) -> Self {
        self.allow_dns = allow;
        self
    }

    /// Set default outbound policy
    pub fn with_allow_all_outbound(mut self, allow: bool) -> Self {
        self.allow_all_outbound = allow;
        self
    }

    /// Enable/disable access logging
    pub fn with_log_access(mut self, log: bool) -> Self {
        self.log_access = log;
        self
    }

    /// Check if a connection to host:port is allowed
    pub fn is_allowed(&self, host: &str, port: u16) -> SecurityResult<()> {
        if self.log_access {
            debug!(host = %host, port = %port, "Checking network isolation policy");
        }

        // Check denied ports first (takes precedence)
        if self.denied_ports.contains(&port) {
            return Err(SecurityError::NetworkIsolationViolation(format!(
                "Port {} is denied",
                port
            )));
        }

        // Check denied hosts
        if self.is_host_in_set(host, &self.denied_hosts) {
            return Err(SecurityError::NetworkIsolationViolation(format!(
                "Host {} is denied",
                host
            )));
        }

        // If not allowing all outbound, check allowed lists
        if !self.allow_all_outbound {
            // Check allowed ports
            if !self.allowed_ports.is_empty() && !self.allowed_ports.contains(&port) {
                return Err(SecurityError::NetworkIsolationViolation(format!(
                    "Port {} is not in allowed list",
                    port
                )));
            }

            // Check allowed hosts
            if !self.allowed_hosts.is_empty() && !self.is_host_in_set(host, &self.allowed_hosts) {
                return Err(SecurityError::NetworkIsolationViolation(format!(
                    "Host {} is not in allowed list",
                    host
                )));
            }
        }

        Ok(())
    }

    /// Check if a host matches any entry in a set (supports CIDR notation)
    fn is_host_in_set(&self, host: &str, set: &HashSet<String>) -> bool {
        // Direct match
        if set.contains(host) {
            return true;
        }

        // Parse host as IP for CIDR matching
        if let Ok(ip) = host.parse::<IpAddr>() {
            for entry in set {
                if let Some((network, prefix)) = entry.split_once('/') {
                    if let Ok(network_ip) = network.parse::<IpAddr>() {
                        if let Ok(prefix_len) = prefix.parse::<u8>() {
                            if ip_in_cidr(ip, network_ip, prefix_len) {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // Wildcard matching for domain names
        for entry in set {
            if entry.contains('*') {
                let pattern = entry
                    .replace('.', r"\.")
                    .replace('*', ".*")
                    .replace('?', ".");
                if let Ok(re) = regex::Regex::new(&format!("^{}$", pattern)) {
                    if re.is_match(host) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

/// Check if an IP is within a CIDR range
fn ip_in_cidr(ip: IpAddr, network: IpAddr, prefix_len: u8) -> bool {
    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(network)) => {
            let ip_bits: u32 = ip.into();
            let network_bits: u32 = network.into();
            let mask: u32 = (!0u32).checked_shl(32 - prefix_len as u32).unwrap_or(0);
            (ip_bits & mask) == (network_bits & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(network)) => {
            let ip_bits: u128 = ip.into();
            let network_bits: u128 = network.into();
            let mask: u128 = (!0u128).checked_shl(128 - prefix_len as u32).unwrap_or(0);
            (ip_bits & mask) == (network_bits & mask)
        }
        _ => false,
    }
}

// ============================================================================
// TLS Certificate Validation
// ============================================================================

/// Minimum TLS version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TlsVersion {
    Tls10,
    Tls11,
    #[default]
    Tls12,
    Tls13,
}

/// TLS certificate validation configuration
#[derive(Debug, Clone)]
pub struct TlsValidationConfig {
    /// Require valid certificates
    pub require_valid_cert: bool,
    /// Minimum TLS version
    pub min_tls_version: TlsVersion,
    /// Path to CA bundle
    pub ca_bundle: Option<PathBuf>,
    /// Additional CA certificates
    pub additional_ca_certs: Vec<PathBuf>,
    /// Enable certificate revocation checking (OCSP/CRL)
    pub check_revocation: bool,
    /// Certificate pins (HPKP-style)
    pub certificate_pins: HashMap<String, Vec<String>>,
    /// Allow specific self-signed certificates by fingerprint
    pub allowed_self_signed: HashSet<String>,
    /// Verify hostname matches certificate
    pub verify_hostname: bool,
    /// Log TLS events
    pub log_tls_events: bool,
}

impl Default for TlsValidationConfig {
    fn default() -> Self {
        Self {
            require_valid_cert: true,
            min_tls_version: TlsVersion::Tls12,
            ca_bundle: None,
            additional_ca_certs: Vec::new(),
            check_revocation: false,
            certificate_pins: HashMap::new(),
            allowed_self_signed: HashSet::new(),
            verify_hostname: true,
            log_tls_events: true,
        }
    }
}

impl TlsValidationConfig {
    /// Create a new TLS validation configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a permissive configuration (for testing)
    pub fn permissive() -> Self {
        Self {
            require_valid_cert: false,
            verify_hostname: false,
            ..Self::default()
        }
    }

    /// Enable/disable valid cert requirement
    pub fn with_require_valid_cert(mut self, require: bool) -> Self {
        self.require_valid_cert = require;
        self
    }

    /// Set minimum TLS version
    pub fn with_min_tls_version(mut self, version: TlsVersion) -> Self {
        self.min_tls_version = version;
        self
    }

    /// Set CA bundle path
    pub fn with_ca_bundle(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca_bundle = Some(path.into());
        self
    }

    /// Add an additional CA certificate
    pub fn with_additional_ca(mut self, path: impl Into<PathBuf>) -> Self {
        self.additional_ca_certs.push(path.into());
        self
    }

    /// Enable/disable revocation checking
    pub fn with_check_revocation(mut self, check: bool) -> Self {
        self.check_revocation = check;
        self
    }

    /// Add a certificate pin for a host
    pub fn with_certificate_pin(
        mut self,
        host: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Self {
        self.certificate_pins
            .entry(host.into())
            .or_default()
            .push(fingerprint.into());
        self
    }

    /// Allow a specific self-signed certificate
    pub fn with_allowed_self_signed(mut self, fingerprint: impl Into<String>) -> Self {
        self.allowed_self_signed.insert(fingerprint.into());
        self
    }

    /// Enable/disable hostname verification
    pub fn with_verify_hostname(mut self, verify: bool) -> Self {
        self.verify_hostname = verify;
        self
    }

    /// Enable/disable TLS event logging
    pub fn with_log_tls_events(mut self, log: bool) -> Self {
        self.log_tls_events = log;
        self
    }

    /// Validate a certificate for a given host
    pub fn validate_certificate(
        &self,
        host: &str,
        cert_fingerprint: &str,
        cert_chain_valid: bool,
        is_self_signed: bool,
    ) -> SecurityResult<()> {
        if self.log_tls_events {
            debug!(
                host = %host,
                fingerprint = %cert_fingerprint,
                chain_valid = %cert_chain_valid,
                self_signed = %is_self_signed,
                "Validating TLS certificate"
            );
        }

        // Check certificate pins first
        if let Some(pins) = self.certificate_pins.get(host) {
            let normalized = normalize_fingerprint(cert_fingerprint);
            let pin_matched = pins.iter().any(|p| normalize_fingerprint(p) == normalized);

            if !pin_matched {
                return Err(SecurityError::CertificatePinningFailed {
                    host: host.to_string(),
                    reason: "Certificate fingerprint does not match any pinned value".to_string(),
                });
            }
        }

        // Handle self-signed certificates
        if is_self_signed {
            let normalized = normalize_fingerprint(cert_fingerprint);
            if self.allowed_self_signed.contains(&normalized) {
                info!(host = %host, "Accepting allowed self-signed certificate");
                return Ok(());
            }

            if self.require_valid_cert {
                return Err(SecurityError::TlsValidationFailed(format!(
                    "Self-signed certificate for {} is not in allowed list",
                    host
                )));
            }
        }

        // Check certificate chain validity
        if self.require_valid_cert && !cert_chain_valid {
            return Err(SecurityError::TlsValidationFailed(format!(
                "Certificate chain validation failed for {}",
                host
            )));
        }

        Ok(())
    }

    /// Convert to reqwest TLS configuration
    #[cfg(feature = "reqwest")]
    pub fn to_reqwest_config(&self) -> reqwest::ClientBuilder {
        let mut builder = reqwest::Client::builder();

        if !self.require_valid_cert {
            builder = builder.danger_accept_invalid_certs(true);
        }

        if !self.verify_hostname {
            builder = builder.danger_accept_invalid_hostnames(true);
        }

        // Add CA bundle if specified
        if let Some(ca_path) = &self.ca_bundle {
            if let Ok(ca_data) = std::fs::read(ca_path) {
                if let Ok(cert) = reqwest::Certificate::from_pem(&ca_data) {
                    builder = builder.add_root_certificate(cert);
                }
            }
        }

        // Add additional CA certificates
        for ca_path in &self.additional_ca_certs {
            if let Ok(ca_data) = std::fs::read(ca_path) {
                if let Ok(cert) = reqwest::Certificate::from_pem(&ca_data) {
                    builder = builder.add_root_certificate(cert);
                }
            }
        }

        // Set minimum TLS version
        builder = match self.min_tls_version {
            TlsVersion::Tls10 => builder.min_tls_version(reqwest::tls::Version::TLS_1_0),
            TlsVersion::Tls11 => builder.min_tls_version(reqwest::tls::Version::TLS_1_1),
            TlsVersion::Tls12 => builder.min_tls_version(reqwest::tls::Version::TLS_1_2),
            TlsVersion::Tls13 => builder.min_tls_version(reqwest::tls::Version::TLS_1_3),
        };

        builder
    }
}

// ============================================================================
// Encryption Audit Logging
// ============================================================================

/// Level of detail for audit logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuditLevel {
    /// Log only errors and security violations
    Minimal,
    /// Log connections and key operations
    #[default]
    Standard,
    /// Log all encryption-related events
    Verbose,
}

/// Type of audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    // Connection events
    ConnectionAttempt,
    ConnectionEstablished,
    ConnectionFailed,
    ConnectionClosed,

    // Host key events
    HostKeyVerified,
    HostKeyRejected,
    HostKeyMismatch,
    HostKeyFirstSeen,
    HostKeyPinned,

    // TLS events
    TlsHandshakeStarted,
    TlsHandshakeCompleted,
    TlsCertificateVerified,
    TlsCertificateRejected,
    TlsCertificatePinned,

    // Authentication events
    AuthenticationStarted,
    AuthenticationSucceeded,
    AuthenticationFailed,

    // Jump host events
    JumpHostConnecting,
    JumpHostConnected,
    JumpHostFailed,

    // Network isolation events
    NetworkAccessAllowed,
    NetworkAccessDenied,

    // Encryption events
    EncryptionNegotiated,
    CipherSelected,
    KeyExchangeCompleted,
}

/// An audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: AuditEventType,
    /// Host involved
    pub host: Option<String>,
    /// Port involved
    pub port: Option<u16>,
    /// User involved
    pub user: Option<String>,
    /// Session ID
    pub session_id: Option<String>,
    /// Event message
    pub message: String,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Success/failure
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

impl AuditEvent {
    /// Create a new audit event
    pub fn new(event_type: AuditEventType, message: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            host: None,
            port: None,
            user: None,
            session_id: None,
            message: message.into(),
            metadata: HashMap::new(),
            success: true,
            error: None,
        }
    }

    /// Set host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set user
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Mark as failure
    pub fn with_failure(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self
    }

    /// Convert to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| format!("{:?}", self))
    }

    /// Convert to log line format
    pub fn to_log_line(&self) -> String {
        let status = if self.success { "OK" } else { "FAIL" };
        let host_info = self
            .host
            .as_ref()
            .map(|h| {
                if let Some(p) = self.port {
                    format!(" host={}:{}", h, p)
                } else {
                    format!(" host={}", h)
                }
            })
            .unwrap_or_default();
        let user_info = self
            .user
            .as_ref()
            .map(|u| format!(" user={}", u))
            .unwrap_or_default();
        let session_info = self
            .session_id
            .as_ref()
            .map(|s| format!(" session={}", s))
            .unwrap_or_default();
        let error_info = self
            .error
            .as_ref()
            .map(|e| format!(" error=\"{}\"", e))
            .unwrap_or_default();

        format!(
            "{} [{}] {:?}{}{}{} - {}{}",
            self.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
            status,
            self.event_type,
            host_info,
            user_info,
            session_info,
            self.message,
            error_info
        )
    }
}

/// Encryption audit logger
pub struct EncryptionAuditLog {
    /// Path to log file
    log_path: PathBuf,
    /// Audit level
    level: AuditLevel,
    /// Whether to also log to tracing
    log_to_tracing: bool,
    /// File handle
    file: Arc<RwLock<Option<File>>>,
    /// Session ID for correlation
    session_id: String,
}

impl EncryptionAuditLog {
    /// Create a new audit log
    pub fn new(log_path: impl Into<PathBuf>) -> Self {
        Self {
            log_path: log_path.into(),
            level: AuditLevel::Standard,
            log_to_tracing: true,
            file: Arc::new(RwLock::new(None)),
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Set audit level
    pub fn with_level(mut self, level: AuditLevel) -> Self {
        self.level = level;
        self
    }

    /// Enable/disable tracing output
    pub fn with_log_to_tracing(mut self, enabled: bool) -> Self {
        self.log_to_tracing = enabled;
        self
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    /// Initialize the log file
    pub async fn init(&self) -> SecurityResult<()> {
        // Create parent directories if needed
        if let Some(parent) = self.log_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        let mut guard = self.file.write().await;
        *guard = Some(file);

        Ok(())
    }

    /// Log an event
    pub async fn log(&self, mut event: AuditEvent) -> SecurityResult<()> {
        // Add session ID if not already set
        if event.session_id.is_none() {
            event.session_id = Some(self.session_id.clone());
        }

        // Check if this event should be logged based on level
        if !self.should_log(&event) {
            return Ok(());
        }

        // Log to tracing if enabled
        if self.log_to_tracing {
            if event.success {
                info!(
                    event_type = ?event.event_type,
                    host = ?event.host,
                    message = %event.message,
                    "Encryption audit"
                );
            } else {
                warn!(
                    event_type = ?event.event_type,
                    host = ?event.host,
                    error = ?event.error,
                    message = %event.message,
                    "Encryption audit (failure)"
                );
            }
        }

        // Write to file
        let mut guard = self.file.write().await;
        if let Some(file) = guard.as_mut() {
            let line = format!("{}\n", event.to_log_line());
            file.write_all(line.as_bytes())?;
            file.flush()?;
        }

        Ok(())
    }

    /// Check if an event should be logged based on current level
    fn should_log(&self, event: &AuditEvent) -> bool {
        match self.level {
            AuditLevel::Minimal => !event.success, // Only failures
            AuditLevel::Standard => {
                // Connections, auth, and failures
                matches!(
                    event.event_type,
                    AuditEventType::ConnectionEstablished
                        | AuditEventType::ConnectionFailed
                        | AuditEventType::HostKeyRejected
                        | AuditEventType::HostKeyMismatch
                        | AuditEventType::TlsCertificateRejected
                        | AuditEventType::AuthenticationSucceeded
                        | AuditEventType::AuthenticationFailed
                        | AuditEventType::NetworkAccessDenied
                ) || !event.success
            }
            AuditLevel::Verbose => true, // Everything
        }
    }

    /// Log a connection attempt
    pub async fn log_connection_attempt(
        &self,
        host: &str,
        port: u16,
        user: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::ConnectionAttempt,
                format!("Attempting connection to {}@{}:{}", user, host, port),
            )
            .with_host(host)
            .with_port(port)
            .with_user(user),
        )
        .await
    }

    /// Log a successful connection
    pub async fn log_connection_established(
        &self,
        host: &str,
        port: u16,
        user: &str,
        cipher: &str,
        key_exchange: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::ConnectionEstablished,
                format!(
                    "Connection established to {}@{}:{} (cipher: {}, kex: {})",
                    user, host, port, cipher, key_exchange
                ),
            )
            .with_host(host)
            .with_port(port)
            .with_user(user)
            .with_metadata("cipher", cipher)
            .with_metadata("key_exchange", key_exchange),
        )
        .await
    }

    /// Log a failed connection
    pub async fn log_connection_failed(
        &self,
        host: &str,
        port: u16,
        error: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::ConnectionFailed,
                format!("Connection failed to {}:{}: {}", host, port, error),
            )
            .with_host(host)
            .with_port(port)
            .with_failure(error),
        )
        .await
    }

    /// Log host key verification
    pub async fn log_host_key_verified(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::HostKeyVerified,
                format!(
                    "Host key verified for {}:{} ({}: {})",
                    host, port, key_type, fingerprint
                ),
            )
            .with_host(host)
            .with_port(port)
            .with_metadata("key_type", key_type)
            .with_metadata("fingerprint", fingerprint),
        )
        .await
    }

    /// Log host key rejection
    pub async fn log_host_key_rejected(
        &self,
        host: &str,
        port: u16,
        reason: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::HostKeyRejected,
                format!("Host key rejected for {}:{}: {}", host, port, reason),
            )
            .with_host(host)
            .with_port(port)
            .with_failure(reason),
        )
        .await
    }

    /// Log host key mismatch (potential MITM)
    pub async fn log_host_key_mismatch(
        &self,
        host: &str,
        port: u16,
        expected: &str,
        actual: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::HostKeyMismatch,
                format!(
                    "HOST KEY MISMATCH for {}:{} - expected: {}, actual: {}",
                    host, port, expected, actual
                ),
            )
            .with_host(host)
            .with_port(port)
            .with_metadata("expected_fingerprint", expected)
            .with_metadata("actual_fingerprint", actual)
            .with_failure("Host key mismatch - possible MITM attack"),
        )
        .await
    }

    /// Log TLS certificate validation
    pub async fn log_tls_certificate_verified(&self, host: &str, port: u16) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::TlsCertificateVerified,
                format!("TLS certificate verified for {}:{}", host, port),
            )
            .with_host(host)
            .with_port(port),
        )
        .await
    }

    /// Log TLS certificate rejection
    pub async fn log_tls_certificate_rejected(
        &self,
        host: &str,
        port: u16,
        reason: &str,
    ) -> SecurityResult<()> {
        self.log(
            AuditEvent::new(
                AuditEventType::TlsCertificateRejected,
                format!("TLS certificate rejected for {}:{}: {}", host, port, reason),
            )
            .with_host(host)
            .with_port(port)
            .with_failure(reason),
        )
        .await
    }

    /// Log network access attempt
    pub async fn log_network_access(
        &self,
        host: &str,
        port: u16,
        allowed: bool,
        reason: Option<&str>,
    ) -> SecurityResult<()> {
        let event_type = if allowed {
            AuditEventType::NetworkAccessAllowed
        } else {
            AuditEventType::NetworkAccessDenied
        };

        let message = if allowed {
            format!("Network access allowed to {}:{}", host, port)
        } else {
            format!(
                "Network access denied to {}:{}: {}",
                host,
                port,
                reason.unwrap_or("policy violation")
            )
        };

        let mut event = AuditEvent::new(event_type, message)
            .with_host(host)
            .with_port(port);

        if !allowed {
            event = event.with_failure(reason.unwrap_or("policy violation"));
        }

        self.log(event).await
    }

    /// Flush and close the log
    pub async fn close(&self) -> SecurityResult<()> {
        let mut guard = self.file.write().await;
        if let Some(mut file) = guard.take() {
            file.flush()?;
        }
        Ok(())
    }
}

// ============================================================================
// Combined Security Configuration
// ============================================================================

/// Combined network security configuration
#[derive(Debug, Clone, Default)]
pub struct NetworkSecurityConfig {
    /// Host key verification policy
    pub host_key_policy: HostKeyPolicy,
    /// Jump host chain
    pub jump_hosts: JumpHostChain,
    /// Network isolation settings
    pub network_isolation: NetworkIsolation,
    /// TLS validation settings
    pub tls_config: TlsValidationConfig,
}

impl NetworkSecurityConfig {
    /// Create a new security configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a permissive configuration (for testing)
    pub fn permissive() -> Self {
        Self {
            host_key_policy: HostKeyPolicy::new().with_mode(HostKeyVerificationMode::AcceptAll),
            tls_config: TlsValidationConfig::permissive(),
            ..Self::default()
        }
    }

    /// Create a strict configuration (production recommended)
    pub fn strict() -> Self {
        Self {
            host_key_policy: HostKeyPolicy::new()
                .with_mode(HostKeyVerificationMode::KnownHostsOrPinned)
                .with_auto_add_hosts(false),
            network_isolation: NetworkIsolation::restrictive().allow_ssh().allow_http(),
            tls_config: TlsValidationConfig::new()
                .with_min_tls_version(TlsVersion::Tls12)
                .with_check_revocation(true),
            ..Self::default()
        }
    }

    /// Set host key policy
    pub fn with_host_key_policy(mut self, policy: HostKeyPolicy) -> Self {
        self.host_key_policy = policy;
        self
    }

    /// Set jump host chain
    pub fn with_jump_hosts(mut self, chain: JumpHostChain) -> Self {
        self.jump_hosts = chain;
        self
    }

    /// Set network isolation
    pub fn with_network_isolation(mut self, isolation: NetworkIsolation) -> Self {
        self.network_isolation = isolation;
        self
    }

    /// Set TLS configuration
    pub fn with_tls_config(mut self, config: TlsValidationConfig) -> Self {
        self.tls_config = config;
        self
    }

    /// Add a jump host
    pub fn add_jump_host(mut self, jump_host: JumpHostConfig) -> Self {
        self.jump_hosts = self.jump_hosts.add_jump(jump_host);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_key_policy_default() {
        let policy = HostKeyPolicy::new();
        assert!(matches!(
            policy.mode,
            HostKeyVerificationMode::TrustOnFirstUse
        ));
        assert!(policy.auto_add_hosts);
    }

    #[test]
    fn test_pinned_host_key_matching() {
        let pin = PinnedHostKey::new("*.example.com", "SHA256:abc123")
            .with_port(22)
            .with_key_type("ssh-ed25519");

        assert!(pin.matches_host("server.example.com", 22));
        assert!(pin.matches_host("api.example.com", 22));
        assert!(!pin.matches_host("server.example.com", 2222));
        assert!(!pin.matches_host("other.domain.com", 22));
    }

    #[test]
    fn test_fingerprint_normalization() {
        assert_eq!(
            normalize_fingerprint("SHA256:abcDEF123"),
            normalize_fingerprint("abcdef123")
        );
        assert_eq!(
            normalize_fingerprint("MD5:aa:bb:cc:dd"),
            normalize_fingerprint("aabbccdd")
        );
    }

    #[test]
    fn test_jump_host_chain() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("bastion1.example.com").user("jump1"))
            .add_jump(JumpHostConfig::new("bastion2.example.com").port(2222));

        assert_eq!(chain.len(), 2);
        assert_eq!(
            chain.to_string(),
            "jump1@bastion1.example.com,bastion2.example.com:2222"
        );
    }

    #[test]
    fn test_jump_host_chain_parse() {
        let chain = JumpHostChain::parse("user@host1:22,host2:2222,user2@host3:22").unwrap();
        assert_eq!(chain.len(), 3);
        // Access via iteration since jumps is private
        let jumps: Vec<_> = chain.iter().collect();
        assert_eq!(jumps[0].user, Some("user".to_string()));
        assert_eq!(jumps[1].port, 2222);
        assert_eq!(jumps[2].user, Some("user2".to_string()));
    }

    #[test]
    fn test_network_isolation_allow_all() {
        let isolation = NetworkIsolation::new();
        assert!(isolation.is_allowed("example.com", 443).is_ok());
        assert!(isolation.is_allowed("192.168.1.1", 22).is_ok());
    }

    #[test]
    fn test_network_isolation_restrictive() {
        let isolation = NetworkIsolation::restrictive()
            .allow_host("192.168.1.0/24")
            .allow_port(22);

        assert!(isolation.is_allowed("192.168.1.100", 22).is_ok());
        assert!(isolation.is_allowed("10.0.0.1", 22).is_err());
        assert!(isolation.is_allowed("192.168.1.100", 80).is_err());
    }

    #[test]
    fn test_network_isolation_deny_takes_precedence() {
        let isolation = NetworkIsolation::new()
            .allow_host("192.168.0.0/16")
            .deny_host("192.168.1.100");

        assert!(isolation.is_allowed("192.168.2.1", 22).is_ok());
        assert!(isolation.is_allowed("192.168.1.100", 22).is_err());
    }

    #[test]
    fn test_tls_config_default() {
        let config = TlsValidationConfig::new();
        assert!(config.require_valid_cert);
        assert!(config.verify_hostname);
        assert!(matches!(config.min_tls_version, TlsVersion::Tls12));
    }

    #[test]
    fn test_tls_config_permissive() {
        let config = TlsValidationConfig::permissive();
        assert!(!config.require_valid_cert);
        assert!(!config.verify_hostname);
    }

    #[test]
    fn test_audit_event_formatting() {
        let event = AuditEvent::new(AuditEventType::ConnectionEstablished, "Test connection")
            .with_host("example.com")
            .with_port(22)
            .with_user("testuser");

        let line = event.to_log_line();
        assert!(line.contains("ConnectionEstablished"));
        assert!(line.contains("example.com:22"));
        assert!(line.contains("testuser"));
        assert!(line.contains("OK"));
    }

    #[test]
    fn test_audit_event_failure() {
        let event = AuditEvent::new(AuditEventType::ConnectionFailed, "Connection refused")
            .with_host("example.com")
            .with_failure("Connection refused");

        let line = event.to_log_line();
        assert!(line.contains("FAIL"));
        assert!(line.contains("Connection refused"));
    }

    #[test]
    fn test_ip_in_cidr_v4() {
        let network: IpAddr = "192.168.1.0".parse().unwrap();
        let ip1: IpAddr = "192.168.1.100".parse().unwrap();
        let ip2: IpAddr = "192.168.2.100".parse().unwrap();

        assert!(ip_in_cidr(ip1, network, 24));
        assert!(!ip_in_cidr(ip2, network, 24));
    }

    #[test]
    fn test_security_config_strict() {
        let config = NetworkSecurityConfig::strict();
        assert!(matches!(
            config.host_key_policy.mode,
            HostKeyVerificationMode::KnownHostsOrPinned
        ));
        assert!(!config.host_key_policy.auto_add_hosts);
        assert!(!config.network_isolation.allow_all_outbound);
    }
}
