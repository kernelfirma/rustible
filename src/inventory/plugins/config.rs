//! Plugin Configuration for Dynamic Inventory
//!
//! This module provides a unified configuration structure for all inventory plugins.
//! Configuration can be loaded from YAML files or built programmatically.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur when loading plugin configuration
#[derive(Debug, Error)]
pub enum PluginConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Result type for plugin configuration operations
pub type PluginConfigResult<T> = Result<T, PluginConfigError>;

/// Main plugin configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
    /// Plugin name (e.g., "aws_ec2", "azure_rm", "gcp_compute", "docker")
    pub plugin: String,

    /// Regions/locations to query (for cloud plugins)
    #[serde(default)]
    pub regions: Vec<String>,

    /// Filters to apply when querying resources
    #[serde(default)]
    pub filters: HashMap<String, FilterConfig>,

    /// Keyed groups for dynamic group creation
    #[serde(default)]
    pub keyed_groups: Vec<KeyedGroupConfig>,

    /// Hostname preferences (in order of preference)
    #[serde(default)]
    pub hostnames: Vec<HostnameConfig>,

    /// Compose configuration for setting ansible_host and other vars
    #[serde(default)]
    pub compose: ComposeConfig,

    /// Whether to use strict mode (fail on template errors)
    #[serde(default)]
    pub strict: bool,

    /// Cache TTL in seconds (0 = no caching)
    #[serde(default)]
    pub cache_ttl: u64,

    /// Additional plugin-specific options
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

impl PluginConfig {
    /// Create a new configuration for a specific plugin
    pub fn new(plugin: impl Into<String>) -> Self {
        Self {
            plugin: plugin.into(),
            ..Default::default()
        }
    }

    /// Load configuration from a YAML file
    pub fn from_file(path: &Path) -> PluginConfigResult<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Parse configuration from YAML string
    pub fn from_yaml(yaml: &str) -> PluginConfigResult<Self> {
        let config: Self = serde_yaml::from_str(yaml)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> PluginConfigResult<()> {
        if self.plugin.is_empty() {
            return Err(PluginConfigError::MissingField("plugin".to_string()));
        }
        Ok(())
    }

    /// Get a string option from extras
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.extra
            .get(key)
            .and_then(|v| v.as_str().map(String::from))
    }

    /// Get a boolean option from extras
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.extra.get(key).and_then(|v| v.as_bool())
    }

    /// Get an integer option from extras
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.extra.get(key).and_then(|v| v.as_i64())
    }

    /// Get a list of strings from extras
    pub fn get_string_list(&self, key: &str) -> Option<Vec<String>> {
        self.extra.get(key).and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
        })
    }

    /// Create a builder for this config
    pub fn builder(plugin: impl Into<String>) -> PluginConfigBuilder {
        PluginConfigBuilder::new(plugin)
    }
}

/// Filter configuration for resource selection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterConfig {
    /// Single value filter
    Single(String),
    /// Multiple value filter (OR)
    Multiple(Vec<String>),
    /// Complex filter with operator
    Complex {
        operator: FilterOperator,
        values: Vec<String>,
    },
}

impl FilterConfig {
    /// Get filter values as a vector
    pub fn values(&self) -> Vec<&str> {
        match self {
            FilterConfig::Single(v) => vec![v.as_str()],
            FilterConfig::Multiple(vs) => vs.iter().map(|s| s.as_str()).collect(),
            FilterConfig::Complex { values, .. } => values.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Filter operators for complex filters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FilterOperator {
    /// Equals
    Eq,
    /// Not equals
    Ne,
    /// Contains
    Contains,
    /// Starts with
    StartsWith,
    /// Ends with
    EndsWith,
    /// Regex match
    Regex,
}

/// Keyed group configuration for dynamic group creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyedGroupConfig {
    /// Key expression (can reference resource attributes)
    pub key: String,

    /// Prefix for the generated group name
    #[serde(default)]
    pub prefix: String,

    /// Separator between prefix and key value
    #[serde(default = "default_separator")]
    pub separator: String,

    /// Parent group for all generated groups
    #[serde(default)]
    pub parent_group: Option<String>,

    /// Default value if key is not found
    #[serde(default)]
    pub default_value: Option<String>,

    /// Whether to create empty groups
    #[serde(default)]
    pub trailing_separator: bool,
}

fn default_separator() -> String {
    "_".to_string()
}

impl KeyedGroupConfig {
    /// Create a new keyed group configuration
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

    /// Generate a group name from a value
    pub fn generate_group_name(&self, value: &str) -> String {
        let sanitized = sanitize_group_name(value);
        if self.prefix.is_empty() {
            sanitized
        } else {
            format!("{}{}{}", self.prefix, self.separator, sanitized)
        }
    }
}

/// Hostname configuration for determining inventory hostname
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostnameConfig {
    /// Simple attribute reference
    Simple(String),
    /// Complex hostname with conditions
    Complex {
        name: String,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        separator: String,
    },
}

impl HostnameConfig {
    /// Get the attribute name
    pub fn name(&self) -> &str {
        match self {
            HostnameConfig::Simple(s) => s,
            HostnameConfig::Complex { name, .. } => name,
        }
    }
}

/// Compose configuration for setting host variables
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeConfig {
    /// ansible_host expression
    #[serde(default)]
    pub ansible_host: Option<String>,

    /// ansible_port expression
    #[serde(default)]
    pub ansible_port: Option<String>,

    /// ansible_user expression
    #[serde(default)]
    pub ansible_user: Option<String>,

    /// ansible_connection expression
    #[serde(default)]
    pub ansible_connection: Option<String>,

    /// Additional variables to set
    #[serde(default, flatten)]
    pub extra_vars: HashMap<String, String>,
}

impl ComposeConfig {
    /// Check if any compose configuration is set
    pub fn is_empty(&self) -> bool {
        self.ansible_host.is_none()
            && self.ansible_port.is_none()
            && self.ansible_user.is_none()
            && self.ansible_connection.is_none()
            && self.extra_vars.is_empty()
    }
}

/// Builder for plugin configuration
#[derive(Debug, Clone, Default)]
pub struct PluginConfigBuilder {
    config: PluginConfig,
}

impl PluginConfigBuilder {
    /// Create a new builder for a specific plugin
    pub fn new(plugin: impl Into<String>) -> Self {
        Self {
            config: PluginConfig::new(plugin),
        }
    }

    /// Add a region
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.config.regions.push(region.into());
        self
    }

    /// Add multiple regions
    pub fn regions(mut self, regions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.config
            .regions
            .extend(regions.into_iter().map(Into::into));
        self
    }

    /// Add a filter
    pub fn filter(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config
            .filters
            .insert(key.into(), FilterConfig::Single(value.into()));
        self
    }

    /// Add a multi-value filter
    pub fn filter_multi(
        mut self,
        key: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.config.filters.insert(
            key.into(),
            FilterConfig::Multiple(values.into_iter().map(Into::into).collect()),
        );
        self
    }

    /// Add a keyed group
    pub fn keyed_group(mut self, group: KeyedGroupConfig) -> Self {
        self.config.keyed_groups.push(group);
        self
    }

    /// Add a hostname preference
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.config
            .hostnames
            .push(HostnameConfig::Simple(hostname.into()));
        self
    }

    /// Set ansible_host compose expression
    pub fn compose_ansible_host(mut self, expr: impl Into<String>) -> Self {
        self.config.compose.ansible_host = Some(expr.into());
        self
    }

    /// Set ansible_port compose expression
    pub fn compose_ansible_port(mut self, expr: impl Into<String>) -> Self {
        self.config.compose.ansible_port = Some(expr.into());
        self
    }

    /// Set ansible_user compose expression
    pub fn compose_ansible_user(mut self, expr: impl Into<String>) -> Self {
        self.config.compose.ansible_user = Some(expr.into());
        self
    }

    /// Set strict mode
    pub fn strict(mut self, strict: bool) -> Self {
        self.config.strict = strict;
        self
    }

    /// Set cache TTL
    pub fn cache_ttl(mut self, ttl: u64) -> Self {
        self.config.cache_ttl = ttl;
        self
    }

    /// Add extra option
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<serde_yaml::Value>) -> Self {
        self.config.extra.insert(key.into(), value.into());
        self
    }

    /// Build the configuration
    pub fn build(self) -> PluginConfigResult<PluginConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

/// Sanitize a string to be a valid group name
pub fn sanitize_group_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut prev_underscore = false;

    for ch in name.chars() {
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            prev_underscore = false;
        } else if !prev_underscore {
            result.push('_');
            prev_underscore = true;
        }
    }

    // Trim leading/trailing underscores
    result.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_config_new() {
        let config = PluginConfig::new("aws_ec2");
        assert_eq!(config.plugin, "aws_ec2");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_plugin_config_builder() {
        let config = PluginConfigBuilder::new("aws_ec2")
            .regions(["us-east-1", "us-west-2"])
            .filter("tag:Environment", "production")
            .keyed_group(KeyedGroupConfig::new("instance_type").with_prefix("type"))
            .hostname("tag:Name")
            .compose_ansible_host("private_ip_address")
            .cache_ttl(300)
            .build()
            .unwrap();

        assert_eq!(config.plugin, "aws_ec2");
        assert_eq!(config.regions.len(), 2);
        assert!(config.filters.contains_key("tag:Environment"));
        assert_eq!(config.keyed_groups.len(), 1);
        assert_eq!(config.cache_ttl, 300);
    }

    #[test]
    fn test_keyed_group_generate_name() {
        let kg = KeyedGroupConfig::new("instance_type")
            .with_prefix("type")
            .with_separator("_");

        assert_eq!(kg.generate_group_name("t2.micro"), "type_t2_micro");
        assert_eq!(kg.generate_group_name("m5.large"), "type_m5_large");
    }

    #[test]
    fn test_sanitize_group_name() {
        assert_eq!(sanitize_group_name("my-group"), "my_group");
        assert_eq!(sanitize_group_name("My Group Name"), "my_group_name");
        assert_eq!(sanitize_group_name("test--value"), "test_value");
        assert_eq!(sanitize_group_name("  spaces  "), "spaces");
    }

    #[test]
    fn test_filter_config_values() {
        let single = FilterConfig::Single("value".to_string());
        assert_eq!(single.values(), vec!["value"]);

        let multi = FilterConfig::Multiple(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(multi.values(), vec!["a", "b"]);
    }

    #[test]
    fn test_plugin_config_from_yaml() {
        let yaml = r#"
plugin: aws_ec2
regions:
  - us-east-1
  - us-west-2
filters:
  tag:Environment: production
keyed_groups:
  - key: instance_type
    prefix: type
hostnames:
  - tag:Name
  - private-dns-name
compose:
  ansible_host: private_ip_address
cache_ttl: 300
        "#;

        let config = PluginConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.plugin, "aws_ec2");
        assert_eq!(config.regions, vec!["us-east-1", "us-west-2"]);
        assert_eq!(config.keyed_groups.len(), 1);
        assert_eq!(config.cache_ttl, 300);
    }
}
