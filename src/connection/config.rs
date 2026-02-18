//! Connection configuration module
//!
//! This module handles SSH config parsing, host-specific settings,
//! timeout configuration, and retry logic.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ConnectionError;

/// Default connection timeout in seconds
pub const DEFAULT_TIMEOUT: u64 = 30;

/// Default number of connection retries
pub const DEFAULT_RETRIES: u32 = 3;

/// Default delay between retries in seconds
pub const DEFAULT_RETRY_DELAY: u64 = 1;

/// Main connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Default settings for all connections
    #[serde(default)]
    pub defaults: ConnectionDefaults,

    /// Host-specific configurations
    #[serde(default)]
    pub hosts: HashMap<String, HostConfig>,

    /// SSH config file path (default: ~/.ssh/config)
    #[serde(default)]
    pub ssh_config_path: Option<PathBuf>,

    /// Whether to parse SSH config file
    #[serde(default = "default_true")]
    pub parse_ssh_config: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            defaults: ConnectionDefaults::default(),
            hosts: HashMap::new(),
            ssh_config_path: None,
            parse_ssh_config: true,
        }
    }
}

impl ConnectionConfig {
    /// Create a new connection config
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConnectionError> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            ConnectionError::InvalidConfig(format!("Failed to read config file: {}", e))
        })?;
        Self::from_toml(&content)
    }

    /// Parse configuration from TOML string
    pub fn from_toml(content: &str) -> Result<Self, ConnectionError> {
        toml::from_str(content)
            .map_err(|e| ConnectionError::InvalidConfig(format!("Failed to parse config: {}", e)))
    }

    /// Load and merge SSH config
    pub fn load_ssh_config(&mut self) -> Result<(), ConnectionError> {
        if !self.parse_ssh_config {
            return Ok(());
        }

        let ssh_config_path = self.ssh_config_path.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".ssh")
                .join("config")
        });

        if ssh_config_path.exists() {
            let parsed = SshConfigParser::parse_file(&ssh_config_path)?;
            for (host, config) in parsed {
                // Only add if not already defined
                self.hosts.entry(host).or_insert(config);
            }
        }

        Ok(())
    }

    /// Get configuration for a specific host
    pub fn get_host(&self, host: &str) -> Option<&HostConfig> {
        // First try exact match
        if let Some(config) = self.hosts.get(host) {
            return Some(config);
        }

        // Then try pattern matching
        for (pattern, config) in &self.hosts {
            if (pattern.contains('*') || pattern.contains('?'))
                && matches_pattern(pattern, host) {
                    return Some(config);
                }
        }

        None
    }

    /// Get configuration for a host, with defaults merged
    pub fn get_host_merged(&self, host: &str) -> HostConfig {
        let mut config = self.get_host(host).cloned().unwrap_or_default();

        // Merge with defaults
        if config.user.is_none() {
            config.user = Some(self.defaults.user.clone());
        }
        if config.port.is_none() {
            config.port = Some(self.defaults.port);
        }
        if config.connect_timeout.is_none() {
            config.connect_timeout = Some(self.defaults.timeout);
        }
        if config.retries.is_none() {
            config.retries = Some(self.defaults.retries);
        }
        if config.identity_file.is_none() && !self.defaults.identity_files.is_empty() {
            config.identity_file = self.defaults.identity_files.first().cloned();
        }

        config
    }

    /// Add a host configuration
    pub fn add_host(&mut self, name: impl Into<String>, config: HostConfig) {
        self.hosts.insert(name.into(), config);
    }

    /// Set default user
    pub fn set_default_user(&mut self, user: impl Into<String>) {
        self.defaults.user = user.into();
    }

    /// Set default port
    pub fn set_default_port(&mut self, port: u16) {
        self.defaults.port = port;
    }

    /// Set default timeout
    pub fn set_default_timeout(&mut self, timeout: u64) {
        self.defaults.timeout = timeout;
    }
}

/// Default connection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// Default username for connections
    #[serde(default = "default_user")]
    pub user: String,

    /// Default port for SSH connections
    #[serde(default = "default_port")]
    pub port: u16,

    /// Default connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Default number of connection retries
    #[serde(default = "default_retries")]
    pub retries: u32,

    /// Delay between retries in seconds
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,

    /// Default identity files (private keys) to try
    #[serde(default)]
    pub identity_files: Vec<String>,

    /// Use SSH agent for authentication
    #[serde(default = "default_true")]
    pub use_agent: bool,

    /// Verify host keys
    #[serde(default = "default_true")]
    pub verify_host_key: bool,

    /// Known hosts file path
    #[serde(default)]
    pub known_hosts_file: Option<PathBuf>,
}

fn default_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

fn default_port() -> u16 {
    22
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT
}

fn default_retries() -> u32 {
    DEFAULT_RETRIES
}

fn default_retry_delay() -> u64 {
    DEFAULT_RETRY_DELAY
}

impl Default for ConnectionDefaults {
    fn default() -> Self {
        Self {
            user: default_user(),
            port: 22,
            timeout: DEFAULT_TIMEOUT,
            retries: DEFAULT_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
            identity_files: vec![],
            use_agent: true,
            verify_host_key: true,
            known_hosts_file: None,
        }
    }
}

/// Host-specific configuration
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct HostConfig {
    /// Actual hostname or IP address
    pub hostname: Option<String>,

    /// Port to connect to
    pub port: Option<u16>,

    /// Username for authentication
    pub user: Option<String>,

    /// Path to private key file
    pub identity_file: Option<String>,

    /// Password for authentication (not recommended)
    #[serde(skip_serializing)]
    pub password: Option<String>,

    /// Connection timeout in seconds
    pub connect_timeout: Option<u64>,

    /// Number of connection retries
    pub retries: Option<u32>,

    /// Retry delay in seconds
    pub retry_delay: Option<u64>,

    /// Connection type (ssh, local, docker)
    pub connection: Option<String>,

    /// Proxy/jump host
    pub proxy_jump: Option<String>,

    /// Forward agent
    #[serde(default)]
    pub forward_agent: bool,

    /// Compression
    #[serde(default)]
    pub compression: bool,

    /// Server alive interval (seconds)
    pub server_alive_interval: Option<u64>,

    /// Server alive count max
    pub server_alive_count_max: Option<u32>,

    /// Strict host key checking
    pub strict_host_key_checking: Option<bool>,

    /// User known hosts file
    pub user_known_hosts_file: Option<String>,

    /// Extra SSH options
    #[serde(default)]
    pub options: HashMap<String, String>,
}

impl std::fmt::Debug for HostConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostConfig")
            .field("hostname", &self.hostname)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("identity_file", &self.identity_file)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("connect_timeout", &self.connect_timeout)
            .field("retries", &self.retries)
            .field("retry_delay", &self.retry_delay)
            .field("connection", &self.connection)
            .field("proxy_jump", &self.proxy_jump)
            .field("forward_agent", &self.forward_agent)
            .field("compression", &self.compression)
            .field("server_alive_interval", &self.server_alive_interval)
            .field("server_alive_count_max", &self.server_alive_count_max)
            .field("strict_host_key_checking", &self.strict_host_key_checking)
            .field("user_known_hosts_file", &self.user_known_hosts_file)
            .field("options", &self.options)
            .finish()
    }
}

impl HostConfig {
    /// Create a new host config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set hostname
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Set port
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set user
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set identity file
    pub fn identity_file(mut self, path: impl Into<String>) -> Self {
        self.identity_file = Some(path.into());
        self
    }

    /// Set connection timeout
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Enable/disable compression
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Set connection type
    pub fn connection_type(mut self, conn_type: impl Into<String>) -> Self {
        self.connection = Some(conn_type.into());
        self
    }

    /// Get the connection timeout as Duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.connect_timeout.unwrap_or(DEFAULT_TIMEOUT))
    }

    /// Get retry configuration
    pub fn retry_config(&self) -> RetryConfig {
        RetryConfig {
            max_retries: self.retries.unwrap_or(DEFAULT_RETRIES),
            retry_delay: Duration::from_secs(self.retry_delay.unwrap_or(DEFAULT_RETRY_DELAY)),
            exponential_backoff: true,
            max_delay: Duration::from_secs(30),
        }
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,

    /// Initial delay between retries
    pub retry_delay: Duration,

    /// Use exponential backoff
    pub exponential_backoff: bool,

    /// Maximum delay between retries
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_RETRIES,
            retry_delay: Duration::from_secs(DEFAULT_RETRY_DELAY),
            exponential_backoff: true,
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryConfig {
    /// Calculate delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if self.exponential_backoff {
            let delay = self.retry_delay * 2u32.pow(attempt.min(10));
            delay.min(self.max_delay)
        } else {
            self.retry_delay
        }
    }
}

/// SSH config file parser
pub struct SshConfigParser;

impl SshConfigParser {
    /// Parse an SSH config file
    pub fn parse_file(
        path: impl AsRef<Path>,
    ) -> Result<HashMap<String, HostConfig>, ConnectionError> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            ConnectionError::InvalidConfig(format!("Failed to read SSH config: {}", e))
        })?;
        Self::parse(&content)
    }

    /// Parse SSH config content
    pub fn parse(content: &str) -> Result<HashMap<String, HostConfig>, ConnectionError> {
        let mut hosts: HashMap<String, HostConfig> = HashMap::new();
        let mut current_hosts: Vec<String> = Vec::new();
        let mut current_config = HostConfig::default();

        // Regex for matching Host directive
        static HOST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*Host\s+(.+)$").unwrap());
        // Regex for matching key-value pairs
        static KV_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\w+)\s+(.+)$").unwrap());

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Check for Host directive
            if let Some(captures) = HOST_RE.captures(line) {
                // Save previous host config
                if !current_hosts.is_empty() {
                    for host in &current_hosts {
                        hosts.insert(host.clone(), current_config.clone());
                    }
                }

                // Start new host(s)
                let hosts_str = captures.get(1).unwrap().as_str();
                current_hosts = hosts_str.split_whitespace().map(String::from).collect();
                current_config = HostConfig::default();
                continue;
            }

            // Parse key-value pairs
            if let Some(captures) = KV_RE.captures(line) {
                let key = captures.get(1).unwrap().as_str().to_lowercase();
                let value = captures.get(2).unwrap().as_str().trim().to_string();

                // Remove quotes if present
                let value = value.trim_matches('"').to_string();

                match key.as_str() {
                    "hostname" => current_config.hostname = Some(value),
                    "port" => {
                        current_config.port = value.parse().ok();
                    }
                    "user" => current_config.user = Some(value),
                    "identityfile" => {
                        // Expand ~ in path
                        let expanded = shellexpand::tilde(&value).to_string();
                        current_config.identity_file = Some(expanded);
                    }
                    "connecttimeout" => {
                        current_config.connect_timeout = value.parse().ok();
                    }
                    "proxyjump" => current_config.proxy_jump = Some(value),
                    "forwardagent" => {
                        current_config.forward_agent = value.to_lowercase() == "yes";
                    }
                    "compression" => {
                        current_config.compression = value.to_lowercase() == "yes";
                    }
                    "serveraliveinterval" => {
                        current_config.server_alive_interval = value.parse().ok();
                    }
                    "serveralivecountmax" => {
                        current_config.server_alive_count_max = value.parse().ok();
                    }
                    "stricthostkeychecking" => {
                        current_config.strict_host_key_checking =
                            match value.to_lowercase().as_str() {
                                "yes" => Some(true),
                                "no" => Some(false),
                                _ => None,
                            };
                    }
                    "userknownhostsfile" => {
                        current_config.user_known_hosts_file = Some(value);
                    }
                    _ => {
                        // Store unknown options
                        current_config.options.insert(key, value);
                    }
                }
            }
        }

        // Save last host config
        if !current_hosts.is_empty() {
            for host in &current_hosts {
                hosts.insert(host.clone(), current_config.clone());
            }
        }

        Ok(hosts)
    }
}

/// Match a host against a pattern (supports * and ?)
fn matches_pattern(pattern: &str, host: &str) -> bool {
    // Fast path for wildcard
    if pattern == "*" {
        return true;
    }

    // Convert glob pattern to regex
    let regex_pattern = pattern
        .replace('.', r"\.")
        .replace('*', ".*")
        .replace('?', ".");

    if let Ok(re) = Regex::new(&format!("^{}$", regex_pattern)) {
        re.is_match(host)
    } else {
        false
    }
}

/// Helper to expand paths with ~ and environment variables
pub fn expand_path(path: &str) -> PathBuf {
    let expanded = shellexpand::full(path).unwrap_or_else(|_| path.into());
    PathBuf::from(expanded.as_ref())
}

/// Get default identity files to try
pub fn default_identity_files() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
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

    #[test]
    fn test_default_config() {
        let config = ConnectionConfig::default();
        assert_eq!(config.defaults.port, 22);
        assert_eq!(config.defaults.timeout, DEFAULT_TIMEOUT);
        assert_eq!(config.defaults.retries, DEFAULT_RETRIES);
    }

    #[test]
    fn test_host_config_builder() {
        let config = HostConfig::new()
            .hostname("example.com")
            .port(2222)
            .user("admin")
            .timeout(60);

        assert_eq!(config.hostname, Some("example.com".to_string()));
        assert_eq!(config.port, Some(2222));
        assert_eq!(config.user, Some("admin".to_string()));
        assert_eq!(config.connect_timeout, Some(60));
    }

    #[test]
    fn test_ssh_config_parsing() {
        let config = r#"
Host example
    HostName example.com
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa

Host *.internal
    User internal
    ProxyJump bastion
"#;

        let hosts = SshConfigParser::parse(config).unwrap();

        let example = hosts.get("example").unwrap();
        assert_eq!(example.hostname, Some("example.com".to_string()));
        assert_eq!(example.port, Some(2222));
        assert_eq!(example.user, Some("admin".to_string()));

        let internal = hosts.get("*.internal").unwrap();
        assert_eq!(internal.user, Some("internal".to_string()));
        assert_eq!(internal.proxy_jump, Some("bastion".to_string()));
    }

    #[test]
    fn test_pattern_matching() {
        assert!(matches_pattern("*.example.com", "server.example.com"));
        assert!(matches_pattern("web-?", "web-1"));
        assert!(!matches_pattern("*.example.com", "example.com"));
        assert!(matches_pattern("*", "anything"));
    }

    #[test]
    fn test_retry_config_delay() {
        let config = RetryConfig::default();

        let delay0 = config.delay_for_attempt(0);
        let delay1 = config.delay_for_attempt(1);
        let delay2 = config.delay_for_attempt(2);

        assert!(delay1 > delay0);
        assert!(delay2 > delay1);
    }

    #[test]
    fn test_config_from_toml() {
        let toml = r#"
[defaults]
user = "admin"
port = 22
timeout = 60

[hosts.webserver]
hostname = "192.168.1.100"
port = 2222
user = "web"
"#;

        let config = ConnectionConfig::from_toml(toml).unwrap();
        assert_eq!(config.defaults.user, "admin");
        assert_eq!(config.defaults.timeout, 60);

        let webserver = config.get_host("webserver").unwrap();
        assert_eq!(webserver.hostname, Some("192.168.1.100".to_string()));
        assert_eq!(webserver.port, Some(2222));
    }
}
