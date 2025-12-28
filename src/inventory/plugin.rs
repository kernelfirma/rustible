//! Inventory Plugin System for Rustible
//!
//! This module provides a plugin-based architecture for extending inventory sources.
//! It supports:
//! - Static inventory files (INI, YAML, JSON)
//! - Dynamic inventory scripts
//! - Custom inventory plugins (AWS EC2, GCP, Azure, etc.)
//! - Inventory caching for improved performance
//!
//! # Architecture
//!
//! The plugin system consists of:
//! - [`InventoryPlugin`] trait: Core trait for inventory sources
//! - [`InventoryPluginFactory`]: Factory for creating plugins by name
//! - [`InventoryPluginRegistry`]: Registry for custom plugins
//! - [`InventoryCache`]: Caching layer for inventory data
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::inventory::plugin::{InventoryPluginFactory, InventoryPluginConfig};
//!
//! // Create a plugin with configuration
//! let config = InventoryPluginConfig::new()
//!     .with_option("region", "us-east-1")
//!     .with_cache_ttl(Duration::from_secs(300));
//!
//! let plugin = InventoryPluginFactory::create("aws_ec2", config)?;
//! let inventory = plugin.get_inventory().await?;
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::{Inventory, InventoryError, InventoryResult};

// ============================================================================
// Plugin Configuration
// ============================================================================

/// Configuration for inventory plugins
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryPluginConfig {
    /// Plugin-specific options
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,

    /// Cache TTL in seconds (0 = no caching)
    #[serde(default)]
    pub cache_ttl_secs: u64,

    /// Whether to enable caching
    #[serde(default)]
    pub cache_enabled: bool,

    /// Path to cache file (for persistent caching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<PathBuf>,

    /// Whether to compose with other inventory sources
    #[serde(default)]
    pub compose: bool,

    /// Keyed groups configuration for dynamic grouping
    #[serde(default)]
    pub keyed_groups: Vec<KeyedGroup>,

    /// Groups configuration for static group assignment
    #[serde(default)]
    pub groups: HashMap<String, String>,

    /// Host filters (Jinja2 expressions)
    #[serde(default)]
    pub filters: Vec<String>,

    /// Strict mode - fail on template errors
    #[serde(default)]
    pub strict: bool,
}

impl InventoryPluginConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a configuration option
    pub fn with_option(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// Set cache TTL
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl_secs = ttl.as_secs();
        self.cache_enabled = true;
        self
    }

    /// Enable caching with a specific path
    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(path.into());
        self.cache_enabled = true;
        self
    }

    /// Enable compose mode
    pub fn with_compose(mut self) -> Self {
        self.compose = true;
        self
    }

    /// Add a keyed group
    pub fn with_keyed_group(mut self, group: KeyedGroup) -> Self {
        self.keyed_groups.push(group);
        self
    }

    /// Add a filter expression
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filters.push(filter.into());
        self
    }

    /// Get an option value as a string
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.options
            .get(key)
            .and_then(|v| v.as_str().map(String::from))
    }

    /// Get an option value as a boolean
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(|v| v.as_bool())
    }

    /// Get an option value as an integer
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.options.get(key).and_then(|v| v.as_i64())
    }

    /// Get an option value as a list of strings
    pub fn get_string_list(&self, key: &str) -> Option<Vec<String>> {
        self.options.get(key).and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
        })
    }

    /// Get cache TTL as Duration
    pub fn cache_ttl(&self) -> Duration {
        Duration::from_secs(self.cache_ttl_secs)
    }
}

/// Keyed group configuration for dynamic group creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyedGroup {
    /// Key expression (Jinja2)
    pub key: String,

    /// Prefix for the group name
    #[serde(default)]
    pub prefix: String,

    /// Separator between prefix and key value
    #[serde(default = "default_separator")]
    pub separator: String,

    /// Parent group for all created groups
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_group: Option<String>,

    /// Default value if key is not found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,

    /// Trailing separator behavior
    #[serde(default)]
    pub trailing_separator: bool,
}

fn default_separator() -> String {
    "_".to_string()
}

impl KeyedGroup {
    /// Create a new keyed group
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            prefix: String::new(),
            separator: default_separator(),
            parent_group: None,
            default_value: None,
            trailing_separator: false,
        }
    }

    /// Set the prefix
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Set the separator
    pub fn with_separator(mut self, sep: impl Into<String>) -> Self {
        self.separator = sep.into();
        self
    }

    /// Set the parent group
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parent_group = Some(parent.into());
        self
    }
}

// ============================================================================
// Plugin Trait
// ============================================================================

/// Core trait for inventory plugins
///
/// Implement this trait to create custom inventory sources such as
/// cloud provider integrations, CMDB connections, or custom APIs.
#[async_trait]
pub trait InventoryPlugin: Send + Sync + fmt::Debug {
    /// Get the plugin name
    fn name(&self) -> &str;

    /// Get the plugin description
    fn description(&self) -> &str {
        "Custom inventory plugin"
    }

    /// Get the plugin version
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Verify that the plugin is properly configured
    fn verify(&self) -> InventoryResult<()> {
        Ok(())
    }

    /// Parse inventory file/source and return parsed inventory
    async fn parse(&self) -> InventoryResult<Inventory>;

    /// Get hosts as JSON (compatible with dynamic inventory scripts)
    async fn get_hosts_json(&self) -> InventoryResult<serde_json::Value> {
        let inventory = self.parse().await?;
        inventory_to_json(&inventory)
    }

    /// Get host variables for a specific host
    async fn get_host_vars(&self, hostname: &str) -> InventoryResult<serde_json::Value> {
        let inventory = self.parse().await?;
        if let Some(host) = inventory.get_host(hostname) {
            let vars: serde_json::Map<String, serde_json::Value> = host
                .vars
                .iter()
                .map(|(k, v)| {
                    let json_val = yaml_to_json(v);
                    (k.clone(), json_val)
                })
                .collect();
            Ok(serde_json::Value::Object(vars))
        } else {
            Ok(serde_json::Value::Object(serde_json::Map::new()))
        }
    }

    /// Refresh the inventory (for cached plugins)
    async fn refresh(&self) -> InventoryResult<()> {
        Ok(())
    }

    /// Get supported options for this plugin
    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        Vec::new()
    }
}

/// Information about a plugin option
#[derive(Debug, Clone)]
pub struct PluginOptionInfo {
    /// Option name
    pub name: String,
    /// Option description
    pub description: String,
    /// Whether the option is required
    pub required: bool,
    /// Default value (if any)
    pub default: Option<String>,
    /// Option type (string, bool, int, list)
    pub option_type: String,
}

impl PluginOptionInfo {
    /// Create a new required string option
    pub fn required_string(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: true,
            default: None,
            option_type: "string".to_string(),
        }
    }

    /// Create a new optional string option with a default
    pub fn optional_string(
        name: impl Into<String>,
        description: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: false,
            default: Some(default.into()),
            option_type: "string".to_string(),
        }
    }

    /// Create a new optional boolean option
    pub fn optional_bool(
        name: impl Into<String>,
        description: impl Into<String>,
        default: bool,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: false,
            default: Some(default.to_string()),
            option_type: "bool".to_string(),
        }
    }
}

// ============================================================================
// Plugin Factory
// ============================================================================

/// Error type for plugin factory operations
#[derive(Debug, Clone)]
pub struct PluginError {
    /// The kind of error
    pub kind: PluginErrorKind,
    /// Error message
    pub message: String,
}

/// Types of plugin errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginErrorKind {
    /// Plugin not found
    NotFound,
    /// Invalid configuration
    InvalidConfig,
    /// Plugin initialization failed
    InitFailed,
    /// Authentication error
    AuthError,
    /// Network error
    NetworkError,
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            PluginErrorKind::NotFound => write!(f, "Plugin not found: {}", self.message),
            PluginErrorKind::InvalidConfig => write!(f, "Invalid configuration: {}", self.message),
            PluginErrorKind::InitFailed => {
                write!(f, "Plugin initialization failed: {}", self.message)
            }
            PluginErrorKind::AuthError => write!(f, "Authentication error: {}", self.message),
            PluginErrorKind::NetworkError => write!(f, "Network error: {}", self.message),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<PluginError> for InventoryError {
    fn from(err: PluginError) -> Self {
        InventoryError::DynamicInventoryFailed(err.to_string())
    }
}

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

/// Information about an available plugin
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name
    pub name: &'static str,
    /// Plugin description
    pub description: &'static str,
    /// Plugin type/category
    pub plugin_type: PluginType,
    /// Required options
    pub required_options: Vec<&'static str>,
}

/// Type of inventory plugin
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    /// File-based inventory (INI, YAML, JSON)
    File,
    /// Script-based dynamic inventory
    Script,
    /// Cloud provider (AWS, GCP, Azure)
    Cloud,
    /// Container platform (Docker, Kubernetes)
    Container,
    /// Custom/third-party plugin
    Custom,
}

impl fmt::Display for PluginType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginType::File => write!(f, "file"),
            PluginType::Script => write!(f, "script"),
            PluginType::Cloud => write!(f, "cloud"),
            PluginType::Container => write!(f, "container"),
            PluginType::Custom => write!(f, "custom"),
        }
    }
}

/// Factory type for creating plugins
pub type PluginFactoryFn =
    Box<dyn Fn(InventoryPluginConfig) -> PluginResult<Arc<dyn InventoryPlugin>> + Send + Sync>;

/// Factory for creating inventory plugins
pub struct InventoryPluginFactory;

impl InventoryPluginFactory {
    /// Create a plugin by name
    pub fn create(
        name: &str,
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        let name_lower = name.to_lowercase();

        match name_lower.as_str() {
            // File-based plugins
            "file" | "ini" | "yaml" | "json" => Self::create_file_plugin(config),
            "script" | "dynamic" => Self::create_script_plugin(config),

            // Cloud plugins
            "aws_ec2" | "ec2" => Self::create_aws_ec2_plugin(config),

            // Container plugins
            #[cfg(feature = "docker")]
            "docker" => Self::create_docker_plugin(config),

            #[cfg(feature = "kubernetes")]
            "kubernetes" | "k8s" => Self::create_kubernetes_plugin(config),

            _ => Err(PluginError {
                kind: PluginErrorKind::NotFound,
                message: format!(
                    "'{}'. Available plugins: {}",
                    name,
                    Self::available_plugin_names().join(", ")
                ),
            }),
        }
    }

    /// Create a plugin with default configuration
    pub fn create_default(name: &str) -> PluginResult<Arc<dyn InventoryPlugin>> {
        Self::create(name, InventoryPluginConfig::default())
    }

    /// Get list of available plugin names
    pub fn available_plugin_names() -> Vec<&'static str> {
        let names = vec!["file", "ini", "yaml", "json", "script", "aws_ec2"];

        #[cfg(feature = "docker")]
        names.push("docker");

        #[cfg(feature = "kubernetes")]
        names.push("kubernetes");

        names
    }

    /// Get information about all available plugins
    pub fn available_plugins() -> Vec<PluginInfo> {
        let plugins = vec![
            PluginInfo {
                name: "file",
                description: "File-based inventory (INI, YAML, JSON)",
                plugin_type: PluginType::File,
                required_options: vec!["path"],
            },
            PluginInfo {
                name: "script",
                description: "Dynamic inventory script",
                plugin_type: PluginType::Script,
                required_options: vec!["path"],
            },
            PluginInfo {
                name: "aws_ec2",
                description: "AWS EC2 instances inventory",
                plugin_type: PluginType::Cloud,
                required_options: vec![],
            },
        ];

        #[cfg(feature = "docker")]
        plugins.push(PluginInfo {
            name: "docker",
            description: "Docker containers inventory",
            plugin_type: PluginType::Container,
            required_options: vec![],
        });

        #[cfg(feature = "kubernetes")]
        plugins.push(PluginInfo {
            name: "kubernetes",
            description: "Kubernetes pods/nodes inventory",
            plugin_type: PluginType::Container,
            required_options: vec![],
        });

        plugins
    }

    /// Check if a plugin exists
    pub fn plugin_exists(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        Self::available_plugin_names()
            .iter()
            .any(|n| n.to_lowercase() == name_lower)
    }

    // Private factory methods

    fn create_file_plugin(config: InventoryPluginConfig) -> PluginResult<Arc<dyn InventoryPlugin>> {
        let path = config.get_string("path").ok_or_else(|| PluginError {
            kind: PluginErrorKind::InvalidConfig,
            message: "Missing required option: path".to_string(),
        })?;

        Ok(Arc::new(FileInventoryPlugin::new(
            PathBuf::from(path),
            config,
        )))
    }

    fn create_script_plugin(
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        let path = config.get_string("path").ok_or_else(|| PluginError {
            kind: PluginErrorKind::InvalidConfig,
            message: "Missing required option: path".to_string(),
        })?;

        Ok(Arc::new(ScriptInventoryPlugin::new(
            PathBuf::from(path),
            config,
        )))
    }

    fn create_aws_ec2_plugin(
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        Ok(Arc::new(AwsEc2InventoryPlugin::new(config)))
    }

    #[cfg(feature = "docker")]
    fn create_docker_plugin(
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        Ok(Arc::new(DockerInventoryPlugin::new(config)))
    }

    #[cfg(feature = "kubernetes")]
    fn create_kubernetes_plugin(
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        Ok(Arc::new(KubernetesInventoryPlugin::new(config)))
    }
}

// ============================================================================
// Plugin Registry
// ============================================================================

/// Registry for custom inventory plugins
pub struct InventoryPluginRegistry {
    /// Registered plugin factories
    factories: HashMap<String, PluginFactoryFn>,
}

impl Default for InventoryPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InventoryPluginRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Create a registry with all built-in plugins
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        for name in InventoryPluginFactory::available_plugin_names() {
            let name_owned = name.to_string();
            registry.register(name, move |config| {
                InventoryPluginFactory::create(&name_owned, config)
            });
        }

        registry
    }

    /// Register a custom plugin factory
    pub fn register<F>(&mut self, name: &str, factory: F)
    where
        F: Fn(InventoryPluginConfig) -> PluginResult<Arc<dyn InventoryPlugin>>
            + Send
            + Sync
            + 'static,
    {
        self.factories
            .insert(name.to_lowercase(), Box::new(factory));
    }

    /// Unregister a plugin
    pub fn unregister(&mut self, name: &str) -> bool {
        self.factories.remove(&name.to_lowercase()).is_some()
    }

    /// Create a plugin by name
    pub fn create(
        &self,
        name: &str,
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>> {
        let name_lower = name.to_lowercase();

        if let Some(factory) = self.factories.get(&name_lower) {
            return factory(config);
        }

        // Fall back to built-in factory
        InventoryPluginFactory::create(name, config)
    }

    /// Check if a plugin is registered
    pub fn is_registered(&self, name: &str) -> bool {
        self.factories.contains_key(&name.to_lowercase())
    }

    /// List all registered plugin names
    pub fn registered_names(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }
}

impl fmt::Debug for InventoryPluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InventoryPluginRegistry")
            .field(
                "registered_plugins",
                &self.factories.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ============================================================================
// Built-in Plugins
// ============================================================================

/// File-based inventory plugin
#[derive(Debug)]
#[allow(dead_code)]
pub struct FileInventoryPlugin {
    path: PathBuf,
    config: InventoryPluginConfig,
}

impl FileInventoryPlugin {
    /// Create a new file inventory plugin
    pub fn new(path: PathBuf, config: InventoryPluginConfig) -> Self {
        Self { path, config }
    }
}

#[async_trait]
impl InventoryPlugin for FileInventoryPlugin {
    fn name(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "File-based inventory (INI, YAML, JSON)"
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        Inventory::load(&self.path)
    }

    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        vec![PluginOptionInfo::required_string(
            "path",
            "Path to the inventory file or directory",
        )]
    }
}

/// Script-based dynamic inventory plugin
#[derive(Debug)]
#[allow(dead_code)]
pub struct ScriptInventoryPlugin {
    path: PathBuf,
    config: InventoryPluginConfig,
}

impl ScriptInventoryPlugin {
    /// Create a new script inventory plugin
    pub fn new(path: PathBuf, config: InventoryPluginConfig) -> Self {
        Self { path, config }
    }
}

#[async_trait]
impl InventoryPlugin for ScriptInventoryPlugin {
    fn name(&self) -> &str {
        "script"
    }

    fn description(&self) -> &str {
        "Dynamic inventory script"
    }

    fn verify(&self) -> InventoryResult<()> {
        if !self.path.exists() {
            return Err(InventoryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Script not found: {}", self.path.display()),
            )));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&self.path)?;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(InventoryError::DynamicInventoryFailed(
                    "Script is not executable".to_string(),
                ));
            }
        }

        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;

        let output = tokio::process::Command::new(&self.path)
            .arg("--list")
            .output()
            .await
            .map_err(|e| InventoryError::DynamicInventoryFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(InventoryError::DynamicInventoryFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let mut inventory = Inventory::new();

        // Parse the JSON output into inventory
        let data: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| InventoryError::DynamicInventoryFailed(e.to_string()))?;

        parse_json_inventory(&mut inventory, &data)?;

        Ok(inventory)
    }

    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        vec![PluginOptionInfo::required_string(
            "path",
            "Path to the inventory script",
        )]
    }
}

/// AWS EC2 dynamic inventory plugin
#[derive(Debug)]
pub struct AwsEc2InventoryPlugin {
    config: InventoryPluginConfig,
}

impl AwsEc2InventoryPlugin {
    /// Create a new AWS EC2 inventory plugin
    pub fn new(config: InventoryPluginConfig) -> Self {
        Self { config }
    }

    /// Get the configured AWS region
    fn get_region(&self) -> String {
        self.config
            .get_string("region")
            .or_else(|| {
                self.config
                    .get_string("regions")
                    .map(|r| r.split(',').next().unwrap_or("us-east-1").to_string())
            })
            .unwrap_or_else(|| {
                std::env::var("AWS_REGION")
                    .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                    .unwrap_or_else(|_| "us-east-1".to_string())
            })
    }

    /// Get instance filters
    fn get_filters(&self) -> Vec<(String, String)> {
        let mut filters = Vec::new();

        // Default filter: only running instances
        if self.config.get_bool("include_stopped").unwrap_or(false) {
            filters.push((
                "instance-state-name".to_string(),
                "running,stopped".to_string(),
            ));
        } else {
            filters.push(("instance-state-name".to_string(), "running".to_string()));
        }

        // Tag filters from config
        if let Some(tags) = self.config.options.get("filters") {
            if let Some(tag_map) = tags.as_object() {
                for (key, value) in tag_map {
                    if let Some(v) = value.as_str() {
                        filters.push((format!("tag:{}", key), v.to_string()));
                    }
                }
            }
        }

        filters
    }

    /// Determine the hostname preference
    fn get_hostname_preference(&self) -> Vec<String> {
        self.config.get_string_list("hostnames").unwrap_or_else(|| {
            vec![
                "tag:Name".to_string(),
                "private-dns-name".to_string(),
                "dns-name".to_string(),
            ]
        })
    }
}

#[async_trait]
impl InventoryPlugin for AwsEc2InventoryPlugin {
    fn name(&self) -> &str {
        "aws_ec2"
    }

    fn description(&self) -> &str {
        "AWS EC2 instances inventory"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();

        // Note: This is a simplified implementation
        // A full implementation would use the AWS SDK for Rust
        // For now, we demonstrate the plugin structure

        let region = self.get_region();
        let _filters = self.get_filters();
        let _hostname_prefs = self.get_hostname_preference();

        // Create a placeholder group for EC2 instances
        let mut ec2_group = super::Group::new("aws_ec2");
        ec2_group.set_var("aws_region", serde_yaml::Value::String(region.clone()));

        // In a real implementation, we would:
        // 1. Use aws-sdk-ec2 to describe instances
        // 2. Create hosts from instance data
        // 3. Apply keyed_groups for dynamic grouping
        // 4. Apply filters and hostname preferences

        // Example of what the data structure would look like:
        // Each EC2 instance would become a Host with:
        // - name: instance hostname based on preferences
        // - ansible_host: private or public IP
        // - vars: instance metadata (instance_id, instance_type, tags, etc.)

        inventory.add_group(ec2_group)?;

        // Add info about how to use this plugin
        tracing::info!(
            "AWS EC2 inventory plugin initialized for region: {}. \
             Note: Full AWS SDK integration requires additional configuration.",
            region
        );

        Ok(inventory)
    }

    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        vec![
            PluginOptionInfo::optional_string(
                "region",
                "AWS region (defaults to AWS_REGION env var or us-east-1)",
                "us-east-1",
            ),
            PluginOptionInfo::optional_string(
                "regions",
                "Comma-separated list of AWS regions to query",
                "",
            ),
            PluginOptionInfo::optional_bool(
                "include_stopped",
                "Include stopped instances in inventory",
                false,
            ),
            PluginOptionInfo {
                name: "hostnames".to_string(),
                description: "List of hostname preferences (tag:Name, private-dns-name, etc.)"
                    .to_string(),
                required: false,
                default: Some("tag:Name,private-dns-name".to_string()),
                option_type: "list".to_string(),
            },
            PluginOptionInfo {
                name: "filters".to_string(),
                description: "EC2 instance filters (tag filters, instance-type, etc.)".to_string(),
                required: false,
                default: None,
                option_type: "dict".to_string(),
            },
            PluginOptionInfo::optional_bool(
                "use_private_ip",
                "Use private IP for ansible_host instead of public",
                true,
            ),
        ]
    }
}

// ============================================================================
// Inventory Cache
// ============================================================================

/// Cache entry with timestamp
#[derive(Debug, Clone)]
struct CacheEntry {
    inventory: Inventory,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_valid(&self) -> bool {
        self.created_at.elapsed() < self.ttl
    }
}

/// Inventory cache for improved performance
#[derive(Debug)]
pub struct InventoryCache {
    /// In-memory cache
    cache: RwLock<HashMap<String, CacheEntry>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Path for persistent cache storage
    cache_dir: Option<PathBuf>,
}

impl Default for InventoryCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl InventoryCache {
    /// Create a new cache with the specified TTL
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            default_ttl,
            cache_dir: None,
        }
    }

    /// Create a cache with persistent storage
    pub fn with_persistence(default_ttl: Duration, cache_dir: PathBuf) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            default_ttl,
            cache_dir: Some(cache_dir),
        }
    }

    /// Get an inventory from the cache
    pub async fn get(&self, key: &str) -> Option<Inventory> {
        let cache = self.cache.read().await;
        cache.get(key).and_then(|entry| {
            if entry.is_valid() {
                Some(entry.inventory.clone())
            } else {
                None
            }
        })
    }

    /// Store an inventory in the cache
    pub async fn set(&self, key: &str, inventory: Inventory, ttl: Option<Duration>) {
        let entry = CacheEntry {
            inventory: inventory.clone(),
            created_at: Instant::now(),
            ttl: ttl.unwrap_or(self.default_ttl),
        };

        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), entry);

        // Also persist to disk if configured
        if let Some(cache_dir) = &self.cache_dir {
            if let Err(e) = self.persist_entry(cache_dir, key, &inventory).await {
                tracing::warn!("Failed to persist cache entry: {}", e);
            }
        }
    }

    /// Invalidate a cache entry
    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(key);

        // Also remove from disk if configured
        if let Some(cache_dir) = &self.cache_dir {
            let cache_file = cache_dir.join(format!("{}.json", key));
            let _ = tokio::fs::remove_file(cache_file).await;
        }
    }

    /// Clear all cache entries
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();

        // Also clear disk cache if configured
        if let Some(cache_dir) = &self.cache_dir {
            if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
    }

    /// Get or compute an inventory
    pub async fn get_or_compute<F, Fut>(
        &self,
        key: &str,
        compute: F,
        ttl: Option<Duration>,
    ) -> InventoryResult<Inventory>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = InventoryResult<Inventory>>,
    {
        // Check cache first
        if let Some(inventory) = self.get(key).await {
            return Ok(inventory);
        }

        // Compute and cache
        let inventory = compute().await?;
        self.set(key, inventory.clone(), ttl).await;
        Ok(inventory)
    }

    /// Remove expired entries
    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| entry.is_valid());
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let total = cache.len();
        let valid = cache.values().filter(|e| e.is_valid()).count();
        let expired = total - valid;

        CacheStats {
            total_entries: total,
            valid_entries: valid,
            expired_entries: expired,
        }
    }

    async fn persist_entry(
        &self,
        cache_dir: &Path,
        key: &str,
        inventory: &Inventory,
    ) -> std::io::Result<()> {
        tokio::fs::create_dir_all(cache_dir).await?;

        let cache_file = cache_dir.join(format!("{}.json", key));
        let json = inventory_to_json(inventory)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        tokio::fs::write(cache_file, serde_json::to_string_pretty(&json)?).await
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of cache entries
    pub total_entries: usize,
    /// Number of valid (non-expired) entries
    pub valid_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
}

// ============================================================================
// Cached Plugin Wrapper
// ============================================================================

/// Wrapper that adds caching to any inventory plugin
pub struct CachedInventoryPlugin {
    inner: Arc<dyn InventoryPlugin>,
    cache: Arc<InventoryCache>,
    cache_key: String,
    ttl: Duration,
}

impl CachedInventoryPlugin {
    /// Create a new cached plugin wrapper
    pub fn new(
        plugin: Arc<dyn InventoryPlugin>,
        cache: Arc<InventoryCache>,
        cache_key: impl Into<String>,
        ttl: Duration,
    ) -> Self {
        Self {
            inner: plugin,
            cache,
            cache_key: cache_key.into(),
            ttl,
        }
    }

    /// Create with default TTL from cache
    pub fn with_default_ttl(
        plugin: Arc<dyn InventoryPlugin>,
        cache: Arc<InventoryCache>,
        cache_key: impl Into<String>,
    ) -> Self {
        let ttl = cache.default_ttl;
        Self::new(plugin, cache, cache_key, ttl)
    }
}

impl fmt::Debug for CachedInventoryPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CachedInventoryPlugin")
            .field("inner", &self.inner.name())
            .field("cache_key", &self.cache_key)
            .field("ttl", &self.ttl)
            .finish()
    }
}

#[async_trait]
impl InventoryPlugin for CachedInventoryPlugin {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn version(&self) -> &str {
        self.inner.version()
    }

    fn verify(&self) -> InventoryResult<()> {
        self.inner.verify()
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        let inner = Arc::clone(&self.inner);
        let ttl = self.ttl;

        self.cache
            .get_or_compute(
                &self.cache_key,
                || async move { inner.parse().await },
                Some(ttl),
            )
            .await
    }

    async fn refresh(&self) -> InventoryResult<()> {
        self.cache.invalidate(&self.cache_key).await;
        // Re-parse to refresh cache
        self.parse().await?;
        Ok(())
    }

    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        self.inner.supported_options()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert inventory to JSON format (compatible with Ansible dynamic inventory)
pub fn inventory_to_json(inventory: &Inventory) -> InventoryResult<serde_json::Value> {
    let mut result = serde_json::Map::new();
    let mut hostvars = serde_json::Map::new();

    // Add groups
    for group in inventory.groups() {
        if group.name == "all" {
            continue; // Handle 'all' specially
        }

        let mut group_data = serde_json::Map::new();

        // Add hosts
        let hosts: Vec<serde_json::Value> = group
            .hosts
            .iter()
            .map(|h| serde_json::Value::String(h.clone()))
            .collect();

        if !hosts.is_empty() {
            group_data.insert("hosts".to_string(), serde_json::Value::Array(hosts));
        }

        // Add children
        let children: Vec<serde_json::Value> = group
            .children
            .iter()
            .map(|c| serde_json::Value::String(c.clone()))
            .collect();

        if !children.is_empty() {
            group_data.insert("children".to_string(), serde_json::Value::Array(children));
        }

        // Add group vars
        if !group.vars.is_empty() {
            let vars: serde_json::Map<String, serde_json::Value> = group
                .vars
                .iter()
                .map(|(k, v)| (k.clone(), yaml_to_json(v)))
                .collect();
            group_data.insert("vars".to_string(), serde_json::Value::Object(vars));
        }

        result.insert(group.name.clone(), serde_json::Value::Object(group_data));
    }

    // Add host variables to _meta
    for host in inventory.hosts() {
        if !host.vars.is_empty() {
            let vars: serde_json::Map<String, serde_json::Value> = host
                .vars
                .iter()
                .map(|(k, v)| (k.clone(), yaml_to_json(v)))
                .collect();
            hostvars.insert(host.name.clone(), serde_json::Value::Object(vars));
        }
    }

    // Add _meta section
    let mut meta = serde_json::Map::new();
    meta.insert("hostvars".to_string(), serde_json::Value::Object(hostvars));
    result.insert("_meta".to_string(), serde_json::Value::Object(meta));

    Ok(serde_json::Value::Object(result))
}

/// Convert YAML value to JSON value
fn yaml_to_json(value: &serde_yaml::Value) -> serde_json::Value {
    match value {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter_map(|(k, v)| {
                    if let serde_yaml::Value::String(key) = k {
                        Some((key.clone(), yaml_to_json(v)))
                    } else {
                        None
                    }
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

/// Parse JSON data into inventory
pub fn parse_json_inventory(
    inventory: &mut Inventory,
    data: &serde_json::Value,
) -> InventoryResult<()> {
    if let serde_json::Value::Object(map) = data {
        for (key, value) in map {
            if key == "_meta" {
                // Handle host variables from _meta
                if let Some(hostvars) = value.get("hostvars") {
                    if let serde_json::Value::Object(vars_map) = hostvars {
                        for (host_name, vars) in vars_map {
                            if let Some(host) = inventory.get_host_mut(host_name) {
                                if let serde_json::Value::Object(var_obj) = vars {
                                    for (k, v) in var_obj {
                                        host.set_var(k, json_to_yaml(v));
                                    }
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Parse group
            let mut group = super::Group::new(key);

            if let serde_json::Value::Object(group_data) = value {
                // Parse hosts
                if let Some(serde_json::Value::Array(hosts)) = group_data.get("hosts") {
                    for host_value in hosts {
                        if let serde_json::Value::String(host_name) = host_value {
                            group.add_host(host_name.clone());

                            // Add host if it doesn't exist
                            if inventory.get_host(host_name).is_none() {
                                let mut host = super::Host::new(host_name.clone());
                                host.add_to_group(key.clone());
                                host.add_to_group("all".to_string());
                                inventory.add_host(host)?;
                            }
                        }
                    }
                }

                // Parse children
                if let Some(serde_json::Value::Array(children)) = group_data.get("children") {
                    for child_value in children {
                        if let serde_json::Value::String(child_name) = child_value {
                            group.add_child(child_name.clone());
                        }
                    }
                }

                // Parse vars
                if let Some(serde_json::Value::Object(vars)) = group_data.get("vars") {
                    for (var_key, var_value) in vars {
                        group.set_var(var_key, json_to_yaml(var_value));
                    }
                }
            } else if let serde_json::Value::Array(hosts) = value {
                // Simple list of hosts
                for host_value in hosts {
                    if let serde_json::Value::String(host_name) = host_value {
                        group.add_host(host_name.clone());

                        if inventory.get_host(host_name).is_none() {
                            let mut host = super::Host::new(host_name.clone());
                            host.add_to_group(key.clone());
                            host.add_to_group("all".to_string());
                            inventory.add_host(host)?;
                        }
                    }
                }
            }

            inventory.add_group(group)?;
        }
    }

    Ok(())
}

/// Convert JSON value to YAML value
fn json_to_yaml(value: &serde_json::Value) -> serde_yaml::Value {
    match value {
        serde_json::Value::Null => serde_yaml::Value::Null,
        serde_json::Value::Bool(b) => serde_yaml::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_yaml::Value::Number(serde_yaml::Number::from(f as i64))
            } else {
                serde_yaml::Value::Null
            }
        }
        serde_json::Value::String(s) => serde_yaml::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            serde_yaml::Value::Sequence(arr.iter().map(json_to_yaml).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in obj {
                map.insert(serde_yaml::Value::String(k.clone()), json_to_yaml(v));
            }
            serde_yaml::Value::Mapping(map)
        }
    }
}

/// Alias for parse_json_inventory for backward compatibility
pub fn parse_json_inventory_from_value(
    inventory: &mut Inventory,
    data: &serde_json::Value,
) -> InventoryResult<()> {
    parse_json_inventory(inventory, data)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_config_builder() {
        let config = InventoryPluginConfig::new()
            .with_option("region", "us-west-2")
            .with_cache_ttl(Duration::from_secs(600))
            .with_keyed_group(KeyedGroup::new("tags.environment").with_prefix("env"));

        assert_eq!(config.get_string("region"), Some("us-west-2".to_string()));
        assert_eq!(config.cache_ttl_secs, 600);
        assert!(config.cache_enabled);
        assert_eq!(config.keyed_groups.len(), 1);
    }

    #[test]
    fn test_keyed_group_builder() {
        let kg = KeyedGroup::new("instance_type")
            .with_prefix("type")
            .with_separator("_")
            .with_parent("aws_ec2");

        assert_eq!(kg.key, "instance_type");
        assert_eq!(kg.prefix, "type");
        assert_eq!(kg.separator, "_");
        assert_eq!(kg.parent_group, Some("aws_ec2".to_string()));
    }

    #[test]
    fn test_plugin_factory_available_plugins() {
        let plugins = InventoryPluginFactory::available_plugins();
        assert!(!plugins.is_empty());

        let names = InventoryPluginFactory::available_plugin_names();
        assert!(names.contains(&"file"));
        assert!(names.contains(&"script"));
        assert!(names.contains(&"aws_ec2"));
    }

    #[test]
    fn test_plugin_factory_plugin_exists() {
        assert!(InventoryPluginFactory::plugin_exists("file"));
        assert!(InventoryPluginFactory::plugin_exists("aws_ec2"));
        assert!(!InventoryPluginFactory::plugin_exists("nonexistent"));
    }

    #[test]
    fn test_plugin_registry() {
        let mut registry = InventoryPluginRegistry::new();

        registry.register("custom", |config| {
            Ok(
                Arc::new(FileInventoryPlugin::new(PathBuf::from("/tmp/test"), config))
                    as Arc<dyn InventoryPlugin>,
            )
        });

        assert!(registry.is_registered("custom"));
        assert!(registry.is_registered("CUSTOM")); // Case insensitive
        assert!(!registry.is_registered("unknown"));
    }

    #[tokio::test]
    async fn test_inventory_cache() {
        let cache = InventoryCache::new(Duration::from_secs(60));

        let inventory = Inventory::new();
        cache.set("test", inventory, None).await;

        let cached = cache.get("test").await;
        assert!(cached.is_some());

        cache.invalidate("test").await;
        let cached = cache.get("test").await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = InventoryCache::new(Duration::from_secs(60));

        cache.set("test1", Inventory::new(), None).await;
        cache.set("test2", Inventory::new(), None).await;

        let stats = cache.stats().await;
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.valid_entries, 2);
        assert_eq!(stats.expired_entries, 0);
    }

    #[test]
    fn test_inventory_to_json() {
        let mut inventory = Inventory::new();

        let mut group = super::super::Group::new("webservers");
        group.add_host("web1");
        group.set_var("http_port", serde_yaml::Value::Number(80.into()));
        inventory.add_group(group).unwrap();

        let json = inventory_to_json(&inventory).unwrap();
        assert!(json.is_object());
        assert!(json.get("webservers").is_some());
        assert!(json.get("_meta").is_some());
    }

    #[test]
    fn test_aws_ec2_plugin_options() {
        let plugin = AwsEc2InventoryPlugin::new(InventoryPluginConfig::new());
        let options = plugin.supported_options();

        assert!(!options.is_empty());
        assert!(options.iter().any(|o| o.name == "region"));
        assert!(options.iter().any(|o| o.name == "hostnames"));
    }
}
