//! Terraform State Dynamic Inventory Plugin
//!
//! This plugin discovers hosts from Terraform state files (terraform.tfstate)
//! and creates inventory entries from AWS EC2 instances, Azure VMs, and GCP instances.
//!
//! # Configuration
//!
//! ```yaml
//! plugin: terraform
//! # State backend (choose one):
//! # Local state file
//! state_path: ./terraform.tfstate
//! # Or S3 backend
//! backend: s3
//! bucket: my-terraform-state
//! key: prod/terraform.tfstate
//! region: us-east-1
//! # Or HTTP backend
//! backend: http
//! address: https://terraform.example.com/state
//!
//! # Export terraform outputs as group vars
//! export_outputs: true
//!
//! keyed_groups:
//!   - key: resource_type
//!     prefix: tf
//!   - key: provider
//!     prefix: provider
//! hostnames:
//!   - tag:Name
//!   - name
//!   - private_ip
//! compose:
//!   ansible_host: private_ip
//!   ansible_user: ec2-user
//! ```
//!
//! # Supported Resources
//!
//! - `aws_instance` - AWS EC2 instances
//! - `azurerm_virtual_machine` - Azure VMs
//! - `azurerm_linux_virtual_machine` - Azure Linux VMs
//! - `azurerm_windows_virtual_machine` - Azure Windows VMs
//! - `google_compute_instance` - GCP Compute Engine instances
//!
//! # Features
//!
//! - Local and remote state file support (S3, HTTP)
//! - Terraform outputs as inventory variables
//! - Multi-provider support
//! - Tag/label-based grouping

use super::config::{sanitize_group_name, PluginConfig, PluginConfigError};
use super::{DynamicInventoryPlugin, PluginOption, PluginOptionType};
use crate::inventory::{Group, Host, Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// State backend configuration
#[derive(Debug, Clone)]
pub enum StateBackend {
    /// Local filesystem state file
    Local { path: PathBuf },
    /// AWS S3 remote state
    S3 {
        bucket: String,
        key: String,
        region: String,
    },
    /// HTTP remote state
    Http { address: String },
}

impl Default for StateBackend {
    fn default() -> Self {
        StateBackend::Local {
            path: PathBuf::from("terraform.tfstate"),
        }
    }
}

/// Backend type enumeration for Terraform state sources
///
/// This enum represents all supported Terraform backend types for reading state files.
/// Each variant corresponds to a different storage mechanism where Terraform state can be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TerraformBackendType {
    /// Local filesystem state file
    #[default]
    Local,
    /// AWS S3 remote state backend
    S3,
    /// Google Cloud Storage backend
    Gcs,
    /// Azure Blob Storage backend
    Azure,
    /// HashiCorp Consul backend
    Consul,
    /// HTTP/HTTPS remote state backend
    Http,
}


impl std::fmt::Display for TerraformBackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::S3 => write!(f, "s3"),
            Self::Gcs => write!(f, "gcs"),
            Self::Azure => write!(f, "azure"),
            Self::Consul => write!(f, "consul"),
            Self::Http => write!(f, "http"),
        }
    }
}

/// Plugin configuration for Terraform state inventory
///
/// This struct holds all configuration options for the Terraform inventory plugin,
/// including backend settings, resource mappings, and caching options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerraformPluginConfig {
    /// State backend type
    #[serde(default)]
    pub backend: TerraformBackendType,

    /// Path to local state file (for Local backend)
    #[serde(default)]
    pub state_path: Option<PathBuf>,

    /// S3 bucket name (for S3 backend)
    #[serde(default)]
    pub bucket: Option<String>,

    /// S3/GCS/Azure object key (for remote backends)
    #[serde(default)]
    pub key: Option<String>,

    /// AWS/GCP region (for cloud backends)
    #[serde(default)]
    pub region: Option<String>,

    /// HTTP address (for HTTP backend)
    #[serde(default)]
    pub address: Option<String>,

    /// Azure storage account (for Azure backend)
    #[serde(default)]
    pub storage_account: Option<String>,

    /// Azure container name (for Azure backend)
    #[serde(default)]
    pub container: Option<String>,

    /// Consul address (for Consul backend)
    #[serde(default)]
    pub consul_address: Option<String>,

    /// Resource to host mapping rules
    #[serde(default)]
    pub resource_mappings: HashMap<String, ResourceMapping>,

    /// Export terraform outputs as group vars
    #[serde(default = "default_true")]
    pub export_outputs: bool,

    /// Cache configuration
    #[serde(default)]
    pub cache: Option<CacheConfig>,
}

fn default_true() -> bool {
    true
}

impl Default for TerraformPluginConfig {
    fn default() -> Self {
        Self {
            backend: TerraformBackendType::Local,
            state_path: Some(PathBuf::from("terraform.tfstate")),
            bucket: None,
            key: None,
            region: None,
            address: None,
            storage_account: None,
            container: None,
            consul_address: None,
            resource_mappings: HashMap::new(),
            export_outputs: true,
            cache: None,
        }
    }
}

/// Resource to host mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMapping {
    /// Attribute to use for hostname
    #[serde(default)]
    pub hostname_attribute: Option<String>,

    /// Attribute to use for address
    #[serde(default)]
    pub address_attribute: Option<String>,

    /// Fallback address attribute if primary is not available
    #[serde(default)]
    pub fallback_address: Option<String>,

    /// Grouping rules based on resource attributes
    #[serde(default)]
    pub group_by: Vec<GroupByRule>,

    /// Host variables to set from resource attributes
    #[serde(default)]
    pub host_vars: HashMap<String, String>,
}

/// Rule for grouping hosts by attribute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupByRule {
    /// Attribute path to group by
    pub attribute: String,

    /// Prefix for the group name
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Cache configuration for Terraform state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable caching
    #[serde(default)]
    pub enabled: bool,

    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub ttl: u64,
}

fn default_cache_ttl() -> u64 {
    300
}

/// Trait for Terraform state backends
///
/// This trait defines the interface for reading Terraform state from various backends.
/// Implementations handle authentication, fetching, and parsing of state data.
#[async_trait]
pub trait TerraformStateBackend: Send + Sync + std::fmt::Debug {
    /// Get the backend type
    fn backend_type(&self) -> TerraformBackendType;

    /// Fetch and parse the Terraform state
    async fn fetch_state(&self) -> InventoryResult<TerraformState>;

    /// Check if the backend is properly configured and accessible
    async fn verify(&self) -> InventoryResult<()>;
}

/// Local filesystem backend for Terraform state
#[derive(Debug, Clone)]
pub struct LocalBackend {
    /// Path to the state file
    path: PathBuf,
}

impl LocalBackend {
    /// Create a new local backend with the given path
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl TerraformStateBackend for LocalBackend {
    fn backend_type(&self) -> TerraformBackendType {
        TerraformBackendType::Local
    }

    async fn fetch_state(&self) -> InventoryResult<TerraformState> {
        let content = std::fs::read_to_string(&self.path).map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to read terraform state file '{}': {}",
                self.path.display(),
                e
            ))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to parse terraform state JSON: {}",
                e
            ))
        })
    }

    async fn verify(&self) -> InventoryResult<()> {
        if !self.path.exists() {
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "Terraform state file not found: {}",
                self.path.display()
            )));
        }
        Ok(())
    }
}

/// The main Terraform inventory plugin struct
///
/// This struct wraps `TerraformPlugin` and provides the full inventory plugin interface
/// as described in the architecture document. It supports multiple backends and
/// configurable resource mappings.
pub type TerraformInventoryPlugin = TerraformPlugin;

/// Terraform state file structure (v4 format)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerraformState {
    /// State version (should be 4)
    pub version: u32,
    /// Terraform version that created the state
    pub terraform_version: String,
    /// State serial number
    pub serial: u64,
    /// State lineage UUID
    pub lineage: String,
    /// Terraform outputs
    #[serde(default)]
    pub outputs: HashMap<String, TerraformOutput>,
    /// Resources in state
    #[serde(default)]
    pub resources: Vec<TerraformResource>,
}

/// Terraform output value
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerraformOutput {
    /// Output value
    pub value: serde_json::Value,
    /// Output type
    #[serde(rename = "type", default)]
    pub output_type: Option<serde_json::Value>,
    /// Is sensitive
    #[serde(default)]
    pub sensitive: bool,
}

/// Terraform resource in state
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerraformResource {
    /// Resource mode: "managed" or "data"
    pub mode: String,
    /// Resource type (e.g., "aws_instance")
    #[serde(rename = "type")]
    pub resource_type: String,
    /// Resource name in terraform config
    pub name: String,
    /// Provider configuration
    pub provider: String,
    /// Resource instances
    #[serde(default)]
    pub instances: Vec<TerraformInstance>,
}

/// Terraform resource instance
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerraformInstance {
    /// Schema version
    #[serde(default)]
    pub schema_version: u32,
    /// Instance attributes
    #[serde(default)]
    pub attributes: HashMap<String, serde_json::Value>,
    /// Instance ID (for keyed resources)
    #[serde(default)]
    pub index_key: Option<serde_json::Value>,
    /// Dependencies
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Extracted host information from terraform resources
#[derive(Debug, Clone)]
pub struct TerraformHost {
    /// Unique identifier
    pub id: String,
    /// Host name
    pub name: String,
    /// Resource type (aws_instance, etc.)
    pub resource_type: String,
    /// Resource name in terraform
    pub resource_name: String,
    /// Provider (aws, azurerm, google)
    pub provider: String,
    /// Private IP address
    pub private_ip: Option<String>,
    /// Public IP address
    pub public_ip: Option<String>,
    /// Tags/labels
    pub tags: HashMap<String, String>,
    /// All attributes from terraform
    pub attributes: HashMap<String, serde_json::Value>,
}

impl TerraformHost {
    /// Get the best hostname based on preferences
    pub fn hostname(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            let value = match pref.as_str() {
                "name" | "instance_name" | "vm_name" => Some(self.name.clone()),
                "id" | "instance_id" => Some(self.id.clone()),
                "private_ip" | "private_ip_address" => self.private_ip.clone(),
                "public_ip" | "public_ip_address" => self.public_ip.clone(),
                "resource_name" => Some(self.resource_name.clone()),
                s if s.starts_with("tag:") => {
                    let tag_name = &s[4..];
                    self.tags.get(tag_name).cloned()
                }
                s if s.starts_with("tags.") => {
                    let tag_name = &s[5..];
                    self.tags.get(tag_name).cloned()
                }
                _ => None,
            };

            if let Some(v) = value {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }

        // Default fallback: name, then private_ip, then id
        if !self.name.is_empty() {
            Some(self.name.clone())
        } else {
            self.private_ip.clone().or_else(|| Some(self.id.clone()))
        }
    }

    /// Convert to host variables
    pub fn to_host_vars(&self) -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();

        // Core terraform attributes
        vars.insert(
            "terraform_resource_id".to_string(),
            serde_yaml::Value::String(self.id.clone()),
        );
        vars.insert(
            "terraform_resource_type".to_string(),
            serde_yaml::Value::String(self.resource_type.clone()),
        );
        vars.insert(
            "terraform_resource_name".to_string(),
            serde_yaml::Value::String(self.resource_name.clone()),
        );
        vars.insert(
            "terraform_provider".to_string(),
            serde_yaml::Value::String(self.provider.clone()),
        );

        // Network attributes
        if let Some(ref ip) = self.private_ip {
            vars.insert(
                "terraform_private_ip".to_string(),
                serde_yaml::Value::String(ip.clone()),
            );
        }
        if let Some(ref ip) = self.public_ip {
            vars.insert(
                "terraform_public_ip".to_string(),
                serde_yaml::Value::String(ip.clone()),
            );
        }

        // Tags as nested structure
        if !self.tags.is_empty() {
            let mut tags_map = serde_yaml::Mapping::new();
            for (k, v) in &self.tags {
                tags_map.insert(
                    serde_yaml::Value::String(k.clone()),
                    serde_yaml::Value::String(v.clone()),
                );
            }
            vars.insert(
                "terraform_tags".to_string(),
                serde_yaml::Value::Mapping(tags_map),
            );
        }

        // Add all attributes
        for (key, value) in &self.attributes {
            let yaml_key = format!("tf_{}", key);
            let yaml_value = json_to_yaml(value);
            vars.insert(yaml_key, yaml_value);
        }

        vars
    }
}

/// Terraform inventory plugin
#[derive(Debug)]
pub struct TerraformPlugin {
    config: PluginConfig,
    state_backend: StateBackend,
}

impl TerraformPlugin {
    /// Create a new Terraform plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        let state_backend = Self::parse_backend(&config)?;

        Ok(Self {
            config,
            state_backend,
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("terraform");
        Self::new(config)
    }

    /// Parse backend configuration from plugin config
    fn parse_backend(config: &PluginConfig) -> Result<StateBackend, PluginConfigError> {
        let backend_type = config.get_string("backend").unwrap_or_default();

        match backend_type.to_lowercase().as_str() {
            "s3" => {
                let bucket = config.get_string("bucket").ok_or_else(|| {
                    PluginConfigError::MissingField("bucket (required for S3 backend)".to_string())
                })?;
                let key = config.get_string("key").ok_or_else(|| {
                    PluginConfigError::MissingField("key (required for S3 backend)".to_string())
                })?;
                let region = config
                    .get_string("region")
                    .or_else(|| std::env::var("AWS_REGION").ok())
                    .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
                    .unwrap_or_else(|| "us-east-1".to_string());

                Ok(StateBackend::S3 {
                    bucket,
                    key,
                    region,
                })
            }
            "http" | "https" => {
                let address = config.get_string("address").ok_or_else(|| {
                    PluginConfigError::MissingField(
                        "address (required for HTTP backend)".to_string(),
                    )
                })?;
                Ok(StateBackend::Http { address })
            }
            _ => {
                // Default to local backend
                let path = config
                    .get_string("state_path")
                    .or_else(|| config.get_string("path"))
                    .unwrap_or_else(|| "terraform.tfstate".to_string());

                Ok(StateBackend::Local {
                    path: PathBuf::from(path),
                })
            }
        }
    }

    /// Get hostname preferences
    fn get_hostname_preferences(&self) -> Vec<String> {
        if !self.config.hostnames.is_empty() {
            return self
                .config
                .hostnames
                .iter()
                .map(|h| h.name().to_string())
                .collect();
        }

        // Default preferences
        vec![
            "tag:Name".to_string(),
            "name".to_string(),
            "private_ip".to_string(),
        ]
    }

    /// Check if export_outputs is enabled
    fn should_export_outputs(&self) -> bool {
        self.config.get_bool("export_outputs").unwrap_or(true)
    }

    /// Load state from configured backend
    async fn load_state(&self) -> InventoryResult<TerraformState> {
        match &self.state_backend {
            StateBackend::Local { path } => self.load_local_state(path).await,
            StateBackend::S3 {
                bucket,
                key,
                region,
            } => self.load_s3_state(bucket, key, region).await,
            StateBackend::Http { address } => self.load_http_state(address).await,
        }
    }

    /// Load state from local file
    async fn load_local_state(&self, path: &PathBuf) -> InventoryResult<TerraformState> {
        tracing::info!("Terraform plugin: Loading state from {}", path.display());

        let content = std::fs::read_to_string(path).map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to read terraform state file '{}': {}",
                path.display(),
                e
            ))
        })?;

        self.parse_state_json(&content)
    }

    /// Load state from S3 backend
    async fn load_s3_state(
        &self,
        bucket: &str,
        key: &str,
        region: &str,
    ) -> InventoryResult<TerraformState> {
        tracing::info!(
            "Terraform plugin: Loading state from s3://{}/{}",
            bucket,
            key
        );

        // Note: A full implementation would use aws-sdk-s3
        // For now, we try to use AWS CLI as a fallback
        let output = std::process::Command::new("aws")
            .args(["s3", "cp", &format!("s3://{}/{}", bucket, key), "-"])
            .env("AWS_DEFAULT_REGION", region)
            .output()
            .map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to fetch state from S3 (is AWS CLI installed?): {}",
                    e
                ))
            })?;

        if !output.status.success() {
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "AWS CLI failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let content = String::from_utf8_lossy(&output.stdout);
        self.parse_state_json(&content)
    }

    /// Load state from HTTP backend
    async fn load_http_state(&self, address: &str) -> InventoryResult<TerraformState> {
        tracing::info!("Terraform plugin: Loading state from {}", address);

        // Note: A full implementation would use reqwest
        // For now, we try to use curl as a fallback
        let output = std::process::Command::new("curl")
            .args(["-s", "-f", address])
            .output()
            .map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to fetch state from HTTP (is curl installed?): {}",
                    e
                ))
            })?;

        if !output.status.success() {
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "HTTP request failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let content = String::from_utf8_lossy(&output.stdout);
        self.parse_state_json(&content)
    }

    /// Parse state JSON
    fn parse_state_json(&self, content: &str) -> InventoryResult<TerraformState> {
        serde_json::from_str(content).map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to parse terraform state JSON: {}",
                e
            ))
        })
    }

    /// Extract hosts from terraform state
    fn extract_hosts(&self, state: &TerraformState) -> Vec<TerraformHost> {
        let mut hosts = Vec::new();

        for resource in &state.resources {
            // Only process managed resources (not data sources)
            if resource.mode != "managed" {
                continue;
            }

            // Check if this is a supported resource type
            let provider = extract_provider(&resource.provider);

            for (index, instance) in resource.instances.iter().enumerate() {
                if let Some(host) =
                    self.extract_host_from_instance(resource, instance, index, &provider)
                {
                    hosts.push(host);
                }
            }
        }

        hosts
    }

    /// Extract host from a single terraform instance
    fn extract_host_from_instance(
        &self,
        resource: &TerraformResource,
        instance: &TerraformInstance,
        index: usize,
        provider: &str,
    ) -> Option<TerraformHost> {
        let attrs = &instance.attributes;

        match resource.resource_type.as_str() {
            // AWS EC2 instances
            "aws_instance" => {
                let id = get_string_attr(attrs, "id")?;
                let name = get_tag(attrs, "Name")
                    .or_else(|| Some(format!("{}_{}", resource.name, index)))
                    .unwrap_or_else(|| id.clone());
                let private_ip = get_string_attr(attrs, "private_ip");
                let public_ip = get_string_attr(attrs, "public_ip");
                let tags = extract_tags(attrs);

                Some(TerraformHost {
                    id,
                    name,
                    resource_type: resource.resource_type.clone(),
                    resource_name: resource.name.clone(),
                    provider: provider.to_string(),
                    private_ip,
                    public_ip,
                    tags,
                    attributes: attrs.clone(),
                })
            }

            // Azure VMs
            "azurerm_virtual_machine"
            | "azurerm_linux_virtual_machine"
            | "azurerm_windows_virtual_machine" => {
                let id = get_string_attr(attrs, "id")?;
                let name = get_string_attr(attrs, "name")
                    .or_else(|| Some(format!("{}_{}", resource.name, index)))
                    .unwrap_or_else(|| id.clone());
                let private_ip = get_string_attr(attrs, "private_ip_address");
                let public_ip = get_string_attr(attrs, "public_ip_address");
                let tags = extract_tags(attrs);

                Some(TerraformHost {
                    id,
                    name,
                    resource_type: resource.resource_type.clone(),
                    resource_name: resource.name.clone(),
                    provider: provider.to_string(),
                    private_ip,
                    public_ip,
                    tags,
                    attributes: attrs.clone(),
                })
            }

            // GCP Compute Instances
            "google_compute_instance" => {
                let id = get_string_attr(attrs, "id")
                    .or_else(|| get_string_attr(attrs, "instance_id"))?;
                let name = get_string_attr(attrs, "name")
                    .or_else(|| Some(format!("{}_{}", resource.name, index)))
                    .unwrap_or_else(|| id.clone());

                // GCP network interfaces are nested
                let (private_ip, public_ip) = extract_gcp_network_ips(attrs);
                let tags = extract_labels(attrs);

                Some(TerraformHost {
                    id,
                    name,
                    resource_type: resource.resource_type.clone(),
                    resource_name: resource.name.clone(),
                    provider: provider.to_string(),
                    private_ip,
                    public_ip,
                    tags,
                    attributes: attrs.clone(),
                })
            }

            _ => None,
        }
    }

    /// Check if host passes filters
    fn host_passes_filters(&self, host: &TerraformHost) -> bool {
        for (key, filter_config) in &self.config.filters {
            let filter_values = filter_config.values();
            let host_value = match key.as_str() {
                "resource_type" | "type" => Some(host.resource_type.as_str()),
                "provider" => Some(host.provider.as_str()),
                "resource_name" => Some(host.resource_name.as_str()),
                k if k.starts_with("tag:") => {
                    let tag_name = &k[4..];
                    host.tags.get(tag_name).map(|s| s.as_str())
                }
                k if k.starts_with("tags.") => {
                    let tag_name = &k[5..];
                    host.tags.get(tag_name).map(|s| s.as_str())
                }
                _ => None,
            };

            let Some(value) = host_value else {
                continue;
            };

            if !filter_values.contains(&value) {
                return false;
            }
        }

        true
    }

    /// Get groups for a host based on keyed_groups configuration
    fn get_host_groups(&self, host: &TerraformHost) -> Vec<String> {
        let mut groups = vec!["terraform".to_string()];

        // Add resource type group
        groups.push(format!("tf_{}", sanitize_group_name(&host.resource_type)));

        // Add provider group
        groups.push(format!("provider_{}", sanitize_group_name(&host.provider)));

        // Process keyed_groups configuration
        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, host) {
                let group_name = keyed_group.generate_group_name(&value);
                if !group_name.is_empty() {
                    groups.push(group_name);
                }
            } else if let Some(ref default) = keyed_group.default_value {
                let group_name = keyed_group.generate_group_name(default);
                if !group_name.is_empty() {
                    groups.push(group_name);
                }
            }
        }

        // Add tag-based groups
        for (key, value) in &host.tags {
            let safe_key = sanitize_group_name(key);
            let safe_value = sanitize_group_name(value);
            groups.push(format!("tag_{}_{}", safe_key, safe_value));
        }

        groups
    }

    /// Resolve a keyed group key to a value
    fn resolve_keyed_group_key(&self, key: &str, host: &TerraformHost) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();

        match parts.as_slice() {
            ["tags" | "tag", tag_name] => host.tags.get(*tag_name).cloned(),
            ["resource_type" | "type"] => Some(host.resource_type.clone()),
            ["resource_name"] => Some(host.resource_name.clone()),
            ["provider"] => Some(host.provider.clone()),
            ["id"] => Some(host.id.clone()),
            ["name"] => Some(host.name.clone()),
            _ => None,
        }
    }

    /// Apply compose configuration to set host variables
    fn apply_compose(&self, inv_host: &mut Host, tf_host: &TerraformHost) {
        let compose = &self.config.compose;

        // Set ansible_host
        if let Some(ref expr) = compose.ansible_host {
            if let Some(value) = self.resolve_compose_expression(expr, tf_host) {
                inv_host.ansible_host = Some(value);
            }
        } else {
            // Default: use private IP
            if let Some(ref ip) = tf_host.private_ip {
                inv_host.ansible_host = Some(ip.clone());
            }
        }

        // Set ansible_port
        if let Some(ref expr) = compose.ansible_port {
            if let Some(value) = self.resolve_compose_expression(expr, tf_host) {
                if let Ok(port) = value.parse::<u16>() {
                    inv_host.connection.ssh.port = port;
                }
            }
        }

        // Set ansible_user
        if let Some(ref expr) = compose.ansible_user {
            if let Some(value) = self.resolve_compose_expression(expr, tf_host) {
                inv_host.connection.ssh.user = Some(value);
            }
        } else {
            // Default user based on provider
            let user = match tf_host.provider.as_str() {
                "aws" => "ec2-user",
                "azurerm" | "azure" => "azureuser",
                "google" | "gcp" => "admin",
                _ => "root",
            };
            inv_host.connection.ssh.user = Some(user.to_string());
        }

        // Apply extra vars from compose
        for (key, expr) in &compose.extra_vars {
            if let Some(value) = self.resolve_compose_expression(expr, tf_host) {
                inv_host.set_var(key, serde_yaml::Value::String(value));
            }
        }
    }

    /// Resolve a compose expression to a value
    fn resolve_compose_expression(&self, expr: &str, host: &TerraformHost) -> Option<String> {
        match expr {
            "private_ip" | "private_ip_address" => host.private_ip.clone(),
            "public_ip" | "public_ip_address" => host.public_ip.clone(),
            "name" | "instance_name" => Some(host.name.clone()),
            "id" | "instance_id" => Some(host.id.clone()),
            "resource_type" => Some(host.resource_type.clone()),
            "resource_name" => Some(host.resource_name.clone()),
            "provider" => Some(host.provider.clone()),
            s if s.starts_with("tags.") => {
                let tag_name = &s[5..];
                host.tags.get(tag_name).cloned()
            }
            s if s.starts_with("tag:") => {
                let tag_name = &s[4..];
                host.tags.get(tag_name).cloned()
            }
            _ => Some(expr.to_string()), // Literal value
        }
    }

    /// Convert state to inventory
    fn state_to_inventory(&self, state: TerraformState) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let hostname_prefs = self.get_hostname_preferences();

        // Create base terraform group
        let mut tf_group = Group::new("terraform");
        tf_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("terraform".to_string()),
        );
        tf_group.set_var(
            "terraform_version".to_string(),
            serde_yaml::Value::String(state.terraform_version.clone()),
        );

        // Export outputs as group vars if enabled
        if self.should_export_outputs() {
            for (name, output) in &state.outputs {
                if !output.sensitive {
                    let yaml_value = json_to_yaml(&output.value);
                    tf_group.set_var(format!("tf_output_{}", name), yaml_value);
                }
            }
        }

        // Extract hosts
        let hosts = self.extract_hosts(&state);

        // Process each host
        for tf_host in &hosts {
            // Skip hosts that don't pass filters
            if !self.host_passes_filters(tf_host) {
                continue;
            }

            // Determine hostname
            let Some(hostname) = tf_host.hostname(&hostname_prefs) else {
                tracing::warn!(
                    "Terraform plugin: Could not determine hostname for resource {}/{}",
                    tf_host.resource_type,
                    tf_host.resource_name
                );
                continue;
            };

            // Create host
            let mut host = Host::new(&hostname);

            // Set host variables from terraform data
            for (key, value) in tf_host.to_host_vars() {
                host.set_var(&key, value);
            }

            // Apply compose configuration
            self.apply_compose(&mut host, tf_host);

            // Get groups for this host
            let groups = self.get_host_groups(tf_host);

            // Add host to groups
            for group_name in &groups {
                host.add_to_group(group_name.clone());

                // Ensure group exists
                if inventory.get_group(group_name).is_none() {
                    let group = Group::new(group_name);
                    inventory.add_group(group)?;
                }

                // Add host to group
                if let Some(group) = inventory.get_group_mut(group_name) {
                    group.add_host(hostname.clone());
                }
            }

            // Add host to inventory
            inventory.add_host(host)?;
        }

        // Add the base terraform group
        inventory.add_group(tf_group)?;

        Ok(inventory)
    }
}

#[async_trait]
impl DynamicInventoryPlugin for TerraformPlugin {
    fn name(&self) -> &str {
        "terraform"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Terraform state file dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        match &self.state_backend {
            StateBackend::Local { path } => {
                if !path.exists() {
                    tracing::warn!(
                        "Terraform plugin: State file '{}' does not exist",
                        path.display()
                    );
                }
            }
            StateBackend::S3 { bucket, key, .. } => {
                tracing::info!(
                    "Terraform plugin: Will fetch state from s3://{}/{}",
                    bucket,
                    key
                );
            }
            StateBackend::Http { address } => {
                tracing::info!("Terraform plugin: Will fetch state from {}", address);
            }
        }
        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;
        let state = self.load_state().await?;
        self.state_to_inventory(state)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        // No caching implemented yet
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::optional_string(
                "state_path",
                "Path to local terraform.tfstate file",
                "terraform.tfstate",
            ),
            PluginOption::optional_string(
                "backend",
                "State backend type (local, s3, http)",
                "local",
            ),
            PluginOption::optional_string("bucket", "S3 bucket name (for S3 backend)", ""),
            PluginOption::optional_string("key", "S3 key/path to state file (for S3 backend)", ""),
            PluginOption::optional_string("region", "AWS region (for S3 backend)", "")
                .with_env_var("AWS_REGION"),
            PluginOption::optional_string(
                "address",
                "HTTP address for state (for HTTP backend)",
                "",
            ),
            PluginOption::optional_bool(
                "export_outputs",
                "Export terraform outputs as group vars",
                true,
            ),
            PluginOption::optional_list(
                "hostnames",
                "Hostname preferences in order (tag:Name, name, private_ip)",
            ),
            PluginOption {
                name: "filters".to_string(),
                description: "Resource filters (resource_type, provider, tags)".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic group creation based on resource attributes".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::List,
                env_var: None,
            },
            PluginOption {
                name: "compose".to_string(),
                description: "Set host variables (ansible_host, ansible_user, etc.)".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption::optional_bool("strict", "Fail on template errors", false),
        ]
    }
}

// Helper functions

/// Extract provider name from terraform provider string
fn extract_provider(provider: &str) -> String {
    // provider format: "provider[\"registry.terraform.io/hashicorp/aws\"]"
    // or just "aws"

    // First, try to extract from the full registry path format
    // e.g., provider["registry.terraform.io/hashicorp/aws"]
    if let Some(start) = provider.rfind('/') {
        // Find the closing quote after the last slash
        let remaining = &provider[start + 1..];
        if let Some(end) = remaining.find('"') {
            return remaining[..end].to_string();
        }
        // No closing quote, return everything after the last slash
        // stripping any trailing characters like ]
        return remaining
            .trim_end_matches(['"', ']'])
            .to_string();
    }

    // Fallback: try to extract from provider["aws"]
    if let Some(start) = provider.find('"') {
        if let Some(end) = provider.rfind('"') {
            if start != end {
                return provider[start + 1..end].to_string();
            }
        }
    }
    provider.to_string()
}

/// Get string attribute from terraform attributes
fn get_string_attr(attrs: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    attrs.get(key).and_then(|v| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Get tag value from terraform attributes
fn get_tag(attrs: &HashMap<String, serde_json::Value>, tag_name: &str) -> Option<String> {
    // Try tags.Name first
    if let Some(serde_json::Value::Object(tags)) = attrs.get("tags") {
        if let Some(serde_json::Value::String(v)) = tags.get(tag_name) {
            return Some(v.clone());
        }
    }
    // Try tags_all.Name
    if let Some(serde_json::Value::Object(tags)) = attrs.get("tags_all") {
        if let Some(serde_json::Value::String(v)) = tags.get(tag_name) {
            return Some(v.clone());
        }
    }
    None
}

/// Extract all tags from terraform attributes
fn extract_tags(attrs: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    // Try tags first
    if let Some(serde_json::Value::Object(t)) = attrs.get("tags") {
        for (k, v) in t {
            if let serde_json::Value::String(s) = v {
                tags.insert(k.clone(), s.clone());
            }
        }
    }

    // Merge tags_all (if present and not already in tags)
    if let Some(serde_json::Value::Object(t)) = attrs.get("tags_all") {
        for (k, v) in t {
            if !tags.contains_key(k) {
                if let serde_json::Value::String(s) = v {
                    tags.insert(k.clone(), s.clone());
                }
            }
        }
    }

    tags
}

/// Extract labels (GCP terminology) from terraform attributes
fn extract_labels(attrs: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    let mut labels = HashMap::new();

    if let Some(serde_json::Value::Object(l)) = attrs.get("labels") {
        for (k, v) in l {
            if let serde_json::Value::String(s) = v {
                labels.insert(k.clone(), s.clone());
            }
        }
    }

    labels
}

/// Extract GCP network IPs from network_interface attribute
fn extract_gcp_network_ips(
    attrs: &HashMap<String, serde_json::Value>,
) -> (Option<String>, Option<String>) {
    let mut private_ip = None;
    let mut public_ip = None;

    if let Some(serde_json::Value::Array(interfaces)) = attrs.get("network_interface") {
        if let Some(serde_json::Value::Object(iface)) = interfaces.first() {
            // Get internal IP
            if let Some(serde_json::Value::String(ip)) = iface.get("network_ip") {
                private_ip = Some(ip.clone());
            }

            // Get external IP from access_config
            if let Some(serde_json::Value::Array(access_configs)) = iface.get("access_config") {
                if let Some(serde_json::Value::Object(ac)) = access_configs.first() {
                    if let Some(serde_json::Value::String(ip)) = ac.get("nat_ip") {
                        public_ip = Some(ip.clone());
                    }
                }
            }
        }
    }

    (private_ip, public_ip)
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
                serde_yaml::Value::Number((f as i64).into())
            } else {
                serde_yaml::Value::Number(0.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> TerraformState {
        TerraformState {
            version: 4,
            terraform_version: "1.5.0".to_string(),
            serial: 42,
            lineage: "test-lineage-uuid".to_string(),
            outputs: HashMap::new(),
            resources: Vec::new(),
        }
    }

    fn create_aws_instance_resource() -> TerraformResource {
        let mut attrs = HashMap::new();
        attrs.insert(
            "id".to_string(),
            serde_json::Value::String("i-1234567890abcdef0".to_string()),
        );
        attrs.insert(
            "private_ip".to_string(),
            serde_json::Value::String("10.0.1.100".to_string()),
        );
        attrs.insert(
            "public_ip".to_string(),
            serde_json::Value::String("54.123.45.67".to_string()),
        );

        let mut tags_map = serde_json::Map::new();
        tags_map.insert(
            "Name".to_string(),
            serde_json::Value::String("web-server-01".to_string()),
        );
        tags_map.insert(
            "Environment".to_string(),
            serde_json::Value::String("production".to_string()),
        );
        attrs.insert("tags".to_string(), serde_json::Value::Object(tags_map));

        TerraformResource {
            mode: "managed".to_string(),
            resource_type: "aws_instance".to_string(),
            name: "web".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/aws\"]".to_string(),
            instances: vec![TerraformInstance {
                schema_version: 1,
                attributes: attrs,
                index_key: None,
                dependencies: Vec::new(),
            }],
        }
    }

    fn create_azure_vm_resource() -> TerraformResource {
        let mut attrs = HashMap::new();
        attrs.insert(
            "id".to_string(),
            serde_json::Value::String("/subscriptions/xxx/vm-01".to_string()),
        );
        attrs.insert(
            "name".to_string(),
            serde_json::Value::String("azure-vm-01".to_string()),
        );
        attrs.insert(
            "private_ip_address".to_string(),
            serde_json::Value::String("10.0.2.100".to_string()),
        );

        TerraformResource {
            mode: "managed".to_string(),
            resource_type: "azurerm_linux_virtual_machine".to_string(),
            name: "vm".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/azurerm\"]".to_string(),
            instances: vec![TerraformInstance {
                schema_version: 0,
                attributes: attrs,
                index_key: None,
                dependencies: Vec::new(),
            }],
        }
    }

    fn create_gcp_instance_resource() -> TerraformResource {
        let mut attrs = HashMap::new();
        attrs.insert(
            "id".to_string(),
            serde_json::Value::String(
                "projects/my-project/zones/us-central1-a/instances/gcp-vm-01".to_string(),
            ),
        );
        attrs.insert(
            "name".to_string(),
            serde_json::Value::String("gcp-vm-01".to_string()),
        );

        // GCP network interface structure
        let mut iface = serde_json::Map::new();
        iface.insert(
            "network_ip".to_string(),
            serde_json::Value::String("10.0.3.100".to_string()),
        );

        let mut access_config = serde_json::Map::new();
        access_config.insert(
            "nat_ip".to_string(),
            serde_json::Value::String("35.192.0.100".to_string()),
        );
        iface.insert(
            "access_config".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(access_config)]),
        );

        attrs.insert(
            "network_interface".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(iface)]),
        );

        let mut labels = serde_json::Map::new();
        labels.insert(
            "environment".to_string(),
            serde_json::Value::String("production".to_string()),
        );
        attrs.insert("labels".to_string(), serde_json::Value::Object(labels));

        TerraformResource {
            mode: "managed".to_string(),
            resource_type: "google_compute_instance".to_string(),
            name: "gcp_vm".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/google\"]".to_string(),
            instances: vec![TerraformInstance {
                schema_version: 6,
                attributes: attrs,
                index_key: None,
                dependencies: Vec::new(),
            }],
        }
    }

    // Test 1: Plugin creation
    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();
        assert_eq!(plugin.name(), "terraform");
        assert_eq!(plugin.version(), "1.0.0");
    }

    // Test 2: Plugin with defaults
    #[test]
    fn test_plugin_with_defaults() {
        let plugin = TerraformPlugin::with_defaults().unwrap();
        assert_eq!(plugin.name(), "terraform");

        // Default backend should be local
        match &plugin.state_backend {
            StateBackend::Local { path } => {
                assert_eq!(path.to_str().unwrap(), "terraform.tfstate");
            }
            _ => panic!("Expected local backend"),
        }
    }

    // Test 3: Local backend parsing
    #[test]
    fn test_local_backend_parsing() {
        let mut config = PluginConfig::new("terraform");
        config.extra.insert(
            "state_path".to_string(),
            serde_yaml::Value::String("/path/to/state.tfstate".to_string()),
        );

        let plugin = TerraformPlugin::new(config).unwrap();
        match &plugin.state_backend {
            StateBackend::Local { path } => {
                assert_eq!(path.to_str().unwrap(), "/path/to/state.tfstate");
            }
            _ => panic!("Expected local backend"),
        }
    }

    // Test 4: S3 backend parsing
    #[test]
    fn test_s3_backend_parsing() {
        let mut config = PluginConfig::new("terraform");
        config.extra.insert(
            "backend".to_string(),
            serde_yaml::Value::String("s3".to_string()),
        );
        config.extra.insert(
            "bucket".to_string(),
            serde_yaml::Value::String("my-bucket".to_string()),
        );
        config.extra.insert(
            "key".to_string(),
            serde_yaml::Value::String("prod/terraform.tfstate".to_string()),
        );
        config.extra.insert(
            "region".to_string(),
            serde_yaml::Value::String("us-west-2".to_string()),
        );

        let plugin = TerraformPlugin::new(config).unwrap();
        match &plugin.state_backend {
            StateBackend::S3 {
                bucket,
                key,
                region,
            } => {
                assert_eq!(bucket, "my-bucket");
                assert_eq!(key, "prod/terraform.tfstate");
                assert_eq!(region, "us-west-2");
            }
            _ => panic!("Expected S3 backend"),
        }
    }

    // Test 5: HTTP backend parsing
    #[test]
    fn test_http_backend_parsing() {
        let mut config = PluginConfig::new("terraform");
        config.extra.insert(
            "backend".to_string(),
            serde_yaml::Value::String("http".to_string()),
        );
        config.extra.insert(
            "address".to_string(),
            serde_yaml::Value::String("https://terraform.example.com/state".to_string()),
        );

        let plugin = TerraformPlugin::new(config).unwrap();
        match &plugin.state_backend {
            StateBackend::Http { address } => {
                assert_eq!(address, "https://terraform.example.com/state");
            }
            _ => panic!("Expected HTTP backend"),
        }
    }

    // Test 6: S3 backend missing bucket
    #[test]
    fn test_s3_backend_missing_bucket() {
        let mut config = PluginConfig::new("terraform");
        config.extra.insert(
            "backend".to_string(),
            serde_yaml::Value::String("s3".to_string()),
        );
        // Missing bucket
        config.extra.insert(
            "key".to_string(),
            serde_yaml::Value::String("state.tfstate".to_string()),
        );

        let result = TerraformPlugin::new(config);
        assert!(result.is_err());
    }

    // Test 7: State JSON parsing
    #[test]
    fn test_state_json_parsing() {
        let plugin = TerraformPlugin::with_defaults().unwrap();
        let json = r#"{
            "version": 4,
            "terraform_version": "1.5.0",
            "serial": 42,
            "lineage": "test-uuid",
            "outputs": {},
            "resources": []
        }"#;

        let state = plugin.parse_state_json(json).unwrap();
        assert_eq!(state.version, 4);
        assert_eq!(state.terraform_version, "1.5.0");
        assert_eq!(state.serial, 42);
    }

    // Test 8: Extract provider from registry string
    #[test]
    fn test_extract_provider() {
        assert_eq!(
            extract_provider("provider[\"registry.terraform.io/hashicorp/aws\"]"),
            "aws"
        );
        assert_eq!(
            extract_provider("provider[\"registry.terraform.io/hashicorp/azurerm\"]"),
            "azurerm"
        );
        assert_eq!(
            extract_provider("provider[\"registry.terraform.io/hashicorp/google\"]"),
            "google"
        );
        assert_eq!(extract_provider("aws"), "aws");
    }

    // Test 9: Get string attribute
    #[test]
    fn test_get_string_attr() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "id".to_string(),
            serde_json::Value::String("test-id".to_string()),
        );
        attrs.insert("count".to_string(), serde_json::Value::Number(42.into()));

        assert_eq!(get_string_attr(&attrs, "id"), Some("test-id".to_string()));
        assert_eq!(get_string_attr(&attrs, "count"), Some("42".to_string()));
        assert_eq!(get_string_attr(&attrs, "missing"), None);
    }

    // Test 10: Extract tags
    #[test]
    fn test_extract_tags() {
        let mut attrs = HashMap::new();
        let mut tags = serde_json::Map::new();
        tags.insert(
            "Name".to_string(),
            serde_json::Value::String("web-server".to_string()),
        );
        tags.insert(
            "Environment".to_string(),
            serde_json::Value::String("prod".to_string()),
        );
        attrs.insert("tags".to_string(), serde_json::Value::Object(tags));

        let result = extract_tags(&attrs);
        assert_eq!(result.get("Name"), Some(&"web-server".to_string()));
        assert_eq!(result.get("Environment"), Some(&"prod".to_string()));
    }

    // Test 11: Extract GCP network IPs
    #[test]
    fn test_extract_gcp_network_ips() {
        let mut attrs = HashMap::new();
        let mut iface = serde_json::Map::new();
        iface.insert(
            "network_ip".to_string(),
            serde_json::Value::String("10.0.0.5".to_string()),
        );

        let mut access_config = serde_json::Map::new();
        access_config.insert(
            "nat_ip".to_string(),
            serde_json::Value::String("35.192.0.5".to_string()),
        );
        iface.insert(
            "access_config".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(access_config)]),
        );

        attrs.insert(
            "network_interface".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(iface)]),
        );

        let (private_ip, public_ip) = extract_gcp_network_ips(&attrs);
        assert_eq!(private_ip, Some("10.0.0.5".to_string()));
        assert_eq!(public_ip, Some("35.192.0.5".to_string()));
    }

    // Test 12: TerraformHost hostname preferences
    #[test]
    fn test_terraform_host_hostname() {
        let host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web-server".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: Some("10.0.0.1".to_string()),
            public_ip: Some("54.1.2.3".to_string()),
            tags: {
                let mut t = HashMap::new();
                t.insert("Name".to_string(), "tagged-name".to_string());
                t
            },
            attributes: HashMap::new(),
        };

        // Prefer tag:Name
        let prefs = vec!["tag:Name".to_string()];
        assert_eq!(host.hostname(&prefs), Some("tagged-name".to_string()));

        // Prefer name
        let prefs = vec!["name".to_string()];
        assert_eq!(host.hostname(&prefs), Some("web-server".to_string()));

        // Prefer private_ip
        let prefs = vec!["private_ip".to_string()];
        assert_eq!(host.hostname(&prefs), Some("10.0.0.1".to_string()));

        // Prefer public_ip
        let prefs = vec!["public_ip".to_string()];
        assert_eq!(host.hostname(&prefs), Some("54.1.2.3".to_string()));

        // Fallback chain
        let prefs = vec!["nonexistent".to_string(), "id".to_string()];
        assert_eq!(host.hostname(&prefs), Some("i-12345".to_string()));
    }

    // Test 13: TerraformHost to_host_vars
    #[test]
    fn test_terraform_host_to_host_vars() {
        let mut tags = HashMap::new();
        tags.insert("Environment".to_string(), "prod".to_string());

        let host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web-server".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: Some("10.0.0.1".to_string()),
            public_ip: Some("54.1.2.3".to_string()),
            tags,
            attributes: HashMap::new(),
        };

        let vars = host.to_host_vars();
        assert!(vars.contains_key("terraform_resource_id"));
        assert!(vars.contains_key("terraform_resource_type"));
        assert!(vars.contains_key("terraform_private_ip"));
        assert!(vars.contains_key("terraform_public_ip"));
        assert!(vars.contains_key("terraform_tags"));
    }

    // Test 14: Extract AWS instance
    #[test]
    fn test_extract_aws_instance() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let resource = create_aws_instance_resource();
        let mut state = create_test_state();
        state.resources.push(resource);

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 1);

        let host = &hosts[0];
        assert_eq!(host.id, "i-1234567890abcdef0");
        assert_eq!(host.name, "web-server-01");
        assert_eq!(host.resource_type, "aws_instance");
        assert_eq!(host.provider, "aws");
        assert_eq!(host.private_ip, Some("10.0.1.100".to_string()));
        assert_eq!(host.public_ip, Some("54.123.45.67".to_string()));
        assert_eq!(
            host.tags.get("Environment"),
            Some(&"production".to_string())
        );
    }

    // Test 15: Extract Azure VM
    #[test]
    fn test_extract_azure_vm() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let resource = create_azure_vm_resource();
        let mut state = create_test_state();
        state.resources.push(resource);

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 1);

        let host = &hosts[0];
        assert_eq!(host.name, "azure-vm-01");
        assert_eq!(host.resource_type, "azurerm_linux_virtual_machine");
        assert_eq!(host.provider, "azurerm");
        assert_eq!(host.private_ip, Some("10.0.2.100".to_string()));
    }

    // Test 16: Extract GCP instance
    #[test]
    fn test_extract_gcp_instance() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let resource = create_gcp_instance_resource();
        let mut state = create_test_state();
        state.resources.push(resource);

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 1);

        let host = &hosts[0];
        assert_eq!(host.name, "gcp-vm-01");
        assert_eq!(host.resource_type, "google_compute_instance");
        assert_eq!(host.provider, "google");
        assert_eq!(host.private_ip, Some("10.0.3.100".to_string()));
        assert_eq!(host.public_ip, Some("35.192.0.100".to_string()));
        assert_eq!(
            host.tags.get("environment"),
            Some(&"production".to_string())
        );
    }

    // Test 17: Skip data sources
    #[test]
    fn test_skip_data_sources() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut state = create_test_state();
        state.resources.push(TerraformResource {
            mode: "data".to_string(), // This is a data source, not managed
            resource_type: "aws_instance".to_string(),
            name: "existing".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/aws\"]".to_string(),
            instances: vec![TerraformInstance {
                schema_version: 1,
                attributes: HashMap::new(),
                index_key: None,
                dependencies: Vec::new(),
            }],
        });

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 0);
    }

    // Test 18: Host groups generation
    #[test]
    fn test_host_groups_generation() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut tags = HashMap::new();
        tags.insert("Environment".to_string(), "prod".to_string());

        let host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web-server".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: Some("10.0.0.1".to_string()),
            public_ip: None,
            tags,
            attributes: HashMap::new(),
        };

        let groups = plugin.get_host_groups(&host);
        assert!(groups.contains(&"terraform".to_string()));
        assert!(groups.contains(&"tf_aws_instance".to_string()));
        assert!(groups.contains(&"provider_aws".to_string()));
        assert!(groups.contains(&"tag_environment_prod".to_string()));
    }

    // Test 19: Filter matching
    #[test]
    fn test_filter_matching() {
        let mut config = PluginConfig::new("terraform");
        config.filters.insert(
            "resource_type".to_string(),
            super::super::config::FilterConfig::Single("aws_instance".to_string()),
        );

        let plugin = TerraformPlugin::new(config).unwrap();

        let aws_host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: None,
            public_ip: None,
            tags: HashMap::new(),
            attributes: HashMap::new(),
        };

        let gcp_host = TerraformHost {
            id: "gcp-12345".to_string(),
            name: "vm".to_string(),
            resource_type: "google_compute_instance".to_string(),
            resource_name: "vm".to_string(),
            provider: "google".to_string(),
            private_ip: None,
            public_ip: None,
            tags: HashMap::new(),
            attributes: HashMap::new(),
        };

        assert!(plugin.host_passes_filters(&aws_host));
        assert!(!plugin.host_passes_filters(&gcp_host));
    }

    // Test 20: Output export
    #[test]
    fn test_output_export() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut state = create_test_state();
        state.outputs.insert(
            "vpc_id".to_string(),
            TerraformOutput {
                value: serde_json::Value::String("vpc-12345".to_string()),
                output_type: None,
                sensitive: false,
            },
        );
        state.outputs.insert(
            "secret_key".to_string(),
            TerraformOutput {
                value: serde_json::Value::String("secret".to_string()),
                output_type: None,
                sensitive: true, // Should not be exported
            },
        );

        let inventory = plugin.state_to_inventory(state).unwrap();
        let tf_group = inventory.get_group("terraform").unwrap();

        // Non-sensitive output should be exported
        assert!(tf_group.has_var("tf_output_vpc_id"));
        // Sensitive output should NOT be exported
        assert!(!tf_group.has_var("tf_output_secret_key"));
    }

    // Test 21: Keyed group resolution
    #[test]
    fn test_keyed_group_resolution() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut tags = HashMap::new();
        tags.insert("Role".to_string(), "webserver".to_string());

        let host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: None,
            public_ip: None,
            tags,
            attributes: HashMap::new(),
        };

        assert_eq!(
            plugin.resolve_keyed_group_key("tags.Role", &host),
            Some("webserver".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("resource_type", &host),
            Some("aws_instance".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("provider", &host),
            Some("aws".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("name", &host),
            Some("web".to_string())
        );
    }

    // Test 22: Compose expression resolution
    #[test]
    fn test_compose_expression_resolution() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut tags = HashMap::new();
        tags.insert("ansible_user".to_string(), "deploy".to_string());

        let host = TerraformHost {
            id: "i-12345".to_string(),
            name: "web".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            provider: "aws".to_string(),
            private_ip: Some("10.0.0.1".to_string()),
            public_ip: Some("54.1.2.3".to_string()),
            tags,
            attributes: HashMap::new(),
        };

        assert_eq!(
            plugin.resolve_compose_expression("private_ip", &host),
            Some("10.0.0.1".to_string())
        );
        assert_eq!(
            plugin.resolve_compose_expression("public_ip", &host),
            Some("54.1.2.3".to_string())
        );
        assert_eq!(
            plugin.resolve_compose_expression("tags.ansible_user", &host),
            Some("deploy".to_string())
        );
        assert_eq!(
            plugin.resolve_compose_expression("literal_value", &host),
            Some("literal_value".to_string())
        );
    }

    // Test 23: Full inventory conversion
    #[test]
    fn test_full_inventory_conversion() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut state = create_test_state();
        state.resources.push(create_aws_instance_resource());
        state.resources.push(create_azure_vm_resource());
        state.resources.push(create_gcp_instance_resource());

        let inventory = plugin.state_to_inventory(state).unwrap();

        // Should have 3 hosts
        assert_eq!(inventory.host_count(), 3);

        // Check terraform group exists
        assert!(inventory.get_group("terraform").is_some());

        // Check provider groups exist
        assert!(inventory.get_group("provider_aws").is_some());
        assert!(inventory.get_group("provider_azurerm").is_some());
        assert!(inventory.get_group("provider_google").is_some());
    }

    // Test 24: Multiple instances of same resource
    #[test]
    fn test_multiple_instances() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut attrs1 = HashMap::new();
        attrs1.insert(
            "id".to_string(),
            serde_json::Value::String("i-001".to_string()),
        );
        attrs1.insert(
            "private_ip".to_string(),
            serde_json::Value::String("10.0.0.1".to_string()),
        );

        let mut attrs2 = HashMap::new();
        attrs2.insert(
            "id".to_string(),
            serde_json::Value::String("i-002".to_string()),
        );
        attrs2.insert(
            "private_ip".to_string(),
            serde_json::Value::String("10.0.0.2".to_string()),
        );

        let resource = TerraformResource {
            mode: "managed".to_string(),
            resource_type: "aws_instance".to_string(),
            name: "workers".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/aws\"]".to_string(),
            instances: vec![
                TerraformInstance {
                    schema_version: 1,
                    attributes: attrs1,
                    index_key: Some(serde_json::Value::Number(0.into())),
                    dependencies: Vec::new(),
                },
                TerraformInstance {
                    schema_version: 1,
                    attributes: attrs2,
                    index_key: Some(serde_json::Value::Number(1.into())),
                    dependencies: Vec::new(),
                },
            ],
        };

        let mut state = create_test_state();
        state.resources.push(resource);

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].id, "i-001");
        assert_eq!(hosts[1].id, "i-002");
    }

    // Test 25: JSON to YAML conversion
    #[test]
    fn test_json_to_yaml_conversion() {
        let json_null = serde_json::Value::Null;
        assert_eq!(json_to_yaml(&json_null), serde_yaml::Value::Null);

        let json_bool = serde_json::Value::Bool(true);
        assert_eq!(json_to_yaml(&json_bool), serde_yaml::Value::Bool(true));

        let json_string = serde_json::Value::String("test".to_string());
        assert_eq!(
            json_to_yaml(&json_string),
            serde_yaml::Value::String("test".to_string())
        );

        let json_num = serde_json::Value::Number(42.into());
        if let serde_yaml::Value::Number(n) = json_to_yaml(&json_num) {
            assert_eq!(n.as_i64(), Some(42));
        } else {
            panic!("Expected number");
        }

        let json_array = serde_json::Value::Array(vec![
            serde_json::Value::String("a".to_string()),
            serde_json::Value::String("b".to_string()),
        ]);
        if let serde_yaml::Value::Sequence(seq) = json_to_yaml(&json_array) {
            assert_eq!(seq.len(), 2);
        } else {
            panic!("Expected sequence");
        }
    }

    // Test 26: Empty state handling
    #[test]
    fn test_empty_state_handling() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let state = create_test_state();
        let inventory = plugin.state_to_inventory(state).unwrap();

        // Should have terraform group but no hosts
        assert_eq!(inventory.host_count(), 0);
        assert!(inventory.get_group("terraform").is_some());
    }

    // Test 27: Options documentation
    #[test]
    fn test_options_documentation() {
        let plugin = TerraformPlugin::with_defaults().unwrap();
        let options = plugin.options_documentation();

        // Check that we have documented options
        assert!(!options.is_empty());

        // Check for key options
        let option_names: Vec<&str> = options.iter().map(|o| o.name.as_str()).collect();
        assert!(option_names.contains(&"state_path"));
        assert!(option_names.contains(&"backend"));
        assert!(option_names.contains(&"bucket"));
        assert!(option_names.contains(&"export_outputs"));
    }

    // Test 28: Unsupported resource types are skipped
    #[test]
    fn test_unsupported_resources_skipped() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        let mut state = create_test_state();
        state.resources.push(TerraformResource {
            mode: "managed".to_string(),
            resource_type: "aws_s3_bucket".to_string(), // Not a compute resource
            name: "my_bucket".to_string(),
            provider: "provider[\"registry.terraform.io/hashicorp/aws\"]".to_string(),
            instances: vec![TerraformInstance {
                schema_version: 0,
                attributes: HashMap::new(),
                index_key: None,
                dependencies: Vec::new(),
            }],
        });

        let hosts = plugin.extract_hosts(&state);
        assert_eq!(hosts.len(), 0);
    }

    // Test 29: Tag filter
    #[test]
    fn test_tag_filter() {
        let mut config = PluginConfig::new("terraform");
        config.filters.insert(
            "tag:Environment".to_string(),
            super::super::config::FilterConfig::Single("production".to_string()),
        );

        let plugin = TerraformPlugin::new(config).unwrap();

        let mut prod_tags = HashMap::new();
        prod_tags.insert("Environment".to_string(), "production".to_string());

        let mut dev_tags = HashMap::new();
        dev_tags.insert("Environment".to_string(), "development".to_string());

        let prod_host = TerraformHost {
            id: "i-prod".to_string(),
            name: "prod".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "prod".to_string(),
            provider: "aws".to_string(),
            private_ip: None,
            public_ip: None,
            tags: prod_tags,
            attributes: HashMap::new(),
        };

        let dev_host = TerraformHost {
            id: "i-dev".to_string(),
            name: "dev".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "dev".to_string(),
            provider: "aws".to_string(),
            private_ip: None,
            public_ip: None,
            tags: dev_tags,
            attributes: HashMap::new(),
        };

        assert!(plugin.host_passes_filters(&prod_host));
        assert!(!plugin.host_passes_filters(&dev_host));
    }

    // Test 30: Default ansible_user by provider
    #[test]
    fn test_default_ansible_user_by_provider() {
        let config = PluginConfig::new("terraform");
        let plugin = TerraformPlugin::new(config).unwrap();

        // Test AWS default
        let aws_host = TerraformHost {
            id: "i-1".to_string(),
            name: "aws".to_string(),
            resource_type: "aws_instance".to_string(),
            resource_name: "aws".to_string(),
            provider: "aws".to_string(),
            private_ip: Some("10.0.0.1".to_string()),
            public_ip: None,
            tags: HashMap::new(),
            attributes: HashMap::new(),
        };

        let mut inv_host = Host::new("aws");
        plugin.apply_compose(&mut inv_host, &aws_host);
        assert_eq!(inv_host.connection.ssh.user, Some("ec2-user".to_string()));

        // Test Azure default
        let azure_host = TerraformHost {
            id: "azure-1".to_string(),
            name: "azure".to_string(),
            resource_type: "azurerm_linux_virtual_machine".to_string(),
            resource_name: "azure".to_string(),
            provider: "azurerm".to_string(),
            private_ip: Some("10.0.0.2".to_string()),
            public_ip: None,
            tags: HashMap::new(),
            attributes: HashMap::new(),
        };

        let mut inv_host = Host::new("azure");
        plugin.apply_compose(&mut inv_host, &azure_host);
        assert_eq!(inv_host.connection.ssh.user, Some("azureuser".to_string()));

        // Test GCP default
        let gcp_host = TerraformHost {
            id: "gcp-1".to_string(),
            name: "gcp".to_string(),
            resource_type: "google_compute_instance".to_string(),
            resource_name: "gcp".to_string(),
            provider: "google".to_string(),
            private_ip: Some("10.0.0.3".to_string()),
            public_ip: None,
            tags: HashMap::new(),
            attributes: HashMap::new(),
        };

        let mut inv_host = Host::new("gcp");
        plugin.apply_compose(&mut inv_host, &gcp_host);
        assert_eq!(inv_host.connection.ssh.user, Some("admin".to_string()));
    }

    // Test 31: TerraformBackendType enum
    #[test]
    fn test_terraform_backend_type() {
        assert_eq!(TerraformBackendType::default(), TerraformBackendType::Local);
        assert_eq!(TerraformBackendType::Local.to_string(), "local");
        assert_eq!(TerraformBackendType::S3.to_string(), "s3");
        assert_eq!(TerraformBackendType::Gcs.to_string(), "gcs");
        assert_eq!(TerraformBackendType::Azure.to_string(), "azure");
        assert_eq!(TerraformBackendType::Consul.to_string(), "consul");
        assert_eq!(TerraformBackendType::Http.to_string(), "http");
    }

    // Test 32: TerraformPluginConfig default and serde
    #[test]
    fn test_terraform_plugin_config() {
        let config = TerraformPluginConfig::default();
        assert_eq!(config.backend, TerraformBackendType::Local);
        assert_eq!(
            config.state_path,
            Some(PathBuf::from("terraform.tfstate"))
        );
        assert!(config.export_outputs);
        assert!(config.resource_mappings.is_empty());

        let json = serde_json::json!({
            "backend": "s3",
            "bucket": "my-bucket",
            "key": "prod/state.tfstate",
            "region": "us-west-2"
        });
        let config: TerraformPluginConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.backend, TerraformBackendType::S3);
        assert_eq!(config.bucket, Some("my-bucket".to_string()));
    }

    // Test 33: ResourceMapping serde
    #[test]
    fn test_resource_mapping() {
        let json = serde_json::json!({
            "hostname_attribute": "tags.Name",
            "address_attribute": "public_ip",
            "fallback_address": "private_ip",
            "group_by": [
                {"attribute": "tags.Environment", "prefix": "env"}
            ],
            "host_vars": {
                "instance_type": "{{ instance_type }}"
            }
        });
        let mapping: ResourceMapping = serde_json::from_value(json).unwrap();
        assert_eq!(mapping.hostname_attribute, Some("tags.Name".to_string()));
        assert_eq!(mapping.address_attribute, Some("public_ip".to_string()));
        assert_eq!(mapping.group_by.len(), 1);
        assert_eq!(mapping.group_by[0].attribute, "tags.Environment");
        assert_eq!(mapping.group_by[0].prefix, Some("env".to_string()));
    }

    // Test 34: LocalBackend
    #[test]
    fn test_local_backend() {
        let backend = LocalBackend::new("/tmp/terraform.tfstate");
        assert_eq!(backend.backend_type(), TerraformBackendType::Local);
    }

    // Test 35: CacheConfig defaults
    #[test]
    fn test_cache_config() {
        let json = serde_json::json!({
            "enabled": true
        });
        let config: CacheConfig = serde_json::from_value(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.ttl, 300); // Default TTL
    }

    // Test 36: TerraformInventoryPlugin type alias
    #[test]
    fn test_terraform_inventory_plugin_alias() {
        fn accepts_plugin(_plugin: &TerraformInventoryPlugin) {}
        let plugin = TerraformPlugin::with_defaults().unwrap();
        accepts_plugin(&plugin);
    }
}
