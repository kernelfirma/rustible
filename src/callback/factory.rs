//! Plugin Factory for Rustible Callback System
//!
//! This module provides a factory for creating callback plugins by name string.
//! It supports all built-in plugins, handles configuration options, and provides
//! proper error handling for unknown plugins.
//!
//! # Features
//!
//! - Create plugins by name string (e.g., "minimal", "null", "summary")
//! - Configuration through the existing `CallbackConfig` and `PluginConfig` structures
//! - List all available built-in plugins
//! - Extensible through custom plugin registration
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::factory::{PluginFactory, PluginRegistry};
//! use rustible::callback::config::CallbackConfig;
//!
//! // Create with default configuration
//! let plugin = PluginFactory::create("minimal", &CallbackConfig::default())?;
//!
//! // Create with custom configuration
//! let config = CallbackConfig::for_plugin("null");
//! let null_callback = PluginFactory::create("null", &config)?;
//!
//! // List available plugins
//! for info in PluginFactory::available_plugins() {
//!     println!("{}: {}", info.name, info.description);
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::callback::config::{CallbackConfig, PluginConfig};
use crate::callback::plugins::{
    DiffCallback, DiffConfig, MinimalCallback, NullCallback, ProgressCallback, ProgressConfig,
    SelectiveCallback, SelectiveConfig, SummaryCallback, SummaryConfig,
};
use crate::traits::ExecutionCallback;

// ============================================================================
// Error Types
// ============================================================================

/// Error type for plugin factory operations.
#[derive(Debug, Clone)]
pub struct PluginFactoryError {
    /// The kind of error that occurred.
    pub kind: PluginFactoryErrorKind,
    /// Additional context about the error.
    pub message: String,
}

/// Types of errors that can occur in the plugin factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginFactoryErrorKind {
    /// The requested plugin was not found.
    UnknownPlugin,
    /// Invalid configuration was provided.
    InvalidConfig,
    /// Plugin initialization failed.
    InitializationFailed,
}

impl fmt::Display for PluginFactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            PluginFactoryErrorKind::UnknownPlugin => {
                write!(f, "Unknown plugin: {}", self.message)
            }
            PluginFactoryErrorKind::InvalidConfig => {
                write!(f, "Invalid configuration: {}", self.message)
            }
            PluginFactoryErrorKind::InitializationFailed => {
                write!(f, "Plugin initialization failed: {}", self.message)
            }
        }
    }
}

impl std::error::Error for PluginFactoryError {}

impl PluginFactoryError {
    /// Create an error for an unknown plugin.
    pub fn unknown_plugin(name: &str) -> Self {
        Self {
            kind: PluginFactoryErrorKind::UnknownPlugin,
            message: format!(
                "'{}'. Available plugins: {}",
                name,
                PluginFactory::available_plugin_names().join(", ")
            ),
        }
    }

    /// Create an error for invalid configuration.
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self {
            kind: PluginFactoryErrorKind::InvalidConfig,
            message: message.into(),
        }
    }

    /// Create an error for initialization failure.
    #[allow(dead_code)]
    pub fn init_failed(message: impl Into<String>) -> Self {
        Self {
            kind: PluginFactoryErrorKind::InitializationFailed,
            message: message.into(),
        }
    }
}

/// Result type for plugin factory operations.
pub type PluginResult<T> = Result<T, PluginFactoryError>;

// ============================================================================
// Plugin Information
// ============================================================================

/// Information about an available plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// The plugin's unique name.
    pub name: &'static str,
    /// A brief description of the plugin.
    pub description: &'static str,
    /// The plugin type/category.
    pub plugin_type: PluginType,
    /// Available configuration options.
    pub options: Vec<PluginOptionInfo>,
}

/// Type/category of a callback plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    /// Stdout/display plugins for terminal output.
    Stdout,
    /// Notification plugins (email, slack, etc.).
    Notification,
    /// Logging/output file plugins.
    Logging,
    /// Aggregation/statistics plugins.
    Aggregate,
}

impl fmt::Display for PluginType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginType::Stdout => write!(f, "stdout"),
            PluginType::Notification => write!(f, "notification"),
            PluginType::Logging => write!(f, "logging"),
            PluginType::Aggregate => write!(f, "aggregate"),
        }
    }
}

/// Information about a plugin configuration option.
#[derive(Debug, Clone)]
pub struct PluginOptionInfo {
    /// Option name.
    pub name: &'static str,
    /// Option description.
    pub description: &'static str,
    /// Option type.
    pub option_type: &'static str,
    /// Default value as string.
    pub default: &'static str,
}

// ============================================================================
// Plugin Factory
// ============================================================================

/// Factory type alias for plugin creation functions.
pub type PluginFactoryFn =
    Box<dyn Fn(&CallbackConfig) -> PluginResult<Arc<dyn ExecutionCallback>> + Send + Sync>;

/// Factory for creating callback plugins by name.
///
/// This factory provides a centralized way to instantiate callback plugins
/// using their string name and configuration options.
pub struct PluginFactory;

impl PluginFactory {
    /// Create a callback plugin by name with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to create (case-insensitive).
    /// * `config` - Configuration for the callback system.
    ///
    /// # Returns
    ///
    /// An `Arc<dyn ExecutionCallback>` if successful, or an error if the plugin
    /// is unknown or configuration is invalid.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// # use rustible::callback::config::CallbackConfig;
    /// # use rustible::callback::factory::PluginFactory;
    /// let plugin = PluginFactory::create("minimal", &CallbackConfig::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn create(name: &str, config: &CallbackConfig) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let name_lower = name.to_lowercase();

        // Get plugin-specific config if available
        let plugin_config = config.get_plugin_config(&name_lower);

        match name_lower.as_str() {
            // ================================================================
            // Stdout Plugins
            // ================================================================
            "minimal" => Self::create_minimal(config, plugin_config),
            "null" | "silent" | "quiet" => Self::create_null(config, plugin_config),
            "summary" => Self::create_summary(config, plugin_config),
            "progress" => Self::create_progress(config, plugin_config),
            "selective" => Self::create_selective(config, plugin_config),
            "diff" => Self::create_diff(config, plugin_config),

            // ================================================================
            // Notification Plugins (temporarily disabled - notification.rs needs fixes)
            // ================================================================
            // "notification" | "notify" => Self::create_notification(config, plugin_config),
            _ => Err(PluginFactoryError::unknown_plugin(name)),
        }
    }

    /// Create a callback plugin with default configuration.
    ///
    /// This is a convenience method for creating plugins without custom options.
    pub fn create_default(name: &str) -> PluginResult<Arc<dyn ExecutionCallback>> {
        Self::create(name, &CallbackConfig::default())
    }

    /// Create multiple plugins from a list of names.
    ///
    /// Returns all successfully created plugins. Failed plugins are logged
    /// but don't prevent other plugins from being created.
    pub fn create_many(names: &[&str], config: &CallbackConfig) -> Vec<Arc<dyn ExecutionCallback>> {
        names
            .iter()
            .filter_map(|name| Self::create(name, config).ok())
            .collect()
    }

    /// Create all enabled plugins from a configuration.
    ///
    /// Uses the `enabled_plugins` list from the configuration to create
    /// all specified callback plugins.
    pub fn create_from_config(config: &CallbackConfig) -> Vec<Arc<dyn ExecutionCallback>> {
        config
            .enabled_plugins
            .iter()
            .filter_map(|name| {
                if config.is_plugin_enabled(name) {
                    Self::create(name, config).ok()
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns a list of all available plugin names.
    pub fn available_plugin_names() -> Vec<&'static str> {
        vec![
            "minimal",
            "null",
            "summary",
            "progress",
            "selective",
            "diff",
            // "notification", // temporarily disabled
        ]
    }

    /// Returns detailed information about all available plugins.
    pub fn available_plugins() -> Vec<PluginInfo> {
        vec![
            PluginInfo {
                name: "minimal",
                description: "Minimal output - only failures and final recap (ideal for CI/CD)",
                plugin_type: PluginType::Stdout,
                options: vec![],
            },
            PluginInfo {
                name: "null",
                description: "Silent callback - produces no output (for scripting)",
                plugin_type: PluginType::Stdout,
                options: vec![],
            },
            PluginInfo {
                name: "summary",
                description:
                    "Summary callback with customizable output and unreachable host handling",
                plugin_type: PluginType::Stdout,
                options: vec![
                    PluginOptionInfo {
                        name: "show_per_host",
                        description: "Show per-host statistics breakdown",
                        option_type: "bool",
                        default: "true",
                    },
                    PluginOptionInfo {
                        name: "show_timing",
                        description: "Show task timing information",
                        option_type: "bool",
                        default: "true",
                    },
                    PluginOptionInfo {
                        name: "use_colors",
                        description: "Use ANSI colors in output",
                        option_type: "bool",
                        default: "true",
                    },
                ],
            },
            PluginInfo {
                name: "progress",
                description: "Visual progress bars for playbook execution (requires indicatif)",
                plugin_type: PluginType::Stdout,
                options: vec![
                    PluginOptionInfo {
                        name: "show_host_bars",
                        description: "Show individual progress bars per host",
                        option_type: "bool",
                        default: "true",
                    },
                    PluginOptionInfo {
                        name: "use_colors",
                        description: "Use colored progress bars",
                        option_type: "bool",
                        default: "true",
                    },
                ],
            },
            PluginInfo {
                name: "selective",
                description: "Selective output based on status filters (ok, changed, failed, etc.)",
                plugin_type: PluginType::Stdout,
                options: vec![
                    PluginOptionInfo {
                        name: "show_ok",
                        description: "Show OK tasks",
                        option_type: "bool",
                        default: "true",
                    },
                    PluginOptionInfo {
                        name: "show_changed",
                        description: "Show changed tasks",
                        option_type: "bool",
                        default: "true",
                    },
                    PluginOptionInfo {
                        name: "show_skipped",
                        description: "Show skipped tasks",
                        option_type: "bool",
                        default: "false",
                    },
                    PluginOptionInfo {
                        name: "show_failed",
                        description: "Show failed tasks",
                        option_type: "bool",
                        default: "true",
                    },
                ],
            },
            PluginInfo {
                name: "diff",
                description: "Shows before/after diffs for changed files",
                plugin_type: PluginType::Stdout,
                options: vec![
                    PluginOptionInfo {
                        name: "context_lines",
                        description: "Number of context lines around changes",
                        option_type: "integer",
                        default: "3",
                    },
                    PluginOptionInfo {
                        name: "use_colors",
                        description: "Use ANSI colors for diffs",
                        option_type: "bool",
                        default: "true",
                    },
                ],
            },
            // Notification plugin - temporarily disabled (notification.rs needs fixes)
            // PluginInfo {
            //     name: "notification",
            //     description: "External notifications (Slack, Email, Webhooks)",
            //     plugin_type: PluginType::Notification,
            //     options: vec![...],
            // },
        ]
    }

    /// Check if a plugin with the given name exists.
    pub fn plugin_exists(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        Self::available_plugin_names().iter().any(|&n| {
            n.to_lowercase() == name_lower
                || n.replace('_', "-") == name_lower
                || n.replace('-', "_") == name_lower
        })
    }

    /// Get information about a specific plugin.
    pub fn get_plugin_info(name: &str) -> Option<PluginInfo> {
        let name_lower = name.to_lowercase();
        Self::available_plugins().into_iter().find(|p| {
            p.name.to_lowercase() == name_lower
                || p.name.replace('_', "-") == name_lower
                || p.name.replace('-', "_") == name_lower
        })
    }

    // ========================================================================
    // Private Factory Methods for Each Plugin
    // ========================================================================

    fn create_minimal(
        _config: &CallbackConfig,
        _plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        Ok(Arc::new(MinimalCallback::new()))
    }

    fn create_null(
        _config: &CallbackConfig,
        _plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        Ok(Arc::new(NullCallback::new()))
    }

    fn create_summary(
        config: &CallbackConfig,
        plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let mut summary_config = SummaryConfig {
            use_colors: config.use_colors,
            show_timing: config.show_task_timing,
            ..Default::default()
        };

        // Apply plugin-specific config
        if let Some(pc) = plugin_config {
            if let Some(v) = pc.get_bool("show_host_details") {
                summary_config.show_host_details = v;
            }
            if let Some(v) = pc.get_bool("show_timing") {
                summary_config.show_timing = v;
            }
            if let Some(v) = pc.get_bool("use_colors") {
                summary_config.use_colors = v;
            }
            if let Some(v) = pc.get_bool("compact_mode") {
                summary_config.compact_mode = v;
            }
            if let Some(v) = pc.get_bool("show_exit_code_hint") {
                summary_config.show_exit_code_hint = v;
            }
        }

        Ok(Arc::new(SummaryCallback::with_config(summary_config)))
    }

    fn create_progress(
        config: &CallbackConfig,
        plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let mut progress_config = ProgressConfig {
            use_color: config.use_colors,
            ..Default::default()
        };

        // Apply plugin-specific config
        if let Some(pc) = plugin_config {
            if let Some(v) = pc.get_bool("use_color") {
                progress_config.use_color = v;
            }
            if let Some(v) = pc.get_bool("show_task_spinners") {
                progress_config.show_task_spinners = v;
            }
            if let Some(v) = pc.get_bool("show_elapsed") {
                progress_config.show_elapsed = v;
            }
            if let Some(v) = pc.get_bool("show_eta") {
                progress_config.show_eta = v;
            }
            if let Some(v) = pc.get_i64("spinner_tick_ms") {
                progress_config.spinner_tick_ms = v as u64;
            }
            if let Some(v) = pc.get_i64("max_task_spinners") {
                progress_config.max_task_spinners = v as usize;
            }
        }

        Ok(Arc::new(ProgressCallback::with_config(progress_config)))
    }

    fn create_selective(
        config: &CallbackConfig,
        plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let mut selective_config = SelectiveConfig::default();

        // Apply global config - SelectiveConfig uses StatusFilter and FilterMode
        // The global config options are mapped to the appropriate filter settings
        if !config.show_ok {
            selective_config.status_filter.failures_only = true;
        }
        if !config.show_skipped {
            selective_config.status_filter.skipped_only = false;
        }

        // Apply plugin-specific config
        if let Some(pc) = plugin_config {
            if let Some(v) = pc.get_bool("failures_only") {
                selective_config.status_filter.failures_only = v;
            }
            if let Some(v) = pc.get_bool("changes_only") {
                selective_config.status_filter.changes_only = v;
            }
            if let Some(v) = pc.get_bool("skipped_only") {
                selective_config.status_filter.skipped_only = v;
            }
            if let Some(v) = pc.get_bool("verbose") {
                selective_config.verbose = v;
            }
        }

        Ok(Arc::new(SelectiveCallback::new(selective_config)))
    }

    fn create_diff(
        config: &CallbackConfig,
        plugin_config: Option<&PluginConfig>,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let mut diff_config = DiffConfig {
            use_color: config.use_colors,
            enabled: config.show_diff,
            ..Default::default()
        };

        // Apply plugin-specific config
        if let Some(pc) = plugin_config {
            if let Some(v) = pc.get_i64("context_lines") {
                diff_config.context_lines = v as usize;
            }
            if let Some(v) = pc.get_bool("use_color") {
                diff_config.use_color = v;
            }
            if let Some(v) = pc.get_bool("show_line_numbers") {
                diff_config.show_line_numbers = v;
            }
            if let Some(v) = pc.get_i64("max_lines") {
                diff_config.max_lines = v as usize;
            }
            if let Some(v) = pc.get_bool("enabled") {
                diff_config.enabled = v;
            }
        }

        Ok(Arc::new(DiffCallback::with_config(diff_config)))
    }

    // Notification plugin - temporarily disabled (notification.rs needs fixes)
    // fn create_notification(
    //     _config: &CallbackConfig,
    //     plugin_config: Option<&PluginConfig>,
    // ) -> PluginResult<Arc<dyn ExecutionCallback>> {
    //     let mut notification_config = NotificationConfig::from_env();
    //     if let Some(pc) = plugin_config {
    //         if let Some(v) = pc.get_bool("notify_on_success") {
    //             notification_config.notify_on_success = v;
    //         }
    //         if let Some(v) = pc.get_bool("notify_on_failure") {
    //             notification_config.notify_on_failure = v;
    //         }
    //     }
    //     Ok(Arc::new(NotificationCallback::with_config(notification_config)))
    // }
}

// ============================================================================
// Plugin Registry (for custom/external plugins)
// ============================================================================

/// Registry for custom callback plugins.
///
/// This allows users to register their own callback plugins that can be
/// created by name, just like the built-in plugins.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::config::CallbackConfig;
/// use rustible::callback::factory::PluginRegistry;
/// use std::sync::Arc;
///
/// let mut registry = PluginRegistry::new();
///
/// registry.register("my_plugin", |config| {
///     let _ = config;
///     Ok(Arc::new(MinimalCallback::new()))
/// });
///
/// let plugin = registry.create("my_plugin", &CallbackConfig::default())?;
/// # Ok(())
/// # }
/// ```
pub struct PluginRegistry {
    /// Registered plugin factories
    factories: HashMap<String, PluginFactoryFn>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create a new empty plugin registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Create a registry pre-populated with all built-in plugins.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        for name in PluginFactory::available_plugin_names() {
            let name_owned = name.to_string();
            registry.register(name, move |config| {
                PluginFactory::create(&name_owned, config)
            });
        }

        registry
    }

    /// Register a custom plugin factory.
    ///
    /// The factory function receives the callback configuration and should
    /// return an Arc-wrapped callback plugin or an error.
    pub fn register<F>(&mut self, name: &str, factory: F)
    where
        F: Fn(&CallbackConfig) -> PluginResult<Arc<dyn ExecutionCallback>> + Send + Sync + 'static,
    {
        self.factories
            .insert(name.to_lowercase(), Box::new(factory));
    }

    /// Unregister a plugin factory.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.factories.remove(&name.to_lowercase()).is_some()
    }

    /// Create a plugin by name.
    ///
    /// First checks the registry for custom plugins, then falls back
    /// to built-in plugins.
    pub fn create(
        &self,
        name: &str,
        config: &CallbackConfig,
    ) -> PluginResult<Arc<dyn ExecutionCallback>> {
        let name_lower = name.to_lowercase();

        // Check custom registry first
        if let Some(factory) = self.factories.get(&name_lower) {
            return factory(config);
        }

        // Fall back to built-in factory
        PluginFactory::create(name, config)
    }

    /// Check if a plugin is registered.
    pub fn is_registered(&self, name: &str) -> bool {
        self.factories.contains_key(&name.to_lowercase())
    }

    /// List all registered plugin names.
    pub fn registered_names(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Get combined list of registered and built-in plugin names.
    pub fn all_available_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.factories.keys().cloned().collect();

        for builtin in PluginFactory::available_plugin_names() {
            if !names.iter().any(|n| n == builtin) {
                names.push(builtin.to_string());
            }
        }

        names.sort();
        names
    }
}

impl fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginRegistry")
            .field(
                "registered_plugins",
                &self.factories.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_minimal_plugin() {
        let plugin = PluginFactory::create("minimal", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_null_plugin() {
        let plugin = PluginFactory::create("null", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_summary_plugin() {
        let plugin = PluginFactory::create("summary", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_progress_plugin() {
        let plugin = PluginFactory::create("progress", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_selective_plugin() {
        let plugin = PluginFactory::create("selective", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_diff_plugin() {
        let plugin = PluginFactory::create("diff", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    // Notification test - temporarily disabled
    // #[test]
    // fn test_create_notification_plugin() {
    //     let plugin = PluginFactory::create("notification", &CallbackConfig::default());
    //     assert!(plugin.is_ok());
    // }

    #[test]
    fn test_create_with_config() {
        let mut config = CallbackConfig::default();
        let mut plugin_config = PluginConfig::enabled();
        plugin_config.set_option("show_per_host", true);
        plugin_config.set_option("show_timing", false);
        config.plugins.insert("summary".to_string(), plugin_config);

        let plugin = PluginFactory::create("summary", &config);
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_unknown_plugin() {
        let result = PluginFactory::create("unknown_plugin", &CallbackConfig::default());
        assert!(result.is_err());

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error"),
        };
        assert_eq!(err.kind, PluginFactoryErrorKind::UnknownPlugin);
        assert!(err.message.contains("unknown_plugin"));
    }

    #[test]
    fn test_case_insensitive_names() {
        assert!(PluginFactory::create("MINIMAL", &CallbackConfig::default()).is_ok());
        assert!(PluginFactory::create("Minimal", &CallbackConfig::default()).is_ok());
        assert!(PluginFactory::create("NULL", &CallbackConfig::default()).is_ok());
    }

    #[test]
    fn test_alternative_names() {
        // Test aliases
        assert!(PluginFactory::create("silent", &CallbackConfig::default()).is_ok());
        assert!(PluginFactory::create("quiet", &CallbackConfig::default()).is_ok());
        // "notify" temporarily disabled (notification plugin disabled)
        // assert!(PluginFactory::create("notify", &CallbackConfig::default()).is_ok());
    }

    #[test]
    fn test_available_plugin_names() {
        let names = PluginFactory::available_plugin_names();
        assert!(names.contains(&"minimal"));
        assert!(names.contains(&"null"));
        assert!(names.contains(&"summary"));
        assert!(names.contains(&"progress"));
        assert!(names.contains(&"selective"));
        assert!(names.contains(&"diff"));
        // Notification temporarily disabled
        // assert!(names.contains(&"notification"));
    }

    #[test]
    fn test_available_plugins_info() {
        let plugins = PluginFactory::available_plugins();
        assert!(!plugins.is_empty());

        let minimal = plugins.iter().find(|p| p.name == "minimal");
        assert!(minimal.is_some());
        assert_eq!(minimal.unwrap().plugin_type, PluginType::Stdout);

        let summary = plugins.iter().find(|p| p.name == "summary");
        assert!(summary.is_some());
        assert!(!summary.unwrap().options.is_empty());
    }

    #[test]
    fn test_plugin_exists() {
        assert!(PluginFactory::plugin_exists("minimal"));
        assert!(PluginFactory::plugin_exists("null"));
        assert!(PluginFactory::plugin_exists("summary"));
        assert!(!PluginFactory::plugin_exists("nonexistent"));
    }

    #[test]
    fn test_get_plugin_info() {
        let info = PluginFactory::get_plugin_info("summary");
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.name, "summary");
        assert!(!info.options.is_empty());
    }

    #[test]
    fn test_plugin_factory_error_display() {
        let err = PluginFactoryError::unknown_plugin("test");
        assert!(err.to_string().contains("Unknown plugin"));
        assert!(err.to_string().contains("test"));

        let err = PluginFactoryError::invalid_config("bad config");
        assert!(err.to_string().contains("Invalid configuration"));

        let err = PluginFactoryError::init_failed("init error");
        assert!(err.to_string().contains("initialization failed"));
    }

    #[test]
    fn test_create_default() {
        let plugin = PluginFactory::create_default("minimal");
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_create_many() {
        let plugins = PluginFactory::create_many(
            &["minimal", "null", "nonexistent"],
            &CallbackConfig::default(),
        );
        // Should create 2 plugins (minimal and null), nonexistent is skipped
        assert_eq!(plugins.len(), 2);
    }

    #[test]
    fn test_create_from_config() {
        let mut config = CallbackConfig::default();
        config.enabled_plugins = vec!["minimal".to_string(), "null".to_string()];
        config
            .plugins
            .insert("minimal".to_string(), PluginConfig::enabled());
        config
            .plugins
            .insert("null".to_string(), PluginConfig::enabled());

        let plugins = PluginFactory::create_from_config(&config);
        assert_eq!(plugins.len(), 2);
    }

    // ========================================================================
    // Registry Tests
    // ========================================================================

    #[test]
    fn test_plugin_registry_new() {
        let registry = PluginRegistry::new();
        assert!(registry.registered_names().is_empty());
    }

    #[test]
    fn test_plugin_registry_with_builtins() {
        let registry = PluginRegistry::with_builtins();
        assert!(registry.is_registered("minimal"));
        assert!(registry.is_registered("null"));
    }

    #[test]
    fn test_plugin_registry_register() {
        let mut registry = PluginRegistry::new();

        registry.register("custom", |_config| {
            Ok(Arc::new(MinimalCallback::new()) as Arc<dyn ExecutionCallback>)
        });

        assert!(registry.is_registered("custom"));
        assert!(registry.is_registered("CUSTOM")); // Case-insensitive

        let plugin = registry.create("custom", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_plugin_registry_unregister() {
        let mut registry = PluginRegistry::new();

        registry.register("custom", |_config| {
            Ok(Arc::new(MinimalCallback::new()) as Arc<dyn ExecutionCallback>)
        });

        assert!(registry.unregister("custom"));
        assert!(!registry.is_registered("custom"));
        assert!(!registry.unregister("custom")); // Already removed
    }

    #[test]
    fn test_plugin_registry_fallback_to_builtin() {
        let registry = PluginRegistry::new();

        // Should find built-in even though registry is empty
        let plugin = registry.create("minimal", &CallbackConfig::default());
        assert!(plugin.is_ok());
    }

    #[test]
    fn test_plugin_registry_all_available_names() {
        let mut registry = PluginRegistry::new();
        registry.register("custom", |_| {
            Ok(Arc::new(MinimalCallback::new()) as Arc<dyn ExecutionCallback>)
        });

        let names = registry.all_available_names();
        assert!(names.contains(&"custom".to_string()));
        assert!(names.contains(&"minimal".to_string()));
        assert!(names.contains(&"null".to_string()));
    }

    #[test]
    fn test_plugin_registry_debug() {
        let registry = PluginRegistry::with_builtins();
        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("PluginRegistry"));
    }
}
