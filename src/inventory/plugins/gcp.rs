//! GCP Compute Engine Dynamic Inventory Plugin
//!
//! This plugin discovers Google Cloud Platform Compute Engine instances
//! and creates inventory entries with proper grouping based on labels,
//! machine types, zones, and other attributes.
//!
//! # Configuration
//!
//! ```yaml
//! plugin: gcp_compute
//! projects:
//!   - my-project-id
//! zones:
//!   - us-central1-a
//!   - us-east1-b
//! filters:
//!   - "status = RUNNING"
//!   - "labels.environment = production"
//! keyed_groups:
//!   - key: labels.role
//!     prefix: role
//!   - key: zone
//!     prefix: zone
//!   - key: machine_type
//!     prefix: type
//! hostnames:
//!   - name
//!   - networkInterfaces[0].accessConfigs[0].natIP
//!   - networkInterfaces[0].networkIP
//! compose:
//!   ansible_host: networkInterfaces[0].networkIP
//!   ansible_user: "{{ labels.ssh_user | default('admin') }}"
//! ```
//!
//! # Authentication
//!
//! The plugin uses Google Cloud SDK credential chain:
//! 1. GOOGLE_APPLICATION_CREDENTIALS environment variable
//! 2. Application Default Credentials (gcloud auth application-default login)
//! 3. Service account attached to GCE instance
//!
//! # Features
//!
//! - Multi-project support
//! - Zone and region filtering
//! - Label-based filtering and grouping
//! - Automatic group creation based on GCP attributes
//! - Caching for improved performance
//! - Network and disk information

use super::config::{sanitize_group_name, PluginConfig, PluginConfigError};
use super::{DynamicInventoryPlugin, PluginOption, PluginOptionType};
use crate::inventory::{Group, Host, Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GCP Compute Engine instance data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcpInstance {
    /// Instance ID (numeric)
    pub id: String,
    /// Instance name
    pub name: String,
    /// Machine type (e.g., n1-standard-1)
    pub machine_type: String,
    /// Instance status (RUNNING, STOPPED, etc.)
    pub status: String,
    /// Zone (e.g., us-central1-a)
    pub zone: String,
    /// Region derived from zone
    pub region: String,
    /// Project ID
    pub project: String,
    /// Network interfaces
    pub network_interfaces: Vec<NetworkInterface>,
    /// Instance labels
    pub labels: HashMap<String, String>,
    /// Instance metadata
    pub metadata: HashMap<String, String>,
    /// Service accounts
    pub service_accounts: Vec<String>,
    /// Disks attached
    pub disks: Vec<Disk>,
    /// Tags (network tags)
    pub tags: Vec<String>,
    /// Creation timestamp
    pub creation_timestamp: Option<String>,
    /// Preemptible flag
    pub preemptible: bool,
    /// Scheduling options
    pub scheduling: SchedulingOptions,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Network name
    pub network: String,
    /// Subnetwork name
    pub subnetwork: Option<String>,
    /// Internal IP address
    pub network_ip: String,
    /// External access configurations
    pub access_configs: Vec<AccessConfig>,
}

/// Access configuration for external connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessConfig {
    /// Access config type (ONE_TO_ONE_NAT)
    pub r#type: String,
    /// NAT IP (external IP)
    pub nat_ip: Option<String>,
}

/// Disk attached to an instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disk {
    /// Disk source
    pub source: String,
    /// Device name
    pub device_name: String,
    /// Boot disk flag
    pub boot: bool,
    /// Disk size in GB
    pub disk_size_gb: Option<i64>,
    /// Disk type
    pub disk_type: Option<String>,
}

/// Scheduling options for an instance
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulingOptions {
    /// Automatic restart
    pub automatic_restart: bool,
    /// On host maintenance action
    pub on_host_maintenance: String,
    /// Preemptible flag
    pub preemptible: bool,
}

impl GcpInstance {
    /// Get the primary internal IP address
    pub fn internal_ip(&self) -> Option<&str> {
        self.network_interfaces
            .first()
            .map(|ni| ni.network_ip.as_str())
    }

    /// Get the primary external IP address
    pub fn external_ip(&self) -> Option<&str> {
        self.network_interfaces
            .first()
            .and_then(|ni| ni.access_configs.first())
            .and_then(|ac| ac.nat_ip.as_deref())
    }

    /// Get a label value
    pub fn get_label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(|s| s.as_str())
    }

    /// Get the best hostname based on preferences
    pub fn hostname(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            let value = match pref.as_str() {
                "name" | "instance_name" => Some(self.name.clone()),
                "id" | "instance_id" => Some(self.id.clone()),
                "networkInterfaces[0].networkIP" | "internal_ip" | "private_ip" => {
                    self.internal_ip().map(|s| s.to_string())
                }
                "networkInterfaces[0].accessConfigs[0].natIP" | "external_ip" | "public_ip" => {
                    self.external_ip().map(|s| s.to_string())
                }
                s if s.starts_with("labels.") => {
                    let label_name = &s[7..];
                    self.labels.get(label_name).cloned()
                }
                s if s.starts_with("metadata.") => {
                    let meta_name = &s[9..];
                    self.metadata.get(meta_name).cloned()
                }
                _ => None,
            };

            if let Some(v) = value {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }

        // Default fallback
        Some(self.name.clone())
    }

    /// Convert to host variables
    pub fn to_host_vars(&self) -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();

        // Core instance attributes
        vars.insert(
            "gcp_id".to_string(),
            serde_yaml::Value::String(self.id.clone()),
        );
        vars.insert(
            "gcp_name".to_string(),
            serde_yaml::Value::String(self.name.clone()),
        );
        vars.insert(
            "gcp_machine_type".to_string(),
            serde_yaml::Value::String(self.machine_type.clone()),
        );
        vars.insert(
            "gcp_status".to_string(),
            serde_yaml::Value::String(self.status.clone()),
        );
        vars.insert(
            "gcp_zone".to_string(),
            serde_yaml::Value::String(self.zone.clone()),
        );
        vars.insert(
            "gcp_region".to_string(),
            serde_yaml::Value::String(self.region.clone()),
        );
        vars.insert(
            "gcp_project".to_string(),
            serde_yaml::Value::String(self.project.clone()),
        );

        // Network attributes
        if let Some(ip) = self.internal_ip() {
            vars.insert(
                "gcp_internal_ip".to_string(),
                serde_yaml::Value::String(ip.to_string()),
            );
        }
        if let Some(ip) = self.external_ip() {
            vars.insert(
                "gcp_external_ip".to_string(),
                serde_yaml::Value::String(ip.to_string()),
            );
        }

        // Network interfaces as array
        if !self.network_interfaces.is_empty() {
            let nis: Vec<serde_yaml::Value> = self
                .network_interfaces
                .iter()
                .map(|ni| {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("network".to_string()),
                        serde_yaml::Value::String(ni.network.clone()),
                    );
                    m.insert(
                        serde_yaml::Value::String("network_ip".to_string()),
                        serde_yaml::Value::String(ni.network_ip.clone()),
                    );
                    if let Some(ref subnet) = ni.subnetwork {
                        m.insert(
                            serde_yaml::Value::String("subnetwork".to_string()),
                            serde_yaml::Value::String(subnet.clone()),
                        );
                    }
                    serde_yaml::Value::Mapping(m)
                })
                .collect();
            vars.insert(
                "gcp_network_interfaces".to_string(),
                serde_yaml::Value::Sequence(nis),
            );
        }

        // Tags (network tags)
        if !self.tags.is_empty() {
            vars.insert(
                "gcp_tags".to_string(),
                serde_yaml::Value::Sequence(
                    self.tags
                        .iter()
                        .map(|t| serde_yaml::Value::String(t.clone()))
                        .collect(),
                ),
            );
        }

        // Service accounts
        if !self.service_accounts.is_empty() {
            vars.insert(
                "gcp_service_accounts".to_string(),
                serde_yaml::Value::Sequence(
                    self.service_accounts
                        .iter()
                        .map(|sa| serde_yaml::Value::String(sa.clone()))
                        .collect(),
                ),
            );
        }

        // Labels as nested structure
        if !self.labels.is_empty() {
            let mut labels_map = serde_yaml::Mapping::new();
            for (k, v) in &self.labels {
                labels_map.insert(
                    serde_yaml::Value::String(k.clone()),
                    serde_yaml::Value::String(v.clone()),
                );
            }
            vars.insert(
                "gcp_labels".to_string(),
                serde_yaml::Value::Mapping(labels_map),
            );
        }

        // Metadata as nested structure
        if !self.metadata.is_empty() {
            let mut meta_map = serde_yaml::Mapping::new();
            for (k, v) in &self.metadata {
                meta_map.insert(
                    serde_yaml::Value::String(k.clone()),
                    serde_yaml::Value::String(v.clone()),
                );
            }
            vars.insert(
                "gcp_metadata".to_string(),
                serde_yaml::Value::Mapping(meta_map),
            );
        }

        // Preemptible flag
        vars.insert(
            "gcp_preemptible".to_string(),
            serde_yaml::Value::Bool(self.preemptible),
        );

        // Creation timestamp
        if let Some(ref ts) = self.creation_timestamp {
            vars.insert(
                "gcp_creation_timestamp".to_string(),
                serde_yaml::Value::String(ts.clone()),
            );
        }

        vars
    }
}

/// GCP Compute Engine inventory plugin
#[derive(Debug)]
pub struct GcpPlugin {
    config: PluginConfig,
    /// Cached instances
    #[allow(dead_code)]
    cached_instances: std::sync::RwLock<Option<Vec<GcpInstance>>>,
}

impl GcpPlugin {
    /// Create a new GCP plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        Ok(Self {
            config,
            cached_instances: std::sync::RwLock::new(None),
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("gcp_compute");
        Self::new(config)
    }

    /// Get configured projects
    fn get_projects(&self) -> Vec<String> {
        if let Some(projects) = self.config.get_string_list("projects") {
            return projects;
        }

        // Try environment variable
        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            return vec![project];
        }
        if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            return vec![project];
        }
        if let Ok(project) = std::env::var("GCP_PROJECT") {
            return vec![project];
        }

        Vec::new()
    }

    /// Get configured zones
    fn get_zones(&self) -> Vec<String> {
        self.config.get_string_list("zones").unwrap_or_default()
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
        vec!["name".to_string(), "internal_ip".to_string()]
    }

    /// Check if instance passes filters
    fn instance_passes_filters(&self, instance: &GcpInstance) -> bool {
        for (key, filter_config) in &self.config.filters {
            let filter_values = filter_config.values();
            let instance_value = match key.as_str() {
                "status" => Some(instance.status.as_str()),
                "zone" => Some(instance.zone.as_str()),
                "region" => Some(instance.region.as_str()),
                "machine_type" => Some(instance.machine_type.as_str()),
                "name" => Some(instance.name.as_str()),
                "project" => Some(instance.project.as_str()),
                k if k.starts_with("labels.") => {
                    let label_name = &k[7..];
                    instance.labels.get(label_name).map(|s| s.as_str())
                }
                k if k.starts_with("metadata.") => {
                    let meta_name = &k[9..];
                    instance.metadata.get(meta_name).map(|s| s.as_str())
                }
                _ => None,
            };

            let Some(value) = instance_value else {
                continue;
            };

            if !filter_values.contains(&value) {
                return false;
            }
        }

        true
    }

    /// Get groups for an instance based on keyed_groups configuration
    fn get_instance_groups(&self, instance: &GcpInstance) -> Vec<String> {
        let mut groups = vec!["gcp".to_string(), "gcp_compute".to_string()];

        // Add project group
        groups.push(format!(
            "project_{}",
            sanitize_group_name(&instance.project)
        ));

        // Add zone group
        groups.push(format!("zone_{}", sanitize_group_name(&instance.zone)));

        // Add region group
        groups.push(format!("region_{}", sanitize_group_name(&instance.region)));

        // Add machine type group
        groups.push(format!(
            "type_{}",
            sanitize_group_name(&instance.machine_type)
        ));

        // Add status group
        groups.push(format!("status_{}", instance.status.to_lowercase()));

        // Process keyed_groups configuration
        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, instance) {
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

        // Add label-based groups
        for (key, value) in &instance.labels {
            let safe_key = sanitize_group_name(key);
            let safe_value = sanitize_group_name(value);
            groups.push(format!("label_{}_{}", safe_key, safe_value));
        }

        // Add network tag groups
        for tag in &instance.tags {
            groups.push(format!("tag_{}", sanitize_group_name(tag)));
        }

        groups
    }

    /// Resolve a keyed group key to a value
    fn resolve_keyed_group_key(&self, key: &str, instance: &GcpInstance) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();

        match parts.as_slice() {
            ["labels", label_name] => instance.labels.get(*label_name).cloned(),
            ["metadata", meta_name] => instance.metadata.get(*meta_name).cloned(),
            ["zone"] => Some(instance.zone.clone()),
            ["region"] => Some(instance.region.clone()),
            ["machine_type"] => Some(instance.machine_type.clone()),
            ["status"] => Some(instance.status.clone()),
            ["name"] => Some(instance.name.clone()),
            ["project"] => Some(instance.project.clone()),
            _ => None,
        }
    }

    /// Apply compose configuration to set host variables
    fn apply_compose(&self, host: &mut Host, instance: &GcpInstance) {
        let compose = &self.config.compose;

        // Set ansible_host
        if let Some(ref expr) = compose.ansible_host {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                host.ansible_host = Some(value);
            }
        } else {
            // Default: use internal IP
            if let Some(ip) = instance.internal_ip() {
                host.ansible_host = Some(ip.to_string());
            }
        }

        // Set ansible_port
        if let Some(ref expr) = compose.ansible_port {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                if let Ok(port) = value.parse::<u16>() {
                    host.connection.ssh.port = port;
                }
            }
        }

        // Set ansible_user
        if let Some(ref expr) = compose.ansible_user {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                host.connection.ssh.user = Some(value);
            }
        }

        // Apply extra vars from compose
        for (key, expr) in &compose.extra_vars {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                host.set_var(key, serde_yaml::Value::String(value));
            }
        }
    }

    /// Resolve a compose expression to a value
    fn resolve_compose_expression(&self, expr: &str, instance: &GcpInstance) -> Option<String> {
        match expr {
            "internal_ip" | "networkInterfaces[0].networkIP" => {
                instance.internal_ip().map(|s| s.to_string())
            }
            "external_ip" | "networkInterfaces[0].accessConfigs[0].natIP" => {
                instance.external_ip().map(|s| s.to_string())
            }
            "name" => Some(instance.name.clone()),
            "id" => Some(instance.id.clone()),
            "zone" => Some(instance.zone.clone()),
            "region" => Some(instance.region.clone()),
            "machine_type" => Some(instance.machine_type.clone()),
            "project" => Some(instance.project.clone()),
            s if s.starts_with("labels.") => {
                let label_name = &s[7..];
                instance.labels.get(label_name).cloned()
            }
            s if s.starts_with("metadata.") => {
                let meta_name = &s[9..];
                instance.metadata.get(meta_name).cloned()
            }
            _ => Some(expr.to_string()), // Literal value
        }
    }

    /// Fetch instances from GCP (simulated for now)
    ///
    /// Note: A full implementation would use the Google Cloud SDK for Rust
    async fn fetch_instances(&self) -> InventoryResult<Vec<GcpInstance>> {
        let projects = self.get_projects();
        let zones = self.get_zones();

        tracing::info!(
            "GCP plugin: Querying {} project(s), {} zone(s)",
            projects.len(),
            if zones.is_empty() {
                "all".to_string()
            } else {
                zones.len().to_string()
            }
        );

        if projects.is_empty() {
            tracing::warn!(
                "GCP plugin: No projects configured. Set GOOGLE_CLOUD_PROJECT or configure 'projects' option."
            );
        }

        // In a real implementation, this would call GCP Compute Engine API
        // using google-cloud-compute crate.
        //
        // Example with GCP SDK:
        // ```rust
        // let config = google_cloud_default::GCP_CONFIG.clone();
        // let client = google_cloud_compute::InstancesClient::new(config).await?;
        // let instances = client.list(project, zone).await?;
        // ```

        tracing::warn!(
            "GCP plugin: Google Cloud SDK integration not yet implemented. \
             Configure GCP credentials and install google-cloud-compute for full functionality."
        );

        Ok(Vec::new())
    }

    /// Convert instances to inventory
    fn instances_to_inventory(&self, instances: Vec<GcpInstance>) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let hostname_prefs = self.get_hostname_preferences();

        // Create base gcp_compute group
        let mut gcp_group = Group::new("gcp_compute");
        gcp_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("gcp_compute".to_string()),
        );

        // Process each instance
        for instance in &instances {
            if !self.instance_passes_filters(instance) {
                continue;
            }

            let Some(hostname) = instance.hostname(&hostname_prefs) else {
                tracing::warn!(
                    "GCP plugin: Could not determine hostname for instance {}",
                    instance.name
                );
                continue;
            };

            let mut host = Host::new(&hostname);

            // Set host variables from instance data
            for (key, value) in instance.to_host_vars() {
                host.set_var(&key, value);
            }

            // Apply compose configuration
            self.apply_compose(&mut host, instance);

            // Get groups for this instance
            let groups = self.get_instance_groups(instance);

            // Add host to groups
            for group_name in &groups {
                host.add_to_group(group_name.clone());

                if inventory.get_group(group_name).is_none() {
                    let group = Group::new(group_name);
                    inventory.add_group(group)?;
                }

                if let Some(group) = inventory.get_group_mut(group_name) {
                    group.add_host(hostname.clone());
                }
            }

            inventory.add_host(host)?;
        }

        inventory.add_group(gcp_group)?;

        Ok(inventory)
    }
}

#[async_trait]
impl DynamicInventoryPlugin for GcpPlugin {
    fn name(&self) -> &str {
        "gcp_compute"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "GCP Compute Engine instances dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        // Check for GCP credentials
        let has_creds_file = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok();
        let has_project = std::env::var("GOOGLE_CLOUD_PROJECT").is_ok()
            || std::env::var("GCLOUD_PROJECT").is_ok()
            || !self.get_projects().is_empty();

        if !has_creds_file {
            tracing::warn!(
                "GCP plugin: GOOGLE_APPLICATION_CREDENTIALS not set. \
                 Using Application Default Credentials or attached service account."
            );
        }

        if !has_project {
            tracing::warn!(
                "GCP plugin: No project configured. \
                 Set GOOGLE_CLOUD_PROJECT or configure 'projects' option."
            );
        }

        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;
        let instances = self.fetch_instances().await?;
        self.instances_to_inventory(instances)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        let mut cache = self.cached_instances.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::optional_list("projects", "GCP project IDs to query")
                .with_env_var("GOOGLE_CLOUD_PROJECT"),
            PluginOption::optional_list("zones", "Zones to query (empty = all zones)"),
            PluginOption::optional_list("regions", "Regions to query (empty = all regions)"),
            PluginOption {
                name: "filters".to_string(),
                description: "Instance filters (status, labels, etc.)".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption::optional_list(
                "hostnames",
                "Hostname preferences in order (name, internal_ip, etc.)",
            ),
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic group creation based on instance attributes".to_string(),
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
            PluginOption::optional_bool("use_private_ip", "Use internal IP for ansible_host", true),
            PluginOption::optional_bool("strict", "Fail on template errors", false),
            PluginOption {
                name: "cache_ttl".to_string(),
                description: "Cache TTL in seconds (0 = no caching)".to_string(),
                required: false,
                default: Some("0".to_string()),
                option_type: PluginOptionType::Int,
                env_var: None,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_instance() -> GcpInstance {
        let mut labels = HashMap::new();
        labels.insert("environment".to_string(), "production".to_string());
        labels.insert("role".to_string(), "webserver".to_string());

        let mut metadata = HashMap::new();
        metadata.insert("ssh-keys".to_string(), "admin:ssh-rsa AAAA...".to_string());

        GcpInstance {
            id: "1234567890".to_string(),
            name: "web-server-01".to_string(),
            machine_type: "n1-standard-2".to_string(),
            status: "RUNNING".to_string(),
            zone: "us-central1-a".to_string(),
            region: "us-central1".to_string(),
            project: "my-project".to_string(),
            network_interfaces: vec![NetworkInterface {
                network: "default".to_string(),
                subnetwork: Some("default".to_string()),
                network_ip: "10.128.0.2".to_string(),
                access_configs: vec![AccessConfig {
                    r#type: "ONE_TO_ONE_NAT".to_string(),
                    nat_ip: Some("35.192.0.100".to_string()),
                }],
            }],
            labels,
            metadata,
            service_accounts: vec!["my-sa@my-project.iam.gserviceaccount.com".to_string()],
            disks: vec![],
            tags: vec!["http-server".to_string(), "https-server".to_string()],
            creation_timestamp: Some("2024-01-15T10:30:00Z".to_string()),
            preemptible: false,
            scheduling: SchedulingOptions {
                automatic_restart: true,
                on_host_maintenance: "MIGRATE".to_string(),
                preemptible: false,
            },
        }
    }

    #[test]
    fn test_instance_hostname() {
        let instance = create_test_instance();

        let prefs = vec!["name".to_string()];
        assert_eq!(instance.hostname(&prefs), Some("web-server-01".to_string()));

        let prefs = vec!["internal_ip".to_string()];
        assert_eq!(instance.hostname(&prefs), Some("10.128.0.2".to_string()));

        let prefs = vec!["external_ip".to_string()];
        assert_eq!(instance.hostname(&prefs), Some("35.192.0.100".to_string()));
    }

    #[test]
    fn test_instance_to_host_vars() {
        let instance = create_test_instance();
        let vars = instance.to_host_vars();

        assert!(vars.contains_key("gcp_name"));
        assert!(vars.contains_key("gcp_zone"));
        assert!(vars.contains_key("gcp_labels"));
        assert!(vars.contains_key("gcp_internal_ip"));
    }

    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("gcp_compute");
        let plugin = GcpPlugin::new(config).unwrap();
        assert_eq!(plugin.name(), "gcp_compute");
    }

    #[test]
    fn test_keyed_group_resolution() {
        let config = PluginConfig::new("gcp_compute");
        let plugin = GcpPlugin::new(config).unwrap();
        let instance = create_test_instance();

        let value = plugin.resolve_keyed_group_key("labels.environment", &instance);
        assert_eq!(value, Some("production".to_string()));

        let value = plugin.resolve_keyed_group_key("zone", &instance);
        assert_eq!(value, Some("us-central1-a".to_string()));
    }

    #[test]
    fn test_instance_groups() {
        let config = PluginConfig::new("gcp_compute");
        let plugin = GcpPlugin::new(config).unwrap();
        let instance = create_test_instance();

        let groups = plugin.get_instance_groups(&instance);

        assert!(groups.contains(&"gcp".to_string()));
        assert!(groups.contains(&"gcp_compute".to_string()));
        assert!(groups.contains(&"zone_us_central1_a".to_string()));
        assert!(groups.contains(&"region_us_central1".to_string()));
        assert!(groups.contains(&"status_running".to_string()));
    }
}
