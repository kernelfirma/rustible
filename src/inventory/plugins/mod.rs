//! Inventory Plugins for Rustible
//!
//! This module provides dynamic inventory plugins for various cloud providers
//! and container platforms. Each plugin implements the `InventoryPlugin` trait
//! and can be used to dynamically discover and manage hosts.
//!
//! # Available Plugins
//!
//! - [`aws_ec2`]: AWS EC2 instances inventory
//! - [`azure`]: Azure Virtual Machines inventory
//! - [`gcp`]: Google Cloud Platform Compute Engine inventory
//! - [`terraform`]: Terraform state file inventory
//! - [`docker`]: Docker containers inventory
//!
//! # Configuration
//!
//! All plugins use YAML configuration files with a consistent structure:
//!
//! ```yaml
//! plugin: aws_ec2
//! regions:
//!   - us-east-1
//!   - us-west-2
//! filters:
//!   tag:Environment: production
//! keyed_groups:
//!   - key: tags.Role
//!     prefix: role
//!   - key: instance_type
//!     prefix: type
//! hostnames:
//!   - tag:Name
//!   - private-dns-name
//! compose:
//!   ansible_host: private_ip_address
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use rustible::inventory::plugins::{AwsEc2Plugin, PluginConfig, DynamicInventoryPlugin};
//! use std::path::Path;
//!
//! let config = PluginConfig::from_file(Path::new("aws_ec2.yml"))?;
//! let plugin = AwsEc2Plugin::new(config)?;
//! let inventory = plugin.parse().await?;
//! # Ok(())
//! # }
//! ```

pub mod aws_ec2;
pub mod azure;
pub mod config;
pub mod gcp;
pub mod terraform;

#[cfg(feature = "slurm")]
pub mod slurm;
#[cfg(feature = "openstack")]
pub mod openstack;

pub use aws_ec2::AwsEc2Plugin;
pub use azure::AzurePlugin;
pub use config::{
    sanitize_group_name, ComposeConfig, FilterConfig, FilterOperator, HostnameConfig,
    KeyedGroupConfig, PluginConfig, PluginConfigBuilder, PluginConfigError, PluginConfigResult,
};
pub use gcp::GcpPlugin;
pub use terraform::{
    CacheConfig, GroupByRule, LocalBackend, ResourceMapping, TerraformBackendType,
    TerraformInventoryPlugin, TerraformPlugin, TerraformPluginConfig, TerraformStateBackend,
};

#[cfg(feature = "slurm")]
pub use slurm::SlurmPlugin;
#[cfg(feature = "openstack")]
pub use openstack::OpenstackPlugin;

use super::{Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

/// Common trait for all dynamic inventory plugins
#[async_trait]
pub trait DynamicInventoryPlugin: Send + Sync + fmt::Debug {
    /// Get the plugin name
    fn name(&self) -> &str;

    /// Get the plugin version
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Get the plugin description
    fn description(&self) -> &str;

    /// Verify plugin configuration
    fn verify(&self) -> InventoryResult<()>;

    /// Parse and return the inventory
    async fn parse(&self) -> InventoryResult<Inventory>;

    /// Refresh cached data (if any)
    async fn refresh(&self) -> InventoryResult<()> {
        Ok(())
    }

    /// Get plugin-specific options documentation
    fn options_documentation(&self) -> Vec<PluginOption>;
}

/// Documentation for a plugin option
#[derive(Debug, Clone)]
pub struct PluginOption {
    /// Option name
    pub name: String,
    /// Option description
    pub description: String,
    /// Whether the option is required
    pub required: bool,
    /// Default value (if any)
    pub default: Option<String>,
    /// Option type (string, bool, list, dict)
    pub option_type: PluginOptionType,
    /// Environment variable alternative
    pub env_var: Option<String>,
}

impl PluginOption {
    /// Create a new required string option
    pub fn required_string(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: true,
            default: None,
            option_type: PluginOptionType::String,
            env_var: None,
        }
    }

    /// Create a new optional string option with default
    pub fn optional_string(name: &str, description: &str, default: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: false,
            default: Some(default.to_string()),
            option_type: PluginOptionType::String,
            env_var: None,
        }
    }

    /// Create a new optional boolean option
    pub fn optional_bool(name: &str, description: &str, default: bool) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: false,
            default: Some(default.to_string()),
            option_type: PluginOptionType::Bool,
            env_var: None,
        }
    }

    /// Create a new optional list option
    pub fn optional_list(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required: false,
            default: None,
            option_type: PluginOptionType::List,
            env_var: None,
        }
    }

    /// Set environment variable alternative
    pub fn with_env_var(mut self, env_var: &str) -> Self {
        self.env_var = Some(env_var.to_string());
        self
    }
}

/// Type of plugin option
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOptionType {
    /// String value
    String,
    /// Boolean value
    Bool,
    /// Integer value
    Int,
    /// List of values
    List,
    /// Dictionary/map of values
    Dict,
}

impl fmt::Display for PluginOptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginOptionType::String => write!(f, "string"),
            PluginOptionType::Bool => write!(f, "bool"),
            PluginOptionType::Int => write!(f, "int"),
            PluginOptionType::List => write!(f, "list"),
            PluginOptionType::Dict => write!(f, "dict"),
        }
    }
}

/// Registry for dynamic inventory plugins
pub struct DynamicPluginRegistry {
    plugins: std::collections::HashMap<String, Arc<dyn DynamicInventoryPlugin>>,
}

impl Default for DynamicPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicPluginRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            plugins: std::collections::HashMap::new(),
        }
    }

    /// Register a plugin
    pub fn register(&mut self, name: &str, plugin: Arc<dyn DynamicInventoryPlugin>) {
        self.plugins.insert(name.to_lowercase(), plugin);
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn DynamicInventoryPlugin>> {
        self.plugins.get(&name.to_lowercase()).cloned()
    }

    /// Check if a plugin is registered
    pub fn has(&self, name: &str) -> bool {
        self.plugins.contains_key(&name.to_lowercase())
    }

    /// List all registered plugin names
    pub fn list(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Create a registry with all built-in plugins using default configurations
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register AWS EC2 plugin
        if let Ok(plugin) = AwsEc2Plugin::with_defaults() {
            registry.register("aws_ec2", Arc::new(plugin));
        }

        // Register Azure plugin
        if let Ok(plugin) = AzurePlugin::with_defaults() {
            registry.register("azure", Arc::new(plugin));
        }

        // Register GCP plugin
        if let Ok(plugin) = GcpPlugin::with_defaults() {
            registry.register("gcp", Arc::new(plugin));
        }

        // Register Terraform plugin
        if let Ok(plugin) = TerraformPlugin::with_defaults() {
            registry.register("terraform", Arc::new(plugin));
        }

        // Register Slurm plugin
        #[cfg(feature = "slurm")]
        {
            if let Ok(plugin) = SlurmPlugin::with_defaults() {
                registry.register("slurm", Arc::new(plugin));
            }
        }

        // Register OpenStack plugin
        #[cfg(feature = "openstack")]
        {
            if let Ok(plugin) = OpenstackPlugin::with_defaults() {
                registry.register("openstack", Arc::new(plugin));
            }
        }

        registry
    }
}

impl fmt::Debug for DynamicPluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicPluginRegistry")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Create a plugin from a configuration file
pub async fn create_plugin_from_file(
    path: &std::path::Path,
) -> InventoryResult<Arc<dyn DynamicInventoryPlugin>> {
    let config = PluginConfig::from_file(path).map_err(|e| {
        InventoryError::DynamicInventoryFailed(format!(
            "Failed to load plugin config from '{}': {}",
            path.display(),
            e
        ))
    })?;

    create_plugin_from_config(config)
}

/// Create a plugin from a configuration
pub fn create_plugin_from_config(
    config: PluginConfig,
) -> InventoryResult<Arc<dyn DynamicInventoryPlugin>> {
    let plugin_name = config.plugin.to_lowercase();

    match plugin_name.as_str() {
        "aws_ec2" | "amazon.aws.aws_ec2" => {
            let plugin = AwsEc2Plugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create AWS EC2 plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        "azure_rm" | "azure.azcollection.azure_rm" | "azure" => {
            let plugin = AzurePlugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create Azure plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        "gcp_compute" | "google.cloud.gcp_compute" | "gcp" => {
            let plugin = GcpPlugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create GCP plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        "terraform" | "cloud.terraform.terraform_state" => {
            let plugin = TerraformPlugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create Terraform plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        #[cfg(feature = "slurm")]
        "slurm" | "hpc.slurm" => {
            let plugin = SlurmPlugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create Slurm plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        #[cfg(feature = "openstack")]
        "openstack" | "openstack.cloud.openstack" => {
            let plugin = OpenstackPlugin::new(config).map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to create OpenStack plugin: {}",
                    e
                ))
            })?;
            Ok(Arc::new(plugin))
        }
        _ => Err(InventoryError::DynamicInventoryFailed(format!(
            "Unknown plugin: '{}'. Available plugins: aws_ec2, azure, gcp, terraform, slurm, openstack",
            plugin_name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_option_builders() {
        let opt = PluginOption::required_string("region", "AWS region");
        assert!(opt.required);
        assert_eq!(opt.option_type, PluginOptionType::String);

        let opt =
            PluginOption::optional_bool("include_stopped", "Include stopped instances", false);
        assert!(!opt.required);
        assert_eq!(opt.default, Some("false".to_string()));

        let opt =
            PluginOption::optional_list("regions", "List of regions").with_env_var("AWS_REGIONS");
        assert_eq!(opt.env_var, Some("AWS_REGIONS".to_string()));
    }

    #[test]
    fn test_plugin_registry() {
        let registry = DynamicPluginRegistry::new();
        assert!(registry.list().is_empty());
    }
}
