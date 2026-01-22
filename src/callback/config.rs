//! Callback Plugin Configuration
//!
//! This module provides a comprehensive configuration system for callback plugins,
//! supporting multiple configuration sources with proper precedence:
//!
//! 1. Default values (lowest priority)
//! 2. Configuration file (TOML, YAML, or JSON)
//! 3. Environment variables
//! 4. CLI arguments (highest priority)
//!
//! # Configuration File Format (TOML)
//!
//! ```toml
//! [callbacks]
//! # Default callback plugin to use
//! default = "default"
//!
//! # Enable multiple plugins simultaneously
//! enabled = ["default", "timer"]
//!
//! # Global verbosity level (0-4)
//! verbosity = 1
//!
//! # Show diff output for changed files
//! show_diff = false
//!
//! # Check mode (dry run)
//! check_mode = false
//!
//! # Output destination: "stdout", "stderr", or file path
//! output = "stdout"
//!
//! # Per-plugin configuration
//! [callbacks.plugins.timer]
//! enabled = true
//! show_per_task = true
//! show_summary = true
//! top_slowest = 10
//! threshold_secs = 0.0
//!
//! [callbacks.plugins.json]
//! enabled = false
//! output = "/var/log/rustible/execution.jsonl"
//! show_full_result = true
//! indent = 0
//!
//! [callbacks.plugins.profile_tasks]
//! enabled = false
//! slow_threshold_secs = 10.0
//! bottleneck_threshold_secs = 30.0
//! top_tasks_count = 20
//! ```
//!
//! # Environment Variables
//!
//! - `RUSTIBLE_CALLBACK` - Default callback plugin
//! - `RUSTIBLE_CALLBACKS_ENABLED` - Comma-separated list of enabled plugins
//! - `RUSTIBLE_CALLBACK_VERBOSITY` - Verbosity level (0-4)
//! - `RUSTIBLE_CALLBACK_SHOW_DIFF` - Enable diff output (true/false)
//! - `RUSTIBLE_CALLBACK_OUTPUT` - Output destination
//! - `RUSTIBLE_CALLBACK_<PLUGIN>_<OPTION>` - Per-plugin options (uppercase)
//!
//! # CLI Arguments
//!
//! - `--callback <name>` or `-c <name>` - Select callback plugin
//! - `--callback-config <path>` - Path to callback configuration file
//! - `--show-diff` / `--no-diff` - Enable/disable diff output
//! - `-v`, `-vv`, `-vvv`, `-vvvv` - Increase verbosity
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::config::{CallbackConfig, CallbackConfigLoader};
//!
//! // Load from all sources with proper precedence
//! let config = CallbackConfigLoader::new()
//!     .with_file("/etc/rustible/callbacks.toml")
//!     .with_env_prefix("RUSTIBLE_CALLBACK")
//!     .with_plugin("timer")
//!     .with_verbosity(2)
//!     .with_show_diff(true)
//!     .load()?;
//!
//! // Access configuration
//! println!("Default plugin: {}", config.default_plugin);
//! println!("Verbosity: {}", config.verbosity);
//!
//! // Get per-plugin config
//! if let Some(timer_config) = config.get_plugin_config("timer") {
//!     println!("Timer enabled: {}", timer_config.enabled);
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tracing::{debug, warn};

// ============================================================================
// Core Configuration Types
// ============================================================================

/// Main callback configuration structure.
///
/// This contains all settings that control callback plugin behavior,
/// including global settings and per-plugin configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CallbackConfig {
    /// Name of the callback plugin (e.g., "default", "json", "minimal")
    pub plugin: String,

    /// Default callback plugin to use when none is specified
    pub default_plugin: String,

    /// List of enabled callback plugins (can run multiple simultaneously)
    pub enabled_plugins: Vec<String>,

    /// Verbosity level (0 = quiet, 1 = normal, 2 = verbose, 3 = debug, 4 = trace)
    pub verbosity: u8,

    /// Whether to show diff output for changed files
    pub show_diff: bool,

    /// Whether we're in check mode (dry run)
    pub check_mode: bool,

    /// Output destination: "stdout", "stderr", or a file path
    pub output: String,

    /// Whether to use colored output
    pub use_colors: bool,

    /// Whether to display task timing information
    pub show_task_timing: bool,

    /// Whether to show skipped tasks
    pub show_skipped: bool,

    /// Whether to show ok tasks (not changed)
    pub show_ok: bool,

    /// Whether to display play recap at the end
    pub show_recap: bool,

    /// Per-plugin configuration options
    #[serde(default)]
    pub plugins: HashMap<String, PluginConfig>,

    /// Additional arbitrary options (for custom plugins)
    #[serde(default)]
    pub options: HashMap<String, JsonValue>,
}

impl Default for CallbackConfig {
    fn default() -> Self {
        Self {
            plugin: "default".to_string(),
            default_plugin: "default".to_string(),
            enabled_plugins: vec!["default".to_string()],
            verbosity: 1,
            show_diff: false,
            check_mode: false,
            output: "stdout".to_string(),
            use_colors: true,
            show_task_timing: false,
            show_skipped: true,
            show_ok: true,
            show_recap: true,
            plugins: HashMap::new(),
            options: HashMap::new(),
        }
    }
}

impl CallbackConfig {
    /// Create a new callback configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration for a specific plugin.
    pub fn for_plugin(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            default_plugin: plugin.to_string(),
            enabled_plugins: vec![plugin.to_string()],
            ..Default::default()
        }
    }

    /// Get plugin-specific configuration.
    pub fn get_plugin_config(&self, plugin_name: &str) -> Option<&PluginConfig> {
        self.plugins.get(plugin_name)
    }

    /// Get mutable plugin-specific configuration, creating if necessary.
    pub fn get_or_create_plugin_config(&mut self, plugin_name: &str) -> &mut PluginConfig {
        self.plugins.entry(plugin_name.to_string()).or_default()
    }

    /// Check if a specific plugin is enabled.
    pub fn is_plugin_enabled(&self, plugin_name: &str) -> bool {
        // Check enabled_plugins list
        if self.enabled_plugins.contains(&plugin_name.to_string()) {
            // Check per-plugin enabled flag
            self.plugins
                .get(plugin_name)
                .map(|p| p.enabled)
                .unwrap_or(true)
        } else {
            false
        }
    }

    /// Set verbosity level with bounds checking.
    pub fn set_verbosity(&mut self, level: u8) {
        self.verbosity = level.min(4);
    }

    /// Enable a plugin.
    pub fn enable_plugin(&mut self, plugin_name: &str) {
        if !self.enabled_plugins.contains(&plugin_name.to_string()) {
            self.enabled_plugins.push(plugin_name.to_string());
        }
        self.get_or_create_plugin_config(plugin_name).enabled = true;
    }

    /// Disable a plugin.
    pub fn disable_plugin(&mut self, plugin_name: &str) {
        self.enabled_plugins.retain(|p| p != plugin_name);
        if let Some(config) = self.plugins.get_mut(plugin_name) {
            config.enabled = false;
        }
    }

    /// Merge another configuration into this one (other takes precedence).
    pub fn merge(&mut self, other: CallbackConfig) {
        // Override scalar values if they differ from defaults
        if other.plugin != "default" {
            self.plugin = other.plugin;
        }
        if other.default_plugin != "default" {
            self.default_plugin = other.default_plugin;
        }
        if !other.enabled_plugins.is_empty() && other.enabled_plugins != vec!["default".to_string()]
        {
            self.enabled_plugins = other.enabled_plugins;
        }
        if other.verbosity != 1 {
            self.verbosity = other.verbosity;
        }
        if other.show_diff {
            self.show_diff = true;
        }
        if other.check_mode {
            self.check_mode = true;
        }
        if other.output != "stdout" {
            self.output = other.output;
        }
        if !other.use_colors {
            self.use_colors = false;
        }
        if other.show_task_timing {
            self.show_task_timing = true;
        }
        if !other.show_skipped {
            self.show_skipped = false;
        }
        if !other.show_ok {
            self.show_ok = false;
        }
        if !other.show_recap {
            self.show_recap = false;
        }

        // Merge per-plugin configurations
        for (name, config) in other.plugins {
            self.plugins
                .entry(name)
                .and_modify(|existing| existing.merge(&config))
                .or_insert(config);
        }

        // Merge additional options
        self.options.extend(other.options);
    }
}

// ============================================================================
// Per-Plugin Configuration
// ============================================================================

/// Configuration for a specific callback plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginConfig {
    /// Whether this plugin is enabled
    pub enabled: bool,

    /// Plugin priority (lower values run first)
    pub priority: Option<i32>,

    /// Output destination override for this plugin
    pub output: Option<String>,

    /// Plugin-specific options
    #[serde(flatten)]
    pub options: HashMap<String, JsonValue>,
}

impl PluginConfig {
    /// Create a new enabled plugin configuration.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Create a new disabled plugin configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Get an option value as a specific type.
    pub fn get_option<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.options
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Get an option value as a string.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.options
            .get(key)
            .and_then(|v| v.as_str().map(String::from))
    }

    /// Get an option value as a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(|v| v.as_bool())
    }

    /// Get an option value as an integer.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.options.get(key).and_then(|v| v.as_i64())
    }

    /// Get an option value as a float.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.options.get(key).and_then(|v| v.as_f64())
    }

    /// Set an option value.
    pub fn set_option(&mut self, key: impl Into<String>, value: impl Into<JsonValue>) {
        self.options.insert(key.into(), value.into());
    }

    /// Merge another plugin config into this one.
    pub fn merge(&mut self, other: &PluginConfig) {
        if other.enabled {
            self.enabled = true;
        }
        if other.priority.is_some() {
            self.priority = other.priority;
        }
        if other.output.is_some() {
            self.output = other.output.clone();
        }
        self.options.extend(other.options.clone());
    }
}

// ============================================================================
// Configuration Loader
// ============================================================================

/// Builder for loading callback configuration from multiple sources.
///
/// Sources are applied in order, with later sources overriding earlier ones:
/// 1. Default values
/// 2. Configuration files
/// 3. Environment variables
/// 4. CLI arguments
#[derive(Debug, Default)]
pub struct CallbackConfigLoader {
    /// Configuration files to load (in order)
    config_files: Vec<PathBuf>,
    /// Environment variable prefix
    env_prefix: Option<String>,
    /// CLI overrides
    cli_overrides: CallbackConfig,
    /// Whether to load from standard locations
    load_standard_locations: bool,
}

impl CallbackConfigLoader {
    /// Create a new configuration loader.
    pub fn new() -> Self {
        Self {
            config_files: Vec::new(),
            env_prefix: Some("RUSTIBLE_CALLBACK".to_string()),
            cli_overrides: CallbackConfig::default(),
            load_standard_locations: true,
        }
    }

    /// Add a configuration file to load.
    pub fn with_file(mut self, path: impl AsRef<Path>) -> Self {
        self.config_files.push(path.as_ref().to_path_buf());
        self
    }

    /// Set the environment variable prefix.
    pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = Some(prefix.into());
        self
    }

    /// Disable environment variable loading.
    pub fn without_env(mut self) -> Self {
        self.env_prefix = None;
        self
    }

    /// Disable loading from standard configuration locations.
    pub fn without_standard_locations(mut self) -> Self {
        self.load_standard_locations = false;
        self
    }

    /// Set CLI override for the default plugin.
    pub fn with_plugin(mut self, plugin: impl Into<String>) -> Self {
        self.cli_overrides.plugin = plugin.into();
        self
    }

    /// Set CLI override for verbosity.
    pub fn with_verbosity(mut self, level: u8) -> Self {
        self.cli_overrides.verbosity = level;
        self
    }

    /// Set CLI override for diff display.
    pub fn with_show_diff(mut self, show: bool) -> Self {
        self.cli_overrides.show_diff = show;
        self
    }

    /// Set CLI override for check mode.
    pub fn with_check_mode(mut self, check: bool) -> Self {
        self.cli_overrides.check_mode = check;
        self
    }

    /// Set CLI override for colors.
    pub fn with_colors(mut self, use_colors: bool) -> Self {
        self.cli_overrides.use_colors = use_colors;
        self
    }

    /// Load configuration from all sources.
    pub fn load(self) -> Result<CallbackConfig> {
        let mut config = CallbackConfig::default();

        // Load from standard locations if enabled
        if self.load_standard_locations {
            for path in Self::standard_config_paths() {
                if path.exists() {
                    debug!("Loading callback config from: {}", path.display());
                    if let Ok(file_config) = Self::load_file(&path) {
                        config.merge(file_config);
                    }
                }
            }
        }

        // Load from explicitly specified files
        for path in &self.config_files {
            if path.exists() {
                debug!("Loading callback config from: {}", path.display());
                let file_config = Self::load_file(path)
                    .with_context(|| format!("Failed to load config from: {}", path.display()))?;
                config.merge(file_config);
            } else {
                warn!("Callback config file not found: {}", path.display());
            }
        }

        // Apply environment variables
        if let Some(prefix) = &self.env_prefix {
            let env_config = Self::load_from_env(prefix);
            config.merge(env_config);
        }

        // Apply CLI overrides (highest priority)
        config.merge(self.cli_overrides);

        Ok(config)
    }

    /// Get standard configuration file locations.
    fn standard_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // System-wide config
        paths.push(PathBuf::from("/etc/rustible/callbacks.toml"));
        paths.push(PathBuf::from("/etc/rustible/callbacks.yml"));

        // User config
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".rustible/callbacks.toml"));
            paths.push(home.join(".rustible/callbacks.yml"));
            paths.push(home.join(".config/rustible/callbacks.toml"));
        }

        // XDG config
        if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
            paths.push(PathBuf::from(xdg_config).join("rustible/callbacks.toml"));
        }

        // Project-local config
        paths.push(PathBuf::from("rustible-callbacks.toml"));
        paths.push(PathBuf::from(".rustible/callbacks.toml"));

        paths
    }

    /// Load configuration from a file.
    fn load_file(path: &Path) -> Result<CallbackConfig> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Parse based on file extension
        let wrapper: CallbacksWrapper = match extension {
            "toml" => toml::from_str(&content)?,
            "yml" | "yaml" => serde_yaml::from_str(&content)?,
            "json" => serde_json::from_str(&content)?,
            _ => {
                // Try TOML first, then YAML
                toml::from_str(&content)
                    .or_else(|_| serde_yaml::from_str(&content))
                    .with_context(|| format!("Failed to parse config file: {}", path.display()))?
            }
        };

        Ok(wrapper.callbacks)
    }

    /// Load configuration from environment variables.
    fn load_from_env(prefix: &str) -> CallbackConfig {
        let mut config = CallbackConfig::default();

        // Main callback plugin
        if let Ok(val) = env::var(prefix) {
            config.plugin = val.clone();
            config.default_plugin = val;
        }

        // Enabled plugins (comma-separated)
        if let Ok(val) = env::var(format!("{}_ENABLED", prefix)) {
            config.enabled_plugins = val.split(',').map(|s| s.trim().to_string()).collect();
        }

        // Verbosity
        if let Ok(val) = env::var(format!("{}_VERBOSITY", prefix)) {
            if let Ok(level) = val.parse::<u8>() {
                config.verbosity = level.min(4);
            }
        }

        // Show diff
        if let Ok(val) = env::var(format!("{}_SHOW_DIFF", prefix)) {
            config.show_diff = val.to_lowercase() == "true" || val == "1";
        }

        // Check mode
        if let Ok(val) = env::var(format!("{}_CHECK_MODE", prefix)) {
            config.check_mode = val.to_lowercase() == "true" || val == "1";
        }

        // Output
        if let Ok(val) = env::var(format!("{}_OUTPUT", prefix)) {
            config.output = val;
        }

        // Colors
        if let Ok(val) = env::var(format!("{}_NO_COLOR", prefix)) {
            if val.to_lowercase() == "true" || val == "1" {
                config.use_colors = false;
            }
        }
        // Also check standard NO_COLOR
        if env::var("NO_COLOR").is_ok() {
            config.use_colors = false;
        }

        // Show task timing
        if let Ok(val) = env::var(format!("{}_SHOW_TIMING", prefix)) {
            config.show_task_timing = val.to_lowercase() == "true" || val == "1";
        }

        // Load per-plugin options from environment
        config.plugins.extend(Self::load_plugin_env_vars(prefix));

        config
    }

    /// Load plugin-specific configuration from environment variables.
    fn load_plugin_env_vars(prefix: &str) -> HashMap<String, PluginConfig> {
        let mut plugins = HashMap::new();

        // Scan all environment variables for plugin-specific settings
        for (key, value) in env::vars() {
            // Pattern: RUSTIBLE_CALLBACK_<PLUGIN>_<OPTION>
            if let Some(suffix) = key.strip_prefix(&format!("{}_", prefix)) {
                // Skip non-plugin vars
                if [
                    "VERBOSITY",
                    "SHOW_DIFF",
                    "CHECK_MODE",
                    "OUTPUT",
                    "NO_COLOR",
                    "ENABLED",
                    "SHOW_TIMING",
                ]
                .contains(&suffix)
                {
                    continue;
                }

                // Split into plugin name and option
                let parts: Vec<&str> = suffix.splitn(2, '_').collect();
                if parts.len() == 2 {
                    let plugin_name = parts[0].to_lowercase();
                    let option_name = parts[1].to_lowercase();

                    let plugin_config = plugins
                        .entry(plugin_name)
                        .or_insert_with(PluginConfig::default);

                    // Try to parse as JSON value, fall back to string
                    let json_value: JsonValue = if let Ok(v) = serde_json::from_str(&value) {
                        v
                    } else if let Ok(b) = value.parse::<bool>() {
                        JsonValue::Bool(b)
                    } else if let Ok(n) = value.parse::<i64>() {
                        JsonValue::Number(n.into())
                    } else if let Ok(n) = value.parse::<f64>() {
                        serde_json::Number::from_f64(n)
                            .map(JsonValue::Number)
                            .unwrap_or(JsonValue::String(value.clone()))
                    } else {
                        JsonValue::String(value)
                    };

                    // Handle special options
                    match option_name.as_str() {
                        "enabled" => {
                            plugin_config.enabled = json_value
                                .as_bool()
                                .unwrap_or(json_value.as_str() == Some("true"));
                        }
                        "priority" => {
                            plugin_config.priority = json_value.as_i64().map(|n| n as i32);
                        }
                        "output" => {
                            plugin_config.output = json_value.as_str().map(String::from);
                        }
                        _ => {
                            plugin_config.options.insert(option_name, json_value);
                        }
                    }
                }
            }
        }

        plugins
    }
}

/// Wrapper for config file format that nests under [callbacks].
#[derive(Debug, Deserialize)]
struct CallbacksWrapper {
    #[serde(default)]
    callbacks: CallbackConfig,
}

// ============================================================================
// Plugin-Specific Configuration Helpers
// ============================================================================

/// Configuration builder for the Timer callback plugin.
#[derive(Debug, Clone, Default)]
pub struct TimerPluginConfig {
    inner: PluginConfig,
}

impl TimerPluginConfig {
    /// Create a new timer plugin configuration.
    pub fn new() -> Self {
        let mut config = Self::default();
        config.inner.enabled = true;
        config
    }

    /// Set whether to show timing after each task.
    pub fn show_per_task(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_per_task", enabled);
        self
    }

    /// Set whether to show timing summary at end.
    pub fn show_summary(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_summary", enabled);
        self
    }

    /// Set number of slowest tasks to show.
    pub fn top_slowest(mut self, count: usize) -> Self {
        self.inner.set_option("top_slowest", count as i64);
        self
    }

    /// Set minimum threshold for showing task timing (seconds).
    pub fn threshold_secs(mut self, seconds: f64) -> Self {
        self.inner.set_option("threshold_secs", seconds);
        self
    }

    /// Set whether to show play-level timing.
    pub fn show_play_timing(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_play_timing", enabled);
        self
    }

    /// Build the plugin configuration.
    pub fn build(self) -> PluginConfig {
        self.inner
    }
}

/// Configuration builder for the JSON callback plugin.
#[derive(Debug, Clone, Default)]
pub struct JsonPluginConfig {
    inner: PluginConfig,
}

impl JsonPluginConfig {
    /// Create a new JSON plugin configuration.
    pub fn new() -> Self {
        let mut config = Self::default();
        config.inner.enabled = true;
        config
    }

    /// Set output destination.
    pub fn output(mut self, path: impl Into<String>) -> Self {
        self.inner.output = Some(path.into());
        self
    }

    /// Set whether to show full result data.
    pub fn show_full_result(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_full_result", enabled);
        self
    }

    /// Set whether to show task arguments.
    pub fn show_task_args(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_task_args", enabled);
        self
    }

    /// Set indentation level (0 for compact).
    pub fn indent(mut self, spaces: usize) -> Self {
        self.inner.set_option("indent", spaces as i64);
        self
    }

    /// Build the plugin configuration.
    pub fn build(self) -> PluginConfig {
        self.inner
    }
}

/// Configuration builder for the ProfileTasks callback plugin.
#[derive(Debug, Clone, Default)]
pub struct ProfileTasksPluginConfig {
    inner: PluginConfig,
}

impl ProfileTasksPluginConfig {
    /// Create a new profile tasks plugin configuration.
    pub fn new() -> Self {
        let mut config = Self::default();
        config.inner.enabled = true;
        config
    }

    /// Set threshold for marking tasks as slow (seconds).
    pub fn slow_threshold_secs(mut self, seconds: f64) -> Self {
        self.inner.set_option("slow_threshold_secs", seconds);
        self
    }

    /// Set threshold for marking tasks as bottlenecks (seconds).
    pub fn bottleneck_threshold_secs(mut self, seconds: f64) -> Self {
        self.inner.set_option("bottleneck_threshold_secs", seconds);
        self
    }

    /// Set number of tasks to show in summary.
    pub fn top_tasks_count(mut self, count: usize) -> Self {
        self.inner.set_option("top_tasks_count", count as i64);
        self
    }

    /// Set whether to show per-host breakdown.
    pub fn show_per_host(mut self, enabled: bool) -> Self {
        self.inner.set_option("show_per_host", enabled);
        self
    }

    /// Set whether to include skipped tasks.
    pub fn include_skipped(mut self, enabled: bool) -> Self {
        self.inner.set_option("include_skipped", enabled);
        self
    }

    /// Build the plugin configuration.
    pub fn build(self) -> PluginConfig {
        self.inner
    }
}

/// Configuration builder for the Minimal callback plugin.
#[derive(Debug, Clone, Default)]
pub struct MinimalPluginConfig {
    inner: PluginConfig,
}

impl MinimalPluginConfig {
    /// Create a new minimal plugin configuration.
    pub fn new() -> Self {
        let mut config = Self::default();
        config.inner.enabled = true;
        config
    }

    /// Build the plugin configuration.
    pub fn build(self) -> PluginConfig {
        self.inner
    }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Load callback configuration from the default locations.
pub fn load_callback_config() -> Result<CallbackConfig> {
    CallbackConfigLoader::new().load()
}

/// Load callback configuration from a specific file.
pub fn load_callback_config_from_file(path: impl AsRef<Path>) -> Result<CallbackConfig> {
    CallbackConfigLoader::new()
        .without_standard_locations()
        .with_file(path)
        .load()
}

/// Create a callback configuration from CLI arguments.
///
/// This is a convenience function for integrating with clap CLI parsing.
pub fn config_from_cli(
    callback: Option<&str>,
    verbosity: u8,
    show_diff: bool,
    check_mode: bool,
    no_color: bool,
) -> CallbackConfig {
    let mut config = CallbackConfig::default();

    if let Some(cb) = callback {
        config.plugin = cb.to_string();
        config.default_plugin = cb.to_string();
        config.enabled_plugins = vec![cb.to_string()];
    }

    config.verbosity = verbosity;
    config.show_diff = show_diff;
    config.check_mode = check_mode;
    config.use_colors = !no_color;

    config
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = CallbackConfig::default();
        assert_eq!(config.plugin, "default");
        assert_eq!(config.verbosity, 1);
        assert!(!config.show_diff);
        assert!(config.use_colors);
        assert!(config.show_recap);
    }

    #[test]
    fn test_config_for_plugin() {
        let config = CallbackConfig::for_plugin("json");
        assert_eq!(config.plugin, "json");
        assert_eq!(config.default_plugin, "json");
        assert_eq!(config.enabled_plugins, vec!["json"]);
    }

    #[test]
    fn test_config_merge() {
        let mut config = CallbackConfig::default();
        let other = CallbackConfig {
            plugin: "json".to_string(),
            verbosity: 3,
            show_diff: true,
            ..Default::default()
        };

        config.merge(other);

        assert_eq!(config.plugin, "json");
        assert_eq!(config.verbosity, 3);
        assert!(config.show_diff);
    }

    #[test]
    fn test_plugin_config() {
        let mut config = PluginConfig::enabled();
        config.set_option("threshold", 10.5);
        config.set_option("enabled_feature", true);
        config.set_option("name", "test");

        assert!(config.enabled);
        assert_eq!(config.get_f64("threshold"), Some(10.5));
        assert_eq!(config.get_bool("enabled_feature"), Some(true));
        assert_eq!(config.get_string("name"), Some("test".to_string()));
    }

    #[test]
    fn test_plugin_enable_disable() {
        let mut config = CallbackConfig::default();

        config.enable_plugin("timer");
        assert!(config.is_plugin_enabled("timer"));
        assert!(config.enabled_plugins.contains(&"timer".to_string()));

        config.disable_plugin("timer");
        assert!(!config.is_plugin_enabled("timer"));
        assert!(!config.enabled_plugins.contains(&"timer".to_string()));
    }

    #[test]
    fn test_timer_plugin_config_builder() {
        let config = TimerPluginConfig::new()
            .show_per_task(true)
            .show_summary(true)
            .top_slowest(20)
            .threshold_secs(1.5)
            .build();

        assert!(config.enabled);
        assert_eq!(config.get_bool("show_per_task"), Some(true));
        assert_eq!(config.get_bool("show_summary"), Some(true));
        assert_eq!(config.get_i64("top_slowest"), Some(20));
        assert_eq!(config.get_f64("threshold_secs"), Some(1.5));
    }

    #[test]
    fn test_json_plugin_config_builder() {
        let config = JsonPluginConfig::new()
            .output("/var/log/test.jsonl")
            .show_full_result(true)
            .indent(2)
            .build();

        assert!(config.enabled);
        assert_eq!(config.output, Some("/var/log/test.jsonl".to_string()));
        assert_eq!(config.get_bool("show_full_result"), Some(true));
        assert_eq!(config.get_i64("indent"), Some(2));
    }

    #[test]
    fn test_profile_tasks_plugin_config_builder() {
        let config = ProfileTasksPluginConfig::new()
            .slow_threshold_secs(5.0)
            .bottleneck_threshold_secs(15.0)
            .top_tasks_count(25)
            .show_per_host(true)
            .build();

        assert!(config.enabled);
        assert_eq!(config.get_f64("slow_threshold_secs"), Some(5.0));
        assert_eq!(config.get_f64("bottleneck_threshold_secs"), Some(15.0));
        assert_eq!(config.get_i64("top_tasks_count"), Some(25));
        assert_eq!(config.get_bool("show_per_host"), Some(true));
    }

    #[test]
    fn test_load_toml_config() {
        let toml_content = r#"
[callbacks]
plugin = "timer"
verbosity = 2
show_diff = true

[callbacks.plugins.timer]
enabled = true
show_per_task = true
threshold_secs = 1.0

[callbacks.plugins.json]
enabled = false
output = "/tmp/output.jsonl"
"#;

        let mut file = NamedTempFile::with_suffix(".toml").unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        let config = CallbackConfigLoader::new()
            .without_standard_locations()
            .without_env()
            .with_file(file.path())
            .load()
            .unwrap();

        assert_eq!(config.plugin, "timer");
        assert_eq!(config.verbosity, 2);
        assert!(config.show_diff);

        let timer_config = config.get_plugin_config("timer").unwrap();
        assert!(timer_config.enabled);
        assert_eq!(timer_config.get_bool("show_per_task"), Some(true));
        assert_eq!(timer_config.get_f64("threshold_secs"), Some(1.0));

        let json_config = config.get_plugin_config("json").unwrap();
        assert!(!json_config.enabled);
        assert_eq!(json_config.output, Some("/tmp/output.jsonl".to_string()));
    }

    #[test]
    fn test_load_yaml_config() {
        let yaml_content = r#"
callbacks:
  plugin: minimal
  verbosity: 3
  show_diff: true
  plugins:
    minimal:
      enabled: true
    profile_tasks:
      enabled: true
      slow_threshold_secs: 5.0
"#;

        let mut file = NamedTempFile::with_suffix(".yml").unwrap();
        file.write_all(yaml_content.as_bytes()).unwrap();

        let config = CallbackConfigLoader::new()
            .without_standard_locations()
            .without_env()
            .with_file(file.path())
            .load()
            .unwrap();

        assert_eq!(config.plugin, "minimal");
        assert_eq!(config.verbosity, 3);

        let profile_config = config.get_plugin_config("profile_tasks").unwrap();
        assert!(profile_config.enabled);
        assert_eq!(profile_config.get_f64("slow_threshold_secs"), Some(5.0));
    }

    #[test]
    fn test_config_from_cli() {
        let config = config_from_cli(Some("json"), 2, true, true, false);

        assert_eq!(config.plugin, "json");
        assert_eq!(config.verbosity, 2);
        assert!(config.show_diff);
        assert!(config.check_mode);
        assert!(config.use_colors);
    }

    #[test]
    fn test_config_from_cli_no_color() {
        let config = config_from_cli(None, 1, false, false, true);

        assert_eq!(config.plugin, "default");
        assert!(!config.use_colors);
    }

    #[test]
    fn test_env_loading() {
        // Set test environment variables
        env::set_var("TEST_CALLBACK", "json");
        env::set_var("TEST_CALLBACK_VERBOSITY", "3");
        env::set_var("TEST_CALLBACK_SHOW_DIFF", "true");
        env::set_var("TEST_CALLBACK_TIMER_ENABLED", "true");
        env::set_var("TEST_CALLBACK_TIMER_THRESHOLD_SECS", "2.5");

        let config = CallbackConfigLoader::load_from_env("TEST_CALLBACK");

        assert_eq!(config.plugin, "json");
        assert_eq!(config.verbosity, 3);
        assert!(config.show_diff);

        let timer_config = config.plugins.get("timer").unwrap();
        assert!(timer_config.enabled);
        assert_eq!(timer_config.get_f64("threshold_secs"), Some(2.5));

        // Clean up
        env::remove_var("TEST_CALLBACK");
        env::remove_var("TEST_CALLBACK_VERBOSITY");
        env::remove_var("TEST_CALLBACK_SHOW_DIFF");
        env::remove_var("TEST_CALLBACK_TIMER_ENABLED");
        env::remove_var("TEST_CALLBACK_TIMER_THRESHOLD_SECS");
    }

    #[test]
    fn test_get_or_create_plugin_config() {
        let mut config = CallbackConfig::default();

        // Plugin doesn't exist yet
        assert!(config.get_plugin_config("new_plugin").is_none());

        // Create it
        let plugin_config = config.get_or_create_plugin_config("new_plugin");
        plugin_config.enabled = true;
        plugin_config.set_option("test_option", 42);

        // Now it exists
        let retrieved = config.get_plugin_config("new_plugin").unwrap();
        assert!(retrieved.enabled);
        assert_eq!(retrieved.get_i64("test_option"), Some(42));
    }

    #[test]
    fn test_set_verbosity_bounds() {
        let mut config = CallbackConfig::default();

        config.set_verbosity(10); // Above max
        assert_eq!(config.verbosity, 4);

        config.set_verbosity(2);
        assert_eq!(config.verbosity, 2);

        config.set_verbosity(0);
        assert_eq!(config.verbosity, 0);
    }
}
