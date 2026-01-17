//! Configuration module for Rustible
//!
//! Handles loading and merging configuration from multiple sources:
//! - Default values
//! - System configuration (/etc/rustible/rustible.cfg)
//! - User configuration (~/.rustible.cfg)
//! - Project configuration (./rustible.cfg)
//! - Environment variables
//! - Command-line arguments

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Configuration Merge Macros
// ============================================================================
// These macros simplify the repetitive field merging logic when combining
// configuration values from multiple sources.

/// Merge an Option<T> field - uses `other` if Some, otherwise falls back to `base`
///
/// # Example
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # macro_rules! merge_option {
/// #     ($base:expr, $other:expr) => {
/// #         $other.or_else(|| $base.clone())
/// #     };
/// # }
/// # let base = Some("inventory.yml".to_string());
/// # let other: Option<String> = None;
/// let merged = merge_option!(base, other);
/// # let _ = merged;
/// # Ok(())
/// # }
/// ```
macro_rules! merge_option {
    ($base:expr, $other:expr) => {
        $other.or_else(|| $base.clone())
    };
}

/// Merge a field with a known default value - uses `other` if non-default, otherwise `base`
///
/// # Example
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # macro_rules! merge_with_default {
/// #     ($base:expr, $other:expr, $default:expr) => {
/// #         if $other != $default {
/// #             $other
/// #         } else {
/// #             $base.clone()
/// #         }
/// #     };
/// # }
/// # let base = 10;
/// # let other = 5;
/// let merged = merge_with_default!(base, other, 5);
/// # let _ = merged;
/// # Ok(())
/// # }
/// ```
macro_rules! merge_with_default {
    ($base:expr, $other:expr, $default:expr) => {
        if $other != $default {
            $other
        } else {
            $base.clone()
        }
    };
}

/// Merge a Vec field - uses `other` if non-empty, otherwise `base`
///
/// # Example
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # macro_rules! merge_vec {
/// #     ($base:expr, $other:expr) => {
/// #         if $other.is_empty() {
/// #             $base.clone()
/// #         } else {
/// #             $other
/// #         }
/// #     };
/// # }
/// # let base: Vec<String> = vec!["/etc/rustible/roles".to_string()];
/// # let other: Vec<String> = Vec::new();
/// let merged = merge_vec!(base, other);
/// # let _ = merged;
/// # Ok(())
/// # }
/// ```
macro_rules! merge_vec {
    ($base:expr, $other:expr) => {
        if $other.is_empty() {
            $base.clone()
        } else {
            $other
        }
    };
}

/// Merge a HashMap field - extends `base` with entries from `other`
///
/// # Example
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # use std::collections::HashMap;
/// # macro_rules! merge_hashmap {
/// #     ($base:expr, $other:expr) => {{
/// #         let mut merged = $base.clone();
/// #         merged.extend($other);
/// #         merged
/// #     }};
/// # }
/// # let base: HashMap<String, String> = HashMap::new();
/// # let other: HashMap<String, String> = HashMap::new();
/// let merged = merge_hashmap!(base, other);
/// # let _ = merged;
/// # Ok(())
/// # }
/// ```
macro_rules! merge_hashmap {
    ($base:expr, $other:expr) => {{
        let mut merged = $base.clone();
        merged.extend($other);
        merged
    }};
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    /// Default settings
    pub defaults: Defaults,

    /// Connection settings
    pub connection: ConnectionConfig,

    /// Privilege escalation settings
    pub privilege_escalation: PrivilegeEscalation,

    /// SSH settings
    pub ssh: SshConfig,

    /// Colors and output settings
    pub colors: ColorsConfig,

    /// Logging settings
    pub logging: LoggingConfig,

    /// Vault settings
    pub vault: VaultConfig,

    /// Galaxy settings (for roles/collections)
    pub galaxy: GalaxyConfig,

    /// Custom module paths
    #[serde(default)]
    pub module_paths: Vec<PathBuf>,

    /// Custom role paths
    #[serde(default)]
    pub role_paths: Vec<PathBuf>,

    /// Environment variables to pass to modules
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

/// Default configuration values
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    /// Default inventory path
    pub inventory: Option<PathBuf>,

    /// Default remote user
    pub remote_user: Option<String>,

    /// Default number of forks (parallel processes)
    pub forks: usize,

    /// Default module
    pub module_name: String,

    /// Default host key checking
    pub host_key_checking: bool,

    /// Default timeout
    pub timeout: u64,

    /// Gather facts by default
    pub gathering: bool,

    /// Default transport/connection type
    pub transport: String,

    /// Hash behavior (replace or merge)
    pub hash_behaviour: String,

    /// Retry files path
    pub retry_files_enabled: bool,

    /// Retry files save path
    pub retry_files_save_path: Option<PathBuf>,

    /// Roles path
    pub roles_path: Vec<PathBuf>,

    /// Collections path
    pub collections_path: Vec<PathBuf>,

    /// Action plugins path
    pub action_plugins: Vec<PathBuf>,

    /// Strategy plugins path
    pub strategy_plugins: Vec<PathBuf>,

    /// Default strategy
    pub strategy: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            inventory: None,
            remote_user: None,
            forks: 5,
            module_name: "command".to_string(),
            host_key_checking: true,
            timeout: 30,
            gathering: true,
            transport: "ssh".to_string(),
            hash_behaviour: "replace".to_string(),
            retry_files_enabled: true,
            retry_files_save_path: None,
            roles_path: vec![PathBuf::from("./roles")],
            collections_path: vec![],
            action_plugins: vec![],
            strategy_plugins: vec![],
            strategy: "linear".to_string(),
        }
    }
}

/// Connection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionConfig {
    /// Pipelining (improves SSH performance)
    pub pipelining: bool,

    /// Control path for SSH multiplexing
    pub control_path: Option<String>,

    /// Control master persistence
    pub control_master: String,

    /// Control persist timeout
    pub control_persist: u64,

    /// SSH executable
    pub ssh_executable: String,

    /// SCP if SSH transfer fails
    pub scp_if_ssh: bool,

    /// SFTP batch mode
    pub sftp_batch_mode: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            pipelining: true,
            control_path: Some("~/.rustible/cp/%r@%h:%p".to_string()),
            control_master: "auto".to_string(),
            control_persist: 60,
            ssh_executable: "ssh".to_string(),
            scp_if_ssh: false,
            sftp_batch_mode: true,
        }
    }
}

/// Privilege escalation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivilegeEscalation {
    /// Enable become by default
    pub r#become: bool,

    /// Default become method
    pub become_method: String,

    /// Default become user
    pub become_user: String,

    /// Ask for become password
    pub become_ask_pass: bool,

    /// Become flags
    pub become_flags: Option<String>,
}

impl Default for PrivilegeEscalation {
    fn default() -> Self {
        Self {
            r#become: false,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        }
    }
}

/// SSH configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SshConfig {
    /// SSH arguments
    pub ssh_args: Vec<String>,

    /// SSH common args
    pub ssh_common_args: Vec<String>,

    /// SSH extra args
    pub ssh_extra_args: Vec<String>,

    /// SCP extra args
    pub scp_extra_args: Vec<String>,

    /// SFTP extra args
    pub sftp_extra_args: Vec<String>,

    /// SSH retries
    pub retries: u32,

    /// Private key file
    pub private_key_file: Option<PathBuf>,

    /// Known hosts file
    pub known_hosts_file: Option<PathBuf>,

    /// Control path for multiplexing
    pub control_path: Option<String>,

    /// Enable pipelining
    pub pipelining: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            ssh_args: vec![
                "-o".to_string(),
                "ControlMaster=auto".to_string(),
                "-o".to_string(),
                "ControlPersist=60s".to_string(),
            ],
            ssh_common_args: vec![],
            ssh_extra_args: vec![],
            scp_extra_args: vec![],
            sftp_extra_args: vec![],
            retries: 3,
            private_key_file: None,
            known_hosts_file: None,
            control_path: None,
            pipelining: true,
        }
    }
}

/// Colors configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    /// Enable colors
    pub enabled: bool,

    /// Highlight color
    pub highlight: String,

    /// Verbose color
    pub verbose: String,

    /// Warning color
    pub warn: String,

    /// Error color
    pub error: String,

    /// Debug color
    pub debug: String,

    /// OK color
    pub ok: String,

    /// Changed color
    pub changed: String,

    /// Unreachable color
    pub unreachable: String,

    /// Skipped color
    pub skipped: String,

    /// Diff add color
    pub diff_add: String,

    /// Diff remove color
    pub diff_remove: String,

    /// Diff lines color
    pub diff_lines: String,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            highlight: "white".to_string(),
            verbose: "blue".to_string(),
            warn: "bright_purple".to_string(),
            error: "red".to_string(),
            debug: "dark_gray".to_string(),
            ok: "green".to_string(),
            changed: "yellow".to_string(),
            unreachable: "bright_red".to_string(),
            skipped: "cyan".to_string(),
            diff_add: "green".to_string(),
            diff_remove: "red".to_string(),
            diff_lines: "cyan".to_string(),
        }
    }
}

/// Logging settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log path
    pub log_path: Option<PathBuf>,

    /// Log level
    pub log_level: String,

    /// Log format
    pub log_format: String,

    /// Log timestamp
    pub log_timestamp: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_path: None,
            log_level: "info".to_string(),
            log_format: "%(asctime)s - %(name)s - %(levelname)s - %(message)s".to_string(),
            log_timestamp: true,
        }
    }
}

/// Vault settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct VaultConfig {
    /// Vault password file
    pub password_file: Option<PathBuf>,

    /// Vault identity list
    pub identity_list: Vec<String>,

    /// Encrypt vault id
    pub encrypt_vault_id: Option<String>,
}

/// Galaxy settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GalaxyConfig {
    /// Galaxy server URL
    pub server: String,

    /// Galaxy server list
    pub server_list: Vec<GalaxyServer>,

    /// Cache path
    pub cache_dir: Option<PathBuf>,

    /// Collections installation path
    pub collections_path: Option<PathBuf>,

    /// Roles installation path
    pub roles_path: Option<PathBuf>,

    /// Ignore certs
    pub ignore_certs: bool,
}

impl Default for GalaxyConfig {
    fn default() -> Self {
        Self {
            server: "https://galaxy.ansible.com".to_string(),
            server_list: vec![],
            cache_dir: None,
            collections_path: None,
            roles_path: None,
            ignore_certs: false,
        }
    }
}

/// Galaxy server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalaxyServer {
    /// Server name
    pub name: String,
    /// Server URL
    pub url: String,
    /// Auth token
    pub token: Option<String>,
}

impl Config {
    /// Load configuration from all sources
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        let mut config = Config::default();

        // Load from standard locations
        let config_paths = Self::get_config_paths(config_path);

        for path in config_paths {
            if path.exists() {
                config = config.merge_from_file(&path)?;
            }
        }

        // Apply environment variable overrides
        config.apply_env_overrides();

        Ok(config)
    }

    /// Get the list of configuration file paths to check
    fn get_config_paths(explicit_path: Option<&PathBuf>) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Explicit path takes priority
        if let Some(path) = explicit_path {
            paths.push(path.clone());
            return paths;
        }

        // System-wide config
        paths.push(PathBuf::from("/etc/rustible/rustible.cfg"));

        // User config
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".rustible.cfg"));
            paths.push(home.join(".rustible/rustible.cfg"));
            paths.push(home.join(".rustible/config"));
        }

        // Project config (current directory)
        paths.push(PathBuf::from("rustible.cfg"));
        paths.push(PathBuf::from(".rustible.cfg"));

        // Environment variable
        if let Ok(env_config) = std::env::var("RUSTIBLE_CONFIG") {
            paths.insert(0, PathBuf::from(env_config));
        }

        paths
    }

    /// Merge configuration from a file
    fn merge_from_file(&self, path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        // Determine format based on extension
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let file_config: Config = match extension {
            "yml" | "yaml" => serde_yaml::from_str(&content)?,
            "json" => serde_json::from_str(&content)?,
            "toml" => toml::from_str(&content)?,
            _ => {
                // Try TOML first (for .cfg files), then YAML
                toml::from_str(&content)
                    .or_else(|_| serde_yaml::from_str(&content))
                    .with_context(|| format!("Failed to parse config file: {}", path.display()))?
            }
        };

        Ok(self.merge(file_config))
    }

    /// Merge another config into this one
    ///
    /// Uses merge macros for cleaner, declarative field merging.
    /// Priority: `other` takes precedence for non-default values.
    fn merge(&self, other: Config) -> Config {
        Config {
            defaults: Defaults {
                // Option fields: use other if Some, otherwise base
                inventory: merge_option!(self.defaults.inventory, other.defaults.inventory),
                remote_user: merge_option!(self.defaults.remote_user, other.defaults.remote_user),
                retry_files_save_path: merge_option!(
                    self.defaults.retry_files_save_path,
                    other.defaults.retry_files_save_path
                ),

                // Fields with known defaults: use other if different from default
                forks: merge_with_default!(self.defaults.forks, other.defaults.forks, 5),
                module_name: merge_with_default!(
                    self.defaults.module_name,
                    other.defaults.module_name,
                    "command"
                ),
                timeout: merge_with_default!(self.defaults.timeout, other.defaults.timeout, 30),
                transport: merge_with_default!(
                    self.defaults.transport,
                    other.defaults.transport,
                    "ssh"
                ),

                // Vec fields: use other if non-empty
                roles_path: merge_vec!(self.defaults.roles_path, other.defaults.roles_path),
                collections_path: merge_vec!(
                    self.defaults.collections_path,
                    other.defaults.collections_path
                ),
                action_plugins: merge_vec!(
                    self.defaults.action_plugins,
                    other.defaults.action_plugins
                ),
                strategy_plugins: merge_vec!(
                    self.defaults.strategy_plugins,
                    other.defaults.strategy_plugins
                ),

                // Direct override fields: other always wins
                host_key_checking: other.defaults.host_key_checking,
                gathering: other.defaults.gathering,
                hash_behaviour: other.defaults.hash_behaviour,
                retry_files_enabled: other.defaults.retry_files_enabled,
                strategy: other.defaults.strategy,
            },

            // Nested config sections: direct override (could be extended with sub-merging)
            connection: other.connection,
            privilege_escalation: other.privilege_escalation,
            ssh: other.ssh,
            colors: other.colors,
            logging: other.logging,

            // Vault config: merge individual fields
            vault: VaultConfig {
                password_file: merge_option!(self.vault.password_file, other.vault.password_file),
                identity_list: merge_vec!(self.vault.identity_list, other.vault.identity_list),
                encrypt_vault_id: merge_option!(
                    self.vault.encrypt_vault_id,
                    other.vault.encrypt_vault_id
                ),
            },

            galaxy: other.galaxy,

            // Top-level collections
            module_paths: merge_vec!(self.module_paths, other.module_paths),
            role_paths: merge_vec!(self.role_paths, other.role_paths),
            environment: merge_hashmap!(self.environment, other.environment),
        }
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(&mut self) {
        // RUSTIBLE_FORKS
        if let Ok(forks) = std::env::var("RUSTIBLE_FORKS") {
            if let Ok(n) = forks.parse() {
                self.defaults.forks = n;
            }
        }

        // RUSTIBLE_TIMEOUT
        if let Ok(timeout) = std::env::var("RUSTIBLE_TIMEOUT") {
            if let Ok(n) = timeout.parse() {
                self.defaults.timeout = n;
            }
        }

        // RUSTIBLE_REMOTE_USER
        if let Ok(user) = std::env::var("RUSTIBLE_REMOTE_USER") {
            self.defaults.remote_user = Some(user);
        }

        // RUSTIBLE_BECOME
        if std::env::var("RUSTIBLE_BECOME").is_ok() {
            self.privilege_escalation.r#become = true;
        }

        // RUSTIBLE_BECOME_METHOD
        if let Ok(method) = std::env::var("RUSTIBLE_BECOME_METHOD") {
            self.privilege_escalation.become_method = method;
        }

        // RUSTIBLE_BECOME_USER
        if let Ok(user) = std::env::var("RUSTIBLE_BECOME_USER") {
            self.privilege_escalation.become_user = user;
        }

        // RUSTIBLE_VAULT_PASSWORD_FILE
        if let Ok(file) = std::env::var("RUSTIBLE_VAULT_PASSWORD_FILE") {
            self.vault.password_file = Some(PathBuf::from(file));
        }

        // RUSTIBLE_SSH_ARGS
        if let Ok(args) = std::env::var("RUSTIBLE_SSH_ARGS") {
            self.ssh.ssh_args = args.split_whitespace().map(String::from).collect();
        }

        // RUSTIBLE_PRIVATE_KEY_FILE
        if let Ok(file) = std::env::var("RUSTIBLE_PRIVATE_KEY_FILE") {
            self.ssh.private_key_file = Some(PathBuf::from(file));
        }

        // NO_COLOR
        if std::env::var("NO_COLOR").is_ok() || std::env::var("RUSTIBLE_NO_COLOR").is_ok() {
            self.colors.enabled = false;
        }

        // RUSTIBLE_LOG_PATH
        if let Ok(path) = std::env::var("RUSTIBLE_LOG_PATH") {
            self.logging.log_path = Some(PathBuf::from(path));
        }

        // RUSTIBLE_STRATEGY
        if let Ok(strategy) = std::env::var("RUSTIBLE_STRATEGY") {
            self.defaults.strategy = strategy;
        }
    }

    /// Get the effective inventory path
    #[allow(dead_code)]
    pub fn inventory_path(&self) -> Option<&PathBuf> {
        self.defaults.inventory.as_ref()
    }

    /// Get the effective remote user
    #[allow(dead_code)]
    pub fn remote_user(&self) -> Option<&str> {
        self.defaults.remote_user.as_deref()
    }

    /// Check if become is enabled
    #[allow(dead_code)]
    pub fn become_enabled(&self) -> bool {
        self.privilege_escalation.r#become
    }

    /// Get vault password file path
    #[allow(dead_code)]
    pub fn vault_password_file(&self) -> Option<&PathBuf> {
        self.vault.password_file.as_ref()
    }

    /// Load from a specific file (legacy compatibility)
    #[allow(dead_code)]
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        Config::default().merge_from_file(&path_buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.defaults.forks, 5);
        assert_eq!(config.defaults.timeout, 30);
        assert_eq!(config.defaults.transport, "ssh");
        assert!(!config.privilege_escalation.r#become);
    }

    #[test]
    fn test_config_merge() {
        let base = Config::default();
        let other = Config {
            defaults: Defaults {
                forks: 10,
                ..Defaults::default()
            },
            ..Config::default()
        };

        let merged = base.merge(other);
        assert_eq!(merged.defaults.forks, 10);
    }

    #[test]
    fn test_env_override() {
        std::env::set_var("RUSTIBLE_FORKS", "20");
        let mut config = Config::default();
        config.apply_env_overrides();
        assert_eq!(config.defaults.forks, 20);
        std::env::remove_var("RUSTIBLE_FORKS");
    }

    // ========================================================================
    // Merge Macro Tests
    // ========================================================================

    #[test]
    fn test_merge_option_macro() {
        // When other is Some, use other
        let base: Option<String> = Some("base".to_string());
        let other: Option<String> = Some("other".to_string());
        let result = merge_option!(base, other);
        assert_eq!(result, Some("other".to_string()));

        // When other is None, fall back to base
        let base: Option<String> = Some("base".to_string());
        let other: Option<String> = None;
        let result = merge_option!(base, other);
        assert_eq!(result, Some("base".to_string()));

        // When both are None, result is None
        let base: Option<String> = None;
        let other: Option<String> = None;
        let result = merge_option!(base, other);
        assert_eq!(result, None);
    }

    #[test]
    fn test_merge_with_default_macro() {
        // When other differs from default, use other
        let base = 5;
        let other = 10;
        let result = merge_with_default!(base, other, 5);
        assert_eq!(result, 10);

        // When other equals default, use base
        let base = 20;
        let other = 5;
        let result = merge_with_default!(base, other, 5);
        assert_eq!(result, 20);

        // Works with strings too
        let base = "custom".to_string();
        let other = "command".to_string();
        let result = merge_with_default!(base, other, "command");
        assert_eq!(result, "custom");
    }

    #[test]
    fn test_merge_vec_macro() {
        // When other is non-empty, use other
        let base = vec!["a".to_string()];
        let other = vec!["b".to_string(), "c".to_string()];
        let result = merge_vec!(base, other);
        assert_eq!(result, vec!["b".to_string(), "c".to_string()]);

        // When other is empty, use base
        let base = vec!["a".to_string()];
        let other: Vec<String> = vec![];
        let result = merge_vec!(base, other);
        assert_eq!(result, vec!["a".to_string()]);
    }

    #[test]
    fn test_merge_hashmap_macro() {
        // HashMap merge extends base with other
        let mut base = HashMap::new();
        base.insert("key1".to_string(), "value1".to_string());

        let mut other = HashMap::new();
        other.insert("key2".to_string(), "value2".to_string());

        let result = merge_hashmap!(base, other);
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));

        // Other overrides base for same keys
        let mut base = HashMap::new();
        base.insert("key".to_string(), "base".to_string());

        let mut other = HashMap::new();
        other.insert("key".to_string(), "other".to_string());

        let result = merge_hashmap!(base, other);
        assert_eq!(result.get("key"), Some(&"other".to_string()));
    }

    #[test]
    fn test_config_merge_comprehensive() {
        // Test comprehensive merge behavior with the refactored function
        let base = Config {
            defaults: Defaults {
                inventory: Some(PathBuf::from("/base/inventory")),
                remote_user: Some("baseuser".to_string()),
                forks: 10,   // Non-default
                timeout: 60, // Non-default
                roles_path: vec![PathBuf::from("/base/roles")],
                ..Defaults::default()
            },
            module_paths: vec![PathBuf::from("/base/modules")],
            environment: {
                let mut env = HashMap::new();
                env.insert("BASE_VAR".to_string(), "base_value".to_string());
                env
            },
            ..Config::default()
        };

        let other = Config {
            defaults: Defaults {
                inventory: Some(PathBuf::from("/other/inventory")),
                remote_user: None,  // Should fall back to base
                forks: 5,           // Default value, should use base
                timeout: 90,        // Non-default, should use other
                roles_path: vec![], // Empty, should use base
                ..Defaults::default()
            },
            module_paths: vec![PathBuf::from("/other/modules")],
            environment: {
                let mut env = HashMap::new();
                env.insert("OTHER_VAR".to_string(), "other_value".to_string());
                env
            },
            ..Config::default()
        };

        let merged = base.merge(other);

        // Option: other wins when Some
        assert_eq!(
            merged.defaults.inventory,
            Some(PathBuf::from("/other/inventory"))
        );

        // Option: base wins when other is None
        assert_eq!(merged.defaults.remote_user, Some("baseuser".to_string()));

        // Default check: base wins when other is default
        assert_eq!(merged.defaults.forks, 10);

        // Default check: other wins when other is non-default
        assert_eq!(merged.defaults.timeout, 90);

        // Vec: base wins when other is empty
        assert_eq!(
            merged.defaults.roles_path,
            vec![PathBuf::from("/base/roles")]
        );

        // Vec: other wins when other is non-empty
        assert_eq!(merged.module_paths, vec![PathBuf::from("/other/modules")]);

        // HashMap: merged from both
        assert_eq!(
            merged.environment.get("BASE_VAR"),
            Some(&"base_value".to_string())
        );
        assert_eq!(
            merged.environment.get("OTHER_VAR"),
            Some(&"other_value".to_string())
        );
    }
}
