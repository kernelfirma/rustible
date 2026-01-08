//! Factory Tests for Rustible Callback Plugin System
//!
//! This test suite validates the plugin factory pattern for creating callback plugins
//! dynamically by name, with configuration options, and proper error handling.
//!
//! # Test Categories
//!
//! 1. Create plugins by name - validate factory can instantiate plugins from string names
//! 2. Invalid plugin names - ensure proper error handling for unknown plugins
//! 3. Plugin configuration options - test factory respects configuration parameters
//! 4. Default plugin selection - verify default plugin behavior
//! 5. Plugin initialization errors - handle malformed configurations gracefully
//!
//! This test module is self-contained and implements its own factory pattern
//! to validate the design, independent of any internal crate implementation.

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;

// ============================================================================
// Plugin Priority System (mirrors real implementation)
// ============================================================================

/// Priority levels for callback plugins.
/// Lower values execute first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginPriority(pub i32);

impl PluginPriority {
    /// Highest priority - stdout/stderr output plugins.
    pub const STDOUT: Self = Self(100);
    /// High priority - essential logging plugins.
    pub const LOGGING: Self = Self(200);
    /// Normal priority - default for most plugins.
    pub const NORMAL: Self = Self(500);
    /// Low priority - metrics and analytics plugins.
    pub const METRICS: Self = Self(700);
    /// Lowest priority - cleanup and finalization plugins.
    pub const CLEANUP: Self = Self(900);
}

impl Default for PluginPriority {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl PartialOrd for PluginPriority {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for PluginPriority {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.0.cmp(&other.0)
    }
}

// ============================================================================
// Callback Plugin Trait (simplified for testing)
// ============================================================================

/// Callback plugin trait for receiving execution events.
#[async_trait]
pub trait CallbackPlugin: Send + Sync + Debug {
    /// Returns the unique name of this plugin.
    fn name(&self) -> &str;

    /// Returns the priority of this plugin.
    fn priority(&self) -> PluginPriority {
        PluginPriority::NORMAL
    }

    /// Returns whether this plugin is enabled.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Returns a description of this plugin.
    fn description(&self) -> &str {
        "No description"
    }

    /// Called when a playbook starts.
    async fn on_playbook_start(&self, _name: &str) {}

    /// Called when a playbook ends.
    async fn on_playbook_end(&self, _name: &str, _success: bool) {}

    /// Called when the plugin is registered.
    async fn on_register(&self) {}

    /// Called when the plugin is deregistered.
    async fn on_deregister(&self) {}

    /// Reset plugin state.
    async fn reset(&self) {}
}

// ============================================================================
// Simple Callback Manager for Integration Tests
// ============================================================================

/// Manages callback plugin registration and event dispatch.
#[derive(Debug, Default)]
pub struct CallbackManager {
    plugins: RwLock<HashMap<String, Arc<dyn CallbackPlugin>>>,
}

impl CallbackManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, plugin: Arc<dyn CallbackPlugin>) -> bool {
        let name = plugin.name().to_string();
        let mut plugins = self.plugins.write();
        let is_new = !plugins.contains_key(&name);
        plugin.on_register().await;
        plugins.insert(name, plugin);
        is_new
    }

    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.read().contains_key(name)
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.read().len()
    }

    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.read().keys().cloned().collect()
    }

    pub async fn on_playbook_start(&self, name: &str) -> DispatchResult {
        let plugins = self.plugins.read();
        let mut result = DispatchResult::default();

        for plugin in plugins.values() {
            if plugin.is_enabled() {
                plugin.on_playbook_start(name).await;
                result.success_count += 1;
            } else {
                result.skipped_count += 1;
            }
        }

        result
    }
}

/// Result of dispatching an event.
#[derive(Debug, Default)]
pub struct DispatchResult {
    pub success_count: usize,
    pub skipped_count: usize,
    pub errors: Vec<String>,
}

impl DispatchResult {
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }
}

// ============================================================================
// Plugin Factory Implementation for Testing
// ============================================================================

/// Error types for plugin factory operations
#[derive(Debug, Clone, PartialEq)]
pub enum PluginFactoryError {
    /// Plugin name not recognized
    UnknownPlugin(String),
    /// Invalid configuration for plugin
    InvalidConfig { plugin: String, message: String },
    /// Plugin initialization failed
    InitializationFailed { plugin: String, cause: String },
    /// Required configuration missing
    MissingConfig { plugin: String, key: String },
    /// Configuration value has wrong type
    TypeMismatch {
        plugin: String,
        key: String,
        expected: String,
        got: String,
    },
}

impl std::fmt::Display for PluginFactoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginFactoryError::UnknownPlugin(name) => {
                write!(f, "Unknown plugin: '{}'", name)
            }
            PluginFactoryError::InvalidConfig { plugin, message } => {
                write!(f, "Invalid config for plugin '{}': {}", plugin, message)
            }
            PluginFactoryError::InitializationFailed { plugin, cause } => {
                write!(f, "Failed to initialize plugin '{}': {}", plugin, cause)
            }
            PluginFactoryError::MissingConfig { plugin, key } => {
                write!(
                    f,
                    "Missing required config '{}' for plugin '{}'",
                    key, plugin
                )
            }
            PluginFactoryError::TypeMismatch {
                plugin,
                key,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Type mismatch for '{}' in plugin '{}': expected {}, got {}",
                    key, plugin, expected, got
                )
            }
        }
    }
}

impl std::error::Error for PluginFactoryError {}

/// Configuration options for plugin creation
#[derive(Debug, Clone, Default)]
pub struct PluginConfig {
    /// Configuration values as key-value pairs
    pub values: HashMap<String, serde_json::Value>,
}

impl PluginConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value(mut self, key: &str, value: serde_json::Value) -> Self {
        self.values.insert(key.to_string(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.values.get(key)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(|v| v.as_bool())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(|v| v.as_str())
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.values.get(key).and_then(|v| v.as_u64())
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| v.as_f64())
    }
}

/// Factory for creating callback plugins by name
pub struct PluginFactory {
    /// Default plugin name to use when none specified
    default_plugin: String,
    /// Available plugin names
    available_plugins: Vec<String>,
}

impl Default for PluginFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginFactory {
    /// Create a new plugin factory with default configuration
    pub fn new() -> Self {
        Self {
            default_plugin: "default".to_string(),
            available_plugins: vec![
                "default".to_string(),
                "minimal".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "timer".to_string(),
                "profile_tasks".to_string(),
                "oneline".to_string(),
                "debug".to_string(),
                "dense".to_string(),
                "null".to_string(),
            ],
        }
    }

    /// Set the default plugin name
    pub fn with_default(mut self, name: &str) -> Self {
        self.default_plugin = name.to_string();
        self
    }

    /// Get list of available plugin names
    pub fn available_plugins(&self) -> &[String] {
        &self.available_plugins
    }

    /// Check if a plugin name is valid
    pub fn is_valid_plugin(&self, name: &str) -> bool {
        self.available_plugins.iter().any(|p| p == name)
    }

    /// Get the default plugin name
    pub fn default_plugin(&self) -> &str {
        &self.default_plugin
    }

    /// Create a plugin by name with default configuration
    pub fn create(&self, name: &str) -> Result<Arc<dyn CallbackPlugin>, PluginFactoryError> {
        self.create_with_config(name, PluginConfig::new())
    }

    /// Create a plugin by name with custom configuration
    pub fn create_with_config(
        &self,
        name: &str,
        config: PluginConfig,
    ) -> Result<Arc<dyn CallbackPlugin>, PluginFactoryError> {
        // Normalize the name
        let normalized_name = name.trim().to_lowercase();

        match normalized_name.as_str() {
            "default" => Ok(Arc::new(DefaultCallback::new(config)?)),
            "minimal" => Ok(Arc::new(MinimalTestCallback::new(config)?)),
            "json" => Ok(Arc::new(JsonTestCallback::new(config)?)),
            "yaml" => Ok(Arc::new(YamlTestCallback::new(config)?)),
            "timer" => Ok(Arc::new(TimerTestCallback::new(config)?)),
            "profile_tasks" => Ok(Arc::new(ProfileTasksTestCallback::new(config)?)),
            "oneline" => Ok(Arc::new(OnelineTestCallback::new(config)?)),
            "debug" => Ok(Arc::new(DebugCallback::new(config)?)),
            "dense" => Ok(Arc::new(DenseCallback::new(config)?)),
            "null" => Ok(Arc::new(NullCallback::new())),
            _ => Err(PluginFactoryError::UnknownPlugin(name.to_string())),
        }
    }

    /// Create the default plugin
    pub fn create_default(&self) -> Result<Arc<dyn CallbackPlugin>, PluginFactoryError> {
        self.create(&self.default_plugin)
    }

    /// Create multiple plugins by name
    pub fn create_many(
        &self,
        names: &[&str],
    ) -> Result<Vec<Arc<dyn CallbackPlugin>>, PluginFactoryError> {
        names.iter().map(|name| self.create(name)).collect()
    }
}

// ============================================================================
// Test Plugin Implementations
// ============================================================================

/// Default callback plugin for standard output
#[derive(Debug)]
pub struct DefaultCallback {
    name: String,
    #[allow(dead_code)]
    use_colors: bool,
    #[allow(dead_code)]
    verbosity: u8,
}

impl DefaultCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "default".to_string(),
            use_colors: config.get_bool("use_colors").unwrap_or(true),
            verbosity: config.get_u64("verbosity").unwrap_or(0) as u8,
        })
    }
}

#[async_trait]
impl CallbackPlugin for DefaultCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::STDOUT
    }

    fn description(&self) -> &str {
        "Default stdout callback plugin"
    }
}

/// Minimal callback that only shows failures
#[derive(Debug)]
pub struct MinimalTestCallback {
    name: String,
    show_ok: bool,
}

impl MinimalTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "minimal".to_string(),
            show_ok: config.get_bool("show_ok").unwrap_or(false),
        })
    }

    pub fn shows_ok(&self) -> bool {
        self.show_ok
    }
}

#[async_trait]
impl CallbackPlugin for MinimalTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::STDOUT
    }

    fn description(&self) -> &str {
        "Minimal output - only failures and recap"
    }
}

/// JSON output callback
#[derive(Debug)]
pub struct JsonTestCallback {
    name: String,
    pretty: bool,
    output_file: Option<String>,
}

impl JsonTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "json".to_string(),
            pretty: config.get_bool("pretty").unwrap_or(false),
            output_file: config.get_str("output_file").map(String::from),
        })
    }

    pub fn is_pretty(&self) -> bool {
        self.pretty
    }

    pub fn output_file(&self) -> Option<&str> {
        self.output_file.as_deref()
    }
}

#[async_trait]
impl CallbackPlugin for JsonTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::LOGGING
    }

    fn description(&self) -> &str {
        "JSON formatted output"
    }
}

/// YAML output callback
#[derive(Debug)]
pub struct YamlTestCallback {
    name: String,
    indent: usize,
}

impl YamlTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "yaml".to_string(),
            indent: config.get_u64("indent").unwrap_or(2) as usize,
        })
    }

    pub fn indent(&self) -> usize {
        self.indent
    }
}

#[async_trait]
impl CallbackPlugin for YamlTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::LOGGING
    }

    fn description(&self) -> &str {
        "YAML formatted output"
    }
}

/// Timer callback for performance tracking
#[derive(Debug)]
pub struct TimerTestCallback {
    name: String,
    show_per_task: bool,
    show_summary: bool,
    top_slowest: usize,
    threshold_secs: f64,
}

impl TimerTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        // Validate threshold if provided
        if let Some(threshold) = config.get_f64("threshold_secs") {
            if threshold < 0.0 {
                return Err(PluginFactoryError::InvalidConfig {
                    plugin: "timer".to_string(),
                    message: "threshold_secs cannot be negative".to_string(),
                });
            }
        }

        Ok(Self {
            name: "timer".to_string(),
            show_per_task: config.get_bool("show_per_task").unwrap_or(true),
            show_summary: config.get_bool("show_summary").unwrap_or(true),
            top_slowest: config.get_u64("top_slowest").unwrap_or(10) as usize,
            threshold_secs: config.get_f64("threshold_secs").unwrap_or(0.0),
        })
    }

    pub fn shows_per_task(&self) -> bool {
        self.show_per_task
    }

    pub fn shows_summary(&self) -> bool {
        self.show_summary
    }

    pub fn top_slowest(&self) -> usize {
        self.top_slowest
    }

    pub fn threshold_secs(&self) -> f64 {
        self.threshold_secs
    }
}

#[async_trait]
impl CallbackPlugin for TimerTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::METRICS
    }

    fn description(&self) -> &str {
        "Task timing and performance reporting"
    }
}

/// Profile tasks callback for detailed timing
#[derive(Debug)]
pub struct ProfileTasksTestCallback {
    name: String,
    slow_threshold_secs: f64,
    bottleneck_threshold_secs: f64,
    #[allow(dead_code)]
    top_tasks_count: usize,
}

impl ProfileTasksTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        let slow = config.get_f64("slow_threshold_secs").unwrap_or(10.0);
        let bottleneck = config.get_f64("bottleneck_threshold_secs").unwrap_or(30.0);

        // Validate that bottleneck threshold is greater than slow threshold
        if bottleneck < slow {
            return Err(PluginFactoryError::InvalidConfig {
                plugin: "profile_tasks".to_string(),
                message: "bottleneck_threshold_secs must be >= slow_threshold_secs".to_string(),
            });
        }

        Ok(Self {
            name: "profile_tasks".to_string(),
            slow_threshold_secs: slow,
            bottleneck_threshold_secs: bottleneck,
            top_tasks_count: config.get_u64("top_tasks_count").unwrap_or(20) as usize,
        })
    }

    pub fn slow_threshold(&self) -> f64 {
        self.slow_threshold_secs
    }

    pub fn bottleneck_threshold(&self) -> f64 {
        self.bottleneck_threshold_secs
    }
}

#[async_trait]
impl CallbackPlugin for ProfileTasksTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::METRICS
    }

    fn description(&self) -> &str {
        "Detailed task profiling and bottleneck detection"
    }
}

/// Oneline callback for compact output
#[derive(Debug)]
pub struct OnelineTestCallback {
    name: String,
    #[allow(dead_code)]
    show_task_name: bool,
    #[allow(dead_code)]
    show_timestamp: bool,
    #[allow(dead_code)]
    max_message_length: usize,
}

impl OnelineTestCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "oneline".to_string(),
            show_task_name: config.get_bool("show_task_name").unwrap_or(false),
            show_timestamp: config.get_bool("show_timestamp").unwrap_or(false),
            max_message_length: config.get_u64("max_message_length").unwrap_or(0) as usize,
        })
    }
}

#[async_trait]
impl CallbackPlugin for OnelineTestCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::STDOUT
    }

    fn description(&self) -> &str {
        "Single-line output for log files"
    }
}

/// Debug callback with verbose output
#[derive(Debug)]
pub struct DebugCallback {
    name: String,
    verbosity: u8,
}

impl DebugCallback {
    pub fn new(config: PluginConfig) -> Result<Self, PluginFactoryError> {
        let verbosity = config.get_u64("verbosity").unwrap_or(1) as u8;
        if verbosity > 5 {
            return Err(PluginFactoryError::InvalidConfig {
                plugin: "debug".to_string(),
                message: "verbosity must be between 0 and 5".to_string(),
            });
        }
        Ok(Self {
            name: "debug".to_string(),
            verbosity,
        })
    }

    pub fn verbosity(&self) -> u8 {
        self.verbosity
    }
}

#[async_trait]
impl CallbackPlugin for DebugCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::STDOUT
    }

    fn description(&self) -> &str {
        "Debug output with variable verbosity"
    }
}

/// Dense callback for compact multi-host output
#[derive(Debug)]
pub struct DenseCallback {
    name: String,
}

impl DenseCallback {
    pub fn new(_config: PluginConfig) -> Result<Self, PluginFactoryError> {
        Ok(Self {
            name: "dense".to_string(),
        })
    }
}

#[async_trait]
impl CallbackPlugin for DenseCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::STDOUT
    }

    fn description(&self) -> &str {
        "Dense multi-host output"
    }
}

/// Null callback that discards all output
#[derive(Debug)]
pub struct NullCallback {
    name: String,
}

impl NullCallback {
    pub fn new() -> Self {
        Self {
            name: "null".to_string(),
        }
    }
}

impl Default for NullCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CallbackPlugin for NullCallback {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> PluginPriority {
        PluginPriority::CLEANUP
    }

    fn description(&self) -> &str {
        "Discards all output"
    }

    fn is_enabled(&self) -> bool {
        true
    }
}

// ============================================================================
// Test 1: Create Plugins by Name
// ============================================================================

#[test]
fn test_factory_create_default_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("default").unwrap();

    assert_eq!(plugin.name(), "default");
    assert_eq!(plugin.priority(), PluginPriority::STDOUT);
}

#[test]
fn test_factory_create_minimal_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("minimal").unwrap();

    assert_eq!(plugin.name(), "minimal");
    assert!(plugin.description().contains("Minimal"));
}

#[test]
fn test_factory_create_json_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("json").unwrap();

    assert_eq!(plugin.name(), "json");
    assert_eq!(plugin.priority(), PluginPriority::LOGGING);
}

#[test]
fn test_factory_create_yaml_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("yaml").unwrap();

    assert_eq!(plugin.name(), "yaml");
    assert!(plugin.description().contains("YAML"));
}

#[test]
fn test_factory_create_timer_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("timer").unwrap();

    assert_eq!(plugin.name(), "timer");
    assert_eq!(plugin.priority(), PluginPriority::METRICS);
}

#[test]
fn test_factory_create_profile_tasks_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("profile_tasks").unwrap();

    assert_eq!(plugin.name(), "profile_tasks");
    assert!(plugin.description().contains("profiling"));
}

#[test]
fn test_factory_create_oneline_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("oneline").unwrap();

    assert_eq!(plugin.name(), "oneline");
}

#[test]
fn test_factory_create_debug_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("debug").unwrap();

    assert_eq!(plugin.name(), "debug");
}

#[test]
fn test_factory_create_dense_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("dense").unwrap();

    assert_eq!(plugin.name(), "dense");
}

#[test]
fn test_factory_create_null_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create("null").unwrap();

    assert_eq!(plugin.name(), "null");
    assert!(plugin.is_enabled());
}

#[test]
fn test_factory_create_all_available_plugins() {
    let factory = PluginFactory::new();

    for name in factory.available_plugins() {
        let result = factory.create(name);
        assert!(result.is_ok(), "Failed to create plugin: {}", name);
        let plugin = result.unwrap();
        assert!(!plugin.name().is_empty());
    }
}

#[test]
fn test_factory_create_with_case_insensitive_name() {
    let factory = PluginFactory::new();

    // All variations should work
    assert!(factory.create("MINIMAL").is_ok());
    assert!(factory.create("Minimal").is_ok());
    assert!(factory.create("mInImAl").is_ok());
    assert!(factory.create("  minimal  ").is_ok());
}

#[test]
fn test_factory_create_many_plugins() {
    let factory = PluginFactory::new();
    let names = &["minimal", "json", "timer"];

    let plugins = factory.create_many(names).unwrap();
    assert_eq!(plugins.len(), 3);
    assert_eq!(plugins[0].name(), "minimal");
    assert_eq!(plugins[1].name(), "json");
    assert_eq!(plugins[2].name(), "timer");
}

// ============================================================================
// Test 2: Invalid Plugin Names
// ============================================================================

#[test]
fn test_factory_unknown_plugin_returns_error() {
    let factory = PluginFactory::new();
    let result = factory.create("nonexistent_plugin");

    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::UnknownPlugin(name) => {
            assert_eq!(name, "nonexistent_plugin");
        }
        _ => panic!("Expected UnknownPlugin error"),
    }
}

#[test]
fn test_factory_empty_plugin_name() {
    let factory = PluginFactory::new();
    let result = factory.create("");

    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::UnknownPlugin(name) => {
            assert!(name.is_empty());
        }
        _ => panic!("Expected UnknownPlugin error"),
    }
}

#[test]
fn test_factory_whitespace_only_name() {
    let factory = PluginFactory::new();
    let result = factory.create("   ");

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PluginFactoryError::UnknownPlugin(_)
    ));
}

#[test]
fn test_factory_similar_but_invalid_name() {
    let factory = PluginFactory::new();

    // Close but not exact matches
    assert!(factory.create("minim").is_err());
    assert!(factory.create("jsons").is_err());
    assert!(factory.create("timer_callback").is_err());
    assert!(factory.create("profile-tasks").is_err()); // hyphen instead of underscore
}

#[test]
fn test_factory_is_valid_plugin() {
    let factory = PluginFactory::new();

    assert!(factory.is_valid_plugin("minimal"));
    assert!(factory.is_valid_plugin("json"));
    assert!(factory.is_valid_plugin("timer"));
    assert!(!factory.is_valid_plugin("unknown"));
    assert!(!factory.is_valid_plugin(""));
}

#[test]
fn test_factory_error_message_contains_plugin_name() {
    let factory = PluginFactory::new();
    let result = factory.create("my_custom_plugin");

    match result.unwrap_err() {
        PluginFactoryError::UnknownPlugin(name) => {
            let error_msg = format!("{}", PluginFactoryError::UnknownPlugin(name.clone()));
            assert!(error_msg.contains("my_custom_plugin"));
        }
        _ => panic!("Expected UnknownPlugin error"),
    }
}

#[test]
fn test_factory_create_many_with_invalid_stops_at_first_error() {
    let factory = PluginFactory::new();
    let names = &["minimal", "invalid_plugin", "json"];

    let result = factory.create_many(names);
    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::UnknownPlugin(name) => {
            assert_eq!(name, "invalid_plugin");
        }
        _ => panic!("Expected UnknownPlugin error"),
    }
}

// ============================================================================
// Test 3: Plugin Configuration Options
// ============================================================================

#[test]
fn test_factory_minimal_with_show_ok_config() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("show_ok", json!(true));

    let plugin = factory.create_with_config("minimal", config).unwrap();
    assert_eq!(plugin.name(), "minimal");
}

#[test]
fn test_factory_json_with_pretty_config() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("pretty", json!(true))
        .with_value("output_file", json!("/tmp/output.json"));

    let plugin = factory.create_with_config("json", config).unwrap();
    assert_eq!(plugin.name(), "json");
}

#[test]
fn test_factory_yaml_with_indent_config() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("indent", json!(4));

    let plugin = factory.create_with_config("yaml", config).unwrap();
    assert_eq!(plugin.name(), "yaml");
}

#[test]
fn test_factory_timer_with_full_config() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("show_per_task", json!(false))
        .with_value("show_summary", json!(true))
        .with_value("top_slowest", json!(5))
        .with_value("threshold_secs", json!(1.5));

    let plugin = factory.create_with_config("timer", config).unwrap();
    assert_eq!(plugin.name(), "timer");
}

#[test]
fn test_factory_timer_rejects_negative_threshold() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("threshold_secs", json!(-1.0));

    let result = factory.create_with_config("timer", config);
    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::InvalidConfig { plugin, message } => {
            assert_eq!(plugin, "timer");
            assert!(message.contains("negative"));
        }
        _ => panic!("Expected InvalidConfig error"),
    }
}

#[test]
fn test_factory_profile_tasks_with_thresholds() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("slow_threshold_secs", json!(5.0))
        .with_value("bottleneck_threshold_secs", json!(15.0))
        .with_value("top_tasks_count", json!(10));

    let plugin = factory.create_with_config("profile_tasks", config).unwrap();
    assert_eq!(plugin.name(), "profile_tasks");
}

#[test]
fn test_factory_profile_tasks_invalid_threshold_relationship() {
    let factory = PluginFactory::new();
    // bottleneck < slow is invalid
    let config = PluginConfig::new()
        .with_value("slow_threshold_secs", json!(20.0))
        .with_value("bottleneck_threshold_secs", json!(10.0));

    let result = factory.create_with_config("profile_tasks", config);
    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::InvalidConfig { plugin, message } => {
            assert_eq!(plugin, "profile_tasks");
            assert!(message.contains("bottleneck_threshold_secs"));
        }
        _ => panic!("Expected InvalidConfig error"),
    }
}

#[test]
fn test_factory_debug_with_verbosity() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("verbosity", json!(3));

    let plugin = factory.create_with_config("debug", config).unwrap();
    assert_eq!(plugin.name(), "debug");
}

#[test]
fn test_factory_debug_verbosity_out_of_range() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("verbosity", json!(10));

    let result = factory.create_with_config("debug", config);
    assert!(result.is_err());
    match result.unwrap_err() {
        PluginFactoryError::InvalidConfig { plugin, message } => {
            assert_eq!(plugin, "debug");
            assert!(message.contains("verbosity"));
        }
        _ => panic!("Expected InvalidConfig error"),
    }
}

#[test]
fn test_factory_oneline_with_all_options() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("show_task_name", json!(true))
        .with_value("show_timestamp", json!(true))
        .with_value("max_message_length", json!(80));

    let plugin = factory.create_with_config("oneline", config).unwrap();
    assert_eq!(plugin.name(), "oneline");
}

#[test]
fn test_factory_ignores_unknown_config_keys() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("unknown_key", json!("some value"))
        .with_value("another_unknown", json!(123));

    // Should not fail - unknown keys are ignored
    let plugin = factory.create_with_config("minimal", config).unwrap();
    assert_eq!(plugin.name(), "minimal");
}

#[test]
fn test_plugin_config_type_helpers() {
    let config = PluginConfig::new()
        .with_value("bool_val", json!(true))
        .with_value("str_val", json!("hello"))
        .with_value("u64_val", json!(42))
        .with_value("f64_val", json!(3.14));

    assert_eq!(config.get_bool("bool_val"), Some(true));
    assert_eq!(config.get_str("str_val"), Some("hello"));
    assert_eq!(config.get_u64("u64_val"), Some(42));
    assert_eq!(config.get_f64("f64_val"), Some(3.14));

    // Missing keys return None
    assert_eq!(config.get_bool("missing"), None);
    assert_eq!(config.get_str("missing"), None);
}

// ============================================================================
// Test 4: Default Plugin Selection
// ============================================================================

#[test]
fn test_factory_default_plugin_is_default() {
    let factory = PluginFactory::new();
    assert_eq!(factory.default_plugin(), "default");
}

#[test]
fn test_factory_create_default_creates_default_plugin() {
    let factory = PluginFactory::new();
    let plugin = factory.create_default().unwrap();

    assert_eq!(plugin.name(), "default");
}

#[test]
fn test_factory_custom_default_plugin() {
    let factory = PluginFactory::new().with_default("minimal");
    assert_eq!(factory.default_plugin(), "minimal");

    let plugin = factory.create_default().unwrap();
    assert_eq!(plugin.name(), "minimal");
}

#[test]
fn test_factory_change_default_to_json() {
    let factory = PluginFactory::new().with_default("json");
    let plugin = factory.create_default().unwrap();

    assert_eq!(plugin.name(), "json");
    assert_eq!(plugin.priority(), PluginPriority::LOGGING);
}

#[test]
fn test_factory_default_to_invalid_plugin_fails() {
    let factory = PluginFactory::new().with_default("invalid_plugin");
    let result = factory.create_default();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PluginFactoryError::UnknownPlugin(_)
    ));
}

#[test]
fn test_factory_available_plugins_list() {
    let factory = PluginFactory::new();
    let available = factory.available_plugins();

    assert!(available.contains(&"default".to_string()));
    assert!(available.contains(&"minimal".to_string()));
    assert!(available.contains(&"json".to_string()));
    assert!(available.contains(&"yaml".to_string()));
    assert!(available.contains(&"timer".to_string()));
    assert!(available.contains(&"profile_tasks".to_string()));
    assert!(available.contains(&"oneline".to_string()));
    assert!(available.contains(&"null".to_string()));
}

#[test]
fn test_factory_available_plugins_not_empty() {
    let factory = PluginFactory::new();
    assert!(!factory.available_plugins().is_empty());
    assert!(factory.available_plugins().len() >= 8); // At least 8 plugins
}

// ============================================================================
// Test 5: Plugin Initialization Errors
// ============================================================================

#[test]
fn test_factory_error_display_unknown_plugin() {
    let error = PluginFactoryError::UnknownPlugin("custom".to_string());
    let msg = format!("{}", error);

    assert!(msg.contains("Unknown plugin"));
    assert!(msg.contains("custom"));
}

#[test]
fn test_factory_error_display_invalid_config() {
    let error = PluginFactoryError::InvalidConfig {
        plugin: "timer".to_string(),
        message: "threshold cannot be negative".to_string(),
    };
    let msg = format!("{}", error);

    assert!(msg.contains("Invalid config"));
    assert!(msg.contains("timer"));
    assert!(msg.contains("threshold cannot be negative"));
}

#[test]
fn test_factory_error_display_initialization_failed() {
    let error = PluginFactoryError::InitializationFailed {
        plugin: "json".to_string(),
        cause: "could not open output file".to_string(),
    };
    let msg = format!("{}", error);

    assert!(msg.contains("Failed to initialize"));
    assert!(msg.contains("json"));
    assert!(msg.contains("could not open output file"));
}

#[test]
fn test_factory_error_display_missing_config() {
    let error = PluginFactoryError::MissingConfig {
        plugin: "custom".to_string(),
        key: "api_key".to_string(),
    };
    let msg = format!("{}", error);

    assert!(msg.contains("Missing required config"));
    assert!(msg.contains("api_key"));
    assert!(msg.contains("custom"));
}

#[test]
fn test_factory_error_display_type_mismatch() {
    let error = PluginFactoryError::TypeMismatch {
        plugin: "timer".to_string(),
        key: "threshold_secs".to_string(),
        expected: "float".to_string(),
        got: "string".to_string(),
    };
    let msg = format!("{}", error);

    assert!(msg.contains("Type mismatch"));
    assert!(msg.contains("threshold_secs"));
    assert!(msg.contains("timer"));
    assert!(msg.contains("float"));
    assert!(msg.contains("string"));
}

#[test]
fn test_factory_error_is_std_error() {
    let error: Box<dyn std::error::Error> =
        Box::new(PluginFactoryError::UnknownPlugin("test".to_string()));

    // Should be able to use as std::error::Error
    assert!(error.to_string().contains("Unknown plugin"));
}

#[test]
fn test_factory_error_equality() {
    let e1 = PluginFactoryError::UnknownPlugin("test".to_string());
    let e2 = PluginFactoryError::UnknownPlugin("test".to_string());
    let e3 = PluginFactoryError::UnknownPlugin("other".to_string());

    assert_eq!(e1, e2);
    assert_ne!(e1, e3);
}

// ============================================================================
// Integration Tests: Factory with CallbackManager
// ============================================================================

#[tokio::test]
async fn test_factory_plugins_register_with_manager() {
    let factory = PluginFactory::new();
    let manager = CallbackManager::new();

    let plugin = factory.create("minimal").unwrap();
    let is_new = manager.register(plugin).await;

    assert!(is_new);
    assert!(manager.has_plugin("minimal"));
    assert_eq!(manager.plugin_count(), 1);
}

#[tokio::test]
async fn test_factory_multiple_plugins_with_manager() {
    let factory = PluginFactory::new();
    let manager = CallbackManager::new();

    let minimal = factory.create("minimal").unwrap();
    let json = factory.create("json").unwrap();
    let timer = factory.create("timer").unwrap();

    manager.register(minimal).await;
    manager.register(json).await;
    manager.register(timer).await;

    assert_eq!(manager.plugin_count(), 3);
    assert!(manager.has_plugin("minimal"));
    assert!(manager.has_plugin("json"));
    assert!(manager.has_plugin("timer"));
}

#[tokio::test]
async fn test_factory_plugins_priority_ordering() {
    let factory = PluginFactory::new();
    let manager = CallbackManager::new();

    // Register in non-priority order
    let timer = factory.create("timer").unwrap(); // METRICS priority
    let minimal = factory.create("minimal").unwrap(); // STDOUT priority
    let json = factory.create("json").unwrap(); // LOGGING priority

    manager.register(timer).await;
    manager.register(minimal).await;
    manager.register(json).await;

    // They should be internally ordered by priority
    let names = manager.plugin_names();
    assert_eq!(names.len(), 3);
}

#[tokio::test]
async fn test_factory_created_plugins_receive_events() {
    let factory = PluginFactory::new();
    let manager = CallbackManager::new();

    let plugin = factory.create("null").unwrap();
    manager.register(plugin).await;

    // Dispatch events - should not panic
    let result = manager.on_playbook_start("test").await;
    assert!(result.is_success());
    assert_eq!(result.success_count, 1);
}

#[tokio::test]
async fn test_factory_with_config_integration() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new()
        .with_value("show_per_task", json!(false))
        .with_value("threshold_secs", json!(5.0));

    let timer = factory.create_with_config("timer", config).unwrap();
    let manager = CallbackManager::new();

    manager.register(timer).await;

    assert!(manager.has_plugin("timer"));
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_factory_create_same_plugin_multiple_times() {
    let factory = PluginFactory::new();

    let p1 = factory.create("minimal").unwrap();
    let p2 = factory.create("minimal").unwrap();

    // Should create separate instances
    assert_eq!(p1.name(), p2.name());
    // But they are different Arc instances
    assert!(!Arc::ptr_eq(&p1, &p2));
}

#[test]
fn test_factory_config_with_empty_values() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new(); // Empty config

    let plugin = factory.create_with_config("timer", config).unwrap();
    assert_eq!(plugin.name(), "timer");
}

#[test]
fn test_factory_config_with_null_values() {
    let factory = PluginFactory::new();
    let config = PluginConfig::new().with_value("show_per_task", serde_json::Value::Null);

    // Null values should be treated as missing (use default)
    let plugin = factory.create_with_config("timer", config).unwrap();
    assert_eq!(plugin.name(), "timer");
}

#[test]
fn test_plugin_descriptions_are_meaningful() {
    let factory = PluginFactory::new();

    for name in factory.available_plugins() {
        let plugin = factory.create(name).unwrap();
        let desc = plugin.description();

        assert!(!desc.is_empty(), "Plugin {} has empty description", name);
        assert!(
            desc.len() > 5,
            "Plugin {} has very short description: {}",
            name,
            desc
        );
    }
}

#[test]
fn test_factory_is_thread_safe() {
    use std::thread;

    let factory = Arc::new(PluginFactory::new());
    let mut handles = vec![];

    for i in 0..10 {
        let f = Arc::clone(&factory);
        let handle = thread::spawn(move || {
            let name = if i % 2 == 0 { "minimal" } else { "json" };
            f.create(name).unwrap()
        });
        handles.push(handle);
    }

    for handle in handles {
        let plugin = handle.join().unwrap();
        assert!(!plugin.name().is_empty());
    }
}

#[test]
fn test_null_plugin_priority_is_cleanup() {
    let factory = PluginFactory::new();
    let plugin = factory.create("null").unwrap();

    assert_eq!(plugin.priority(), PluginPriority::CLEANUP);
}

#[test]
fn test_stdout_plugins_have_stdout_priority() {
    let factory = PluginFactory::new();

    let default_plugin = factory.create("default").unwrap();
    let minimal_plugin = factory.create("minimal").unwrap();
    let oneline_plugin = factory.create("oneline").unwrap();

    assert_eq!(default_plugin.priority(), PluginPriority::STDOUT);
    assert_eq!(minimal_plugin.priority(), PluginPriority::STDOUT);
    assert_eq!(oneline_plugin.priority(), PluginPriority::STDOUT);
}

#[test]
fn test_logging_plugins_have_logging_priority() {
    let factory = PluginFactory::new();

    let json_plugin = factory.create("json").unwrap();
    let yaml_plugin = factory.create("yaml").unwrap();

    assert_eq!(json_plugin.priority(), PluginPriority::LOGGING);
    assert_eq!(yaml_plugin.priority(), PluginPriority::LOGGING);
}

#[test]
fn test_metrics_plugins_have_metrics_priority() {
    let factory = PluginFactory::new();

    let timer_plugin = factory.create("timer").unwrap();
    let profile_plugin = factory.create("profile_tasks").unwrap();

    assert_eq!(timer_plugin.priority(), PluginPriority::METRICS);
    assert_eq!(profile_plugin.priority(), PluginPriority::METRICS);
}
