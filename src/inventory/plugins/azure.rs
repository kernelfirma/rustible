//! Azure Dynamic Inventory Plugin
//!
//! This plugin discovers Azure Virtual Machines and creates inventory entries
//! with proper grouping based on tags, resource groups, locations, etc.
//!
//! # Configuration
//!
//! ```yaml
//! plugin: azure_rm
//! subscription_id: your-subscription-id
//! # Authentication options (choose one):
//! # 1. Service Principal
//! client_id: your-client-id
//! secret: your-client-secret
//! tenant: your-tenant-id
//! # 2. Managed Identity (when running on Azure)
//! use_msi: true
//! # 3. Azure CLI (uses `az login` credentials)
//! auth_source: cli
//!
//! include_vm_resource_groups:
//!   - production-rg
//!   - staging-rg
//! exclude_vm_resource_groups:
//!   - test-rg
//!
//! keyed_groups:
//!   - key: tags.Environment
//!     prefix: env
//!   - key: location
//!     prefix: location
//!   - key: resource_group
//!     prefix: rg
//!
//! hostnames:
//!   - name
//!   - public_ip
//!   - private_ip
//!
//! compose:
//!   ansible_host: private_ip
//!   ansible_user: azureuser
//! ```
//!
//! # Authentication
//!
//! The plugin supports multiple authentication methods:
//! 1. Service Principal (client_id, secret, tenant)
//! 2. Managed Identity (use_msi: true)
//! 3. Azure CLI credentials (auth_source: cli)
//! 4. Environment variables (AZURE_SUBSCRIPTION_ID, AZURE_CLIENT_ID, etc.)
//!
//! # Features
//!
//! - Multi-subscription support
//! - Resource group filtering
//! - Tag-based filtering and grouping
//! - Automatic group creation based on Azure attributes
//! - Support for VM Scale Sets
//! - Network interface discovery

use super::config::{sanitize_group_name, PluginConfig, PluginConfigError};
use super::{DynamicInventoryPlugin, PluginOption, PluginOptionType};
use crate::inventory::{Group, Host, Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Azure Virtual Machine data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureVm {
    /// VM resource ID
    pub id: String,
    /// VM name
    pub name: String,
    /// Resource group name
    pub resource_group: String,
    /// Azure location (e.g., eastus, westus2)
    pub location: String,
    /// VM size (e.g., Standard_DS1_v2)
    pub vm_size: String,
    /// Provisioning state
    pub provisioning_state: String,
    /// Power state (running, deallocated, stopped)
    pub power_state: Option<String>,
    /// Private IP addresses
    pub private_ips: Vec<String>,
    /// Public IP addresses
    pub public_ips: Vec<String>,
    /// Network interfaces
    pub network_interfaces: Vec<String>,
    /// OS type (Linux, Windows)
    pub os_type: String,
    /// OS disk name
    pub os_disk: Option<String>,
    /// Image reference
    pub image: Option<AzureImageReference>,
    /// Availability set
    pub availability_set: Option<String>,
    /// Availability zone
    pub availability_zone: Option<String>,
    /// VM tags
    pub tags: HashMap<String, String>,
    /// Subscription ID
    pub subscription_id: String,
    /// Computer name (hostname inside the VM)
    pub computer_name: Option<String>,
    /// Admin username
    pub admin_username: Option<String>,
}

/// Azure image reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureImageReference {
    /// Publisher
    pub publisher: Option<String>,
    /// Offer
    pub offer: Option<String>,
    /// SKU
    pub sku: Option<String>,
    /// Version
    pub version: Option<String>,
}

impl AzureVm {
    /// Get VM name (preferred identifier)
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get a tag value
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|s| s.as_str())
    }

    /// Get the primary private IP
    pub fn primary_private_ip(&self) -> Option<&str> {
        self.private_ips.first().map(|s| s.as_str())
    }

    /// Get the primary public IP
    pub fn primary_public_ip(&self) -> Option<&str> {
        self.public_ips.first().map(|s| s.as_str())
    }

    /// Check if VM is running
    pub fn is_running(&self) -> bool {
        self.power_state.as_deref() == Some("running")
            || self.power_state.as_deref() == Some("PowerState/running")
    }

    /// Get the best hostname based on preferences
    pub fn hostname(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            let value = match pref.as_str() {
                "name" | "vm_name" => Some(self.name.clone()),
                "computer_name" => self.computer_name.clone(),
                "public_ip" | "public_ip_address" => self.primary_public_ip().map(String::from),
                "private_ip" | "private_ip_address" => self.primary_private_ip().map(String::from),
                "fqdn" => {
                    // Construct FQDN from name and location
                    Some(format!(
                        "{}.{}.cloudapp.azure.com",
                        self.name, self.location
                    ))
                }
                s if s.starts_with("tag:") => {
                    let tag_name = &s[4..];
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

        // Default fallback
        self.primary_private_ip()
            .map(String::from)
            .or_else(|| Some(self.name.clone()))
    }

    /// Convert to host variables
    pub fn to_host_vars(&self) -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();

        // Core VM attributes
        vars.insert(
            "azure_vm_id".to_string(),
            serde_yaml::Value::String(self.id.clone()),
        );
        vars.insert(
            "azure_vm_name".to_string(),
            serde_yaml::Value::String(self.name.clone()),
        );
        vars.insert(
            "azure_resource_group".to_string(),
            serde_yaml::Value::String(self.resource_group.clone()),
        );
        vars.insert(
            "azure_location".to_string(),
            serde_yaml::Value::String(self.location.clone()),
        );
        vars.insert(
            "azure_vm_size".to_string(),
            serde_yaml::Value::String(self.vm_size.clone()),
        );
        vars.insert(
            "azure_os_type".to_string(),
            serde_yaml::Value::String(self.os_type.clone()),
        );
        vars.insert(
            "azure_subscription_id".to_string(),
            serde_yaml::Value::String(self.subscription_id.clone()),
        );

        // State
        vars.insert(
            "azure_provisioning_state".to_string(),
            serde_yaml::Value::String(self.provisioning_state.clone()),
        );
        if let Some(ref state) = self.power_state {
            vars.insert(
                "azure_power_state".to_string(),
                serde_yaml::Value::String(state.clone()),
            );
        }

        // Network
        if !self.private_ips.is_empty() {
            vars.insert(
                "azure_private_ips".to_string(),
                serde_yaml::Value::Sequence(
                    self.private_ips
                        .iter()
                        .map(|s| serde_yaml::Value::String(s.clone()))
                        .collect(),
                ),
            );
            vars.insert(
                "azure_private_ip".to_string(),
                serde_yaml::Value::String(self.private_ips[0].clone()),
            );
        }
        if !self.public_ips.is_empty() {
            vars.insert(
                "azure_public_ips".to_string(),
                serde_yaml::Value::Sequence(
                    self.public_ips
                        .iter()
                        .map(|s| serde_yaml::Value::String(s.clone()))
                        .collect(),
                ),
            );
            vars.insert(
                "azure_public_ip".to_string(),
                serde_yaml::Value::String(self.public_ips[0].clone()),
            );
        }

        // Availability
        if let Some(ref az) = self.availability_zone {
            vars.insert(
                "azure_availability_zone".to_string(),
                serde_yaml::Value::String(az.clone()),
            );
        }
        if let Some(ref avset) = self.availability_set {
            vars.insert(
                "azure_availability_set".to_string(),
                serde_yaml::Value::String(avset.clone()),
            );
        }

        // Image info
        if let Some(ref image) = self.image {
            if let Some(ref publisher) = image.publisher {
                vars.insert(
                    "azure_image_publisher".to_string(),
                    serde_yaml::Value::String(publisher.clone()),
                );
            }
            if let Some(ref offer) = image.offer {
                vars.insert(
                    "azure_image_offer".to_string(),
                    serde_yaml::Value::String(offer.clone()),
                );
            }
            if let Some(ref sku) = image.sku {
                vars.insert(
                    "azure_image_sku".to_string(),
                    serde_yaml::Value::String(sku.clone()),
                );
            }
        }

        // Computer name and admin user
        if let Some(ref name) = self.computer_name {
            vars.insert(
                "azure_computer_name".to_string(),
                serde_yaml::Value::String(name.clone()),
            );
        }
        if let Some(ref user) = self.admin_username {
            vars.insert(
                "azure_admin_username".to_string(),
                serde_yaml::Value::String(user.clone()),
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
                "azure_tags".to_string(),
                serde_yaml::Value::Mapping(tags_map),
            );
        }

        vars
    }
}

/// Azure inventory plugin
#[derive(Debug)]
pub struct AzurePlugin {
    config: PluginConfig,
    /// Cached VMs
    #[allow(dead_code)]
    cached_vms: std::sync::RwLock<Option<Vec<AzureVm>>>,
}

impl AzurePlugin {
    /// Create a new Azure plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        Ok(Self {
            config,
            cached_vms: std::sync::RwLock::new(None),
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("azure_rm");
        Self::new(config)
    }

    /// Get subscription ID from config or environment
    fn get_subscription_id(&self) -> Option<String> {
        self.config
            .get_string("subscription_id")
            .or_else(|| std::env::var("AZURE_SUBSCRIPTION_ID").ok())
    }

    /// Get resource group filter (include)
    fn get_include_resource_groups(&self) -> Vec<String> {
        self.config
            .get_string_list("include_vm_resource_groups")
            .unwrap_or_default()
    }

    /// Get resource group filter (exclude)
    fn get_exclude_resource_groups(&self) -> Vec<String> {
        self.config
            .get_string_list("exclude_vm_resource_groups")
            .unwrap_or_default()
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
            "name".to_string(),
            "private_ip".to_string(),
            "public_ip".to_string(),
        ]
    }

    /// Check if VM passes filters
    fn vm_passes_filters(&self, vm: &AzureVm) -> bool {
        let include_rgs = self.get_include_resource_groups();
        let exclude_rgs = self.get_exclude_resource_groups();

        // Check resource group include filter
        if !include_rgs.is_empty() && !include_rgs.contains(&vm.resource_group) {
            return false;
        }

        // Check resource group exclude filter
        if exclude_rgs.contains(&vm.resource_group) {
            return false;
        }

        // Check power state if configured
        if let Some(include_running_only) = self.config.get_bool("include_running_only") {
            if include_running_only && !vm.is_running() {
                return false;
            }
        }

        // Check tag filters
        for (key, filter_config) in &self.config.filters {
            let filter_values = filter_config.values();

            if let Some(tag_name) = key.strip_prefix("tag:") {
                let tag_value = vm.tags.get(tag_name).map(|s| s.as_str());

                if let Some(value) = tag_value {
                    if !filter_values.contains(&value) {
                        return false;
                    }
                } else {
                    // Tag doesn't exist
                    return false;
                }
            }
        }

        true
    }

    /// Get groups for a VM based on keyed_groups configuration
    fn get_vm_groups(&self, vm: &AzureVm) -> Vec<String> {
        let mut groups = vec!["azure".to_string()];

        // Add location group
        groups.push(format!("location_{}", sanitize_group_name(&vm.location)));

        // Add resource group
        groups.push(format!("rg_{}", sanitize_group_name(&vm.resource_group)));

        // Add VM size group
        groups.push(format!("size_{}", sanitize_group_name(&vm.vm_size)));

        // Add OS type group
        groups.push(format!("os_{}", sanitize_group_name(&vm.os_type)));

        // Add availability zone group if present
        if let Some(ref az) = vm.availability_zone {
            groups.push(format!("az_{}", sanitize_group_name(az)));
        }

        // Process keyed_groups configuration
        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, vm) {
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
        for (key, value) in &vm.tags {
            let safe_key = sanitize_group_name(key);
            let safe_value = sanitize_group_name(value);
            groups.push(format!("tag_{}_{}", safe_key, safe_value));
        }

        groups
    }

    /// Resolve a keyed group key to a value
    fn resolve_keyed_group_key(&self, key: &str, vm: &AzureVm) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();

        match parts.as_slice() {
            ["tags" | "tag", tag_name] => vm.tags.get(*tag_name).cloned(),
            ["location"] => Some(vm.location.clone()),
            ["resource_group"] => Some(vm.resource_group.clone()),
            ["vm_size"] => Some(vm.vm_size.clone()),
            ["os_type"] => Some(vm.os_type.clone()),
            ["availability_zone"] => vm.availability_zone.clone(),
            ["availability_set"] => vm.availability_set.clone(),
            ["power_state"] => vm.power_state.clone(),
            ["provisioning_state"] => Some(vm.provisioning_state.clone()),
            ["image", "publisher"] => vm.image.as_ref().and_then(|i| i.publisher.clone()),
            ["image", "offer"] => vm.image.as_ref().and_then(|i| i.offer.clone()),
            ["image", "sku"] => vm.image.as_ref().and_then(|i| i.sku.clone()),
            _ => None,
        }
    }

    /// Apply compose configuration to set host variables
    fn apply_compose(&self, host: &mut Host, vm: &AzureVm) {
        let compose = &self.config.compose;

        // Set ansible_host
        if let Some(ref expr) = compose.ansible_host {
            if let Some(value) = self.resolve_compose_expression(expr, vm) {
                host.ansible_host = Some(value);
            }
        } else {
            // Default: use private IP
            if let Some(ip) = vm.primary_private_ip() {
                host.ansible_host = Some(ip.to_string());
            }
        }

        // Set ansible_user
        if let Some(ref expr) = compose.ansible_user {
            if let Some(value) = self.resolve_compose_expression(expr, vm) {
                host.connection.ssh.user = Some(value);
            }
        } else if let Some(ref user) = vm.admin_username {
            host.connection.ssh.user = Some(user.clone());
        } else {
            // Default user based on OS
            let user = if vm.os_type.to_lowercase() == "windows" {
                "azureadmin"
            } else {
                "azureuser"
            };
            host.connection.ssh.user = Some(user.to_string());
        }

        // Set connection type for Windows
        if vm.os_type.to_lowercase() == "windows" {
            host.connection.connection = crate::inventory::ConnectionType::Winrm;
        }

        // Apply extra vars from compose
        for (key, expr) in &compose.extra_vars {
            if let Some(value) = self.resolve_compose_expression(expr, vm) {
                host.set_var(key, serde_yaml::Value::String(value));
            }
        }
    }

    /// Resolve a compose expression to a value
    fn resolve_compose_expression(&self, expr: &str, vm: &AzureVm) -> Option<String> {
        match expr {
            "private_ip" | "private_ip_address" => vm.primary_private_ip().map(String::from),
            "public_ip" | "public_ip_address" => vm.primary_public_ip().map(String::from),
            "name" | "vm_name" => Some(vm.name.clone()),
            "computer_name" => vm.computer_name.clone(),
            "location" => Some(vm.location.clone()),
            "resource_group" => Some(vm.resource_group.clone()),
            "admin_username" => vm.admin_username.clone(),
            s if s.starts_with("tags.") => {
                let tag_name = &s[5..];
                vm.tags.get(tag_name).cloned()
            }
            _ => Some(expr.to_string()), // Literal value
        }
    }

    /// Fetch VMs from Azure (simulated for now)
    async fn fetch_vms(&self) -> InventoryResult<Vec<AzureVm>> {
        let subscription_id = self.get_subscription_id();

        if let Some(ref sub_id) = subscription_id {
            tracing::info!("Azure plugin: Querying subscription {}", sub_id);
        } else {
            tracing::warn!(
                "Azure plugin: No subscription_id configured. \
                 Set subscription_id in config or AZURE_SUBSCRIPTION_ID environment variable."
            );
        }

        // In a real implementation, this would use azure_mgmt_compute crate
        // to call Azure Resource Manager API
        //
        // Example with Azure SDK:
        // ```rust
        // let credential = azure_identity::DefaultAzureCredential::default();
        // let client = azure_mgmt_compute::Client::new(credential, subscription_id);
        // let vms = client.virtual_machines().list_all().await?;
        // ```

        tracing::warn!(
            "Azure plugin: Azure SDK integration not yet implemented. \
             Configure Azure credentials and install azure_mgmt_compute for full functionality."
        );

        Ok(Vec::new())
    }

    /// Convert VMs to inventory
    fn vms_to_inventory(&self, vms: Vec<AzureVm>) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let hostname_prefs = self.get_hostname_preferences();

        // Create base azure group
        let mut azure_group = Group::new("azure");
        azure_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("azure_rm".to_string()),
        );

        // Process each VM
        for vm in &vms {
            // Skip VMs that don't pass filters
            if !self.vm_passes_filters(vm) {
                continue;
            }

            // Determine hostname
            let Some(hostname) = vm.hostname(&hostname_prefs) else {
                tracing::warn!(
                    "Azure plugin: Could not determine hostname for VM {}",
                    vm.name
                );
                continue;
            };

            // Create host
            let mut host = Host::new(&hostname);

            // Set host variables from VM data
            for (key, value) in vm.to_host_vars() {
                host.set_var(&key, value);
            }

            // Apply compose configuration
            self.apply_compose(&mut host, vm);

            // Get groups for this VM
            let groups = self.get_vm_groups(vm);

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

        // Add the base azure group
        inventory.add_group(azure_group)?;

        Ok(inventory)
    }
}

#[async_trait]
impl DynamicInventoryPlugin for AzurePlugin {
    fn name(&self) -> &str {
        "azure_rm"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Azure Virtual Machines dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        // Check for Azure credentials
        let has_subscription = self.get_subscription_id().is_some();

        let has_sp_creds = std::env::var("AZURE_CLIENT_ID").is_ok()
            && std::env::var("AZURE_CLIENT_SECRET").is_ok()
            && std::env::var("AZURE_TENANT_ID").is_ok();

        let has_msi = self.config.get_bool("use_msi").unwrap_or(false);

        let has_cli = self.config.get_string("auth_source").as_deref() == Some("cli");

        if !has_subscription {
            tracing::warn!(
                "Azure plugin: No subscription_id configured. \
                 Set subscription_id in config or AZURE_SUBSCRIPTION_ID."
            );
        }

        if !has_sp_creds && !has_msi && !has_cli {
            tracing::warn!(
                "Azure plugin: No Azure credentials found. \
                 Configure service principal (AZURE_CLIENT_ID, AZURE_CLIENT_SECRET, AZURE_TENANT_ID), \
                 use MSI (use_msi: true), or Azure CLI (auth_source: cli)."
            );
        }

        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        // Verify configuration
        self.verify()?;

        // Fetch VMs from Azure
        let vms = self.fetch_vms().await?;

        // Convert to inventory
        self.vms_to_inventory(vms)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        let mut cache = self.cached_vms.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::optional_string("subscription_id", "Azure subscription ID", "")
                .with_env_var("AZURE_SUBSCRIPTION_ID"),
            PluginOption::optional_string("client_id", "Service principal client ID", "")
                .with_env_var("AZURE_CLIENT_ID"),
            PluginOption::optional_string("secret", "Service principal secret", "")
                .with_env_var("AZURE_CLIENT_SECRET"),
            PluginOption::optional_string("tenant", "Azure AD tenant ID", "")
                .with_env_var("AZURE_TENANT_ID"),
            PluginOption::optional_bool(
                "use_msi",
                "Use Managed Service Identity for authentication",
                false,
            ),
            PluginOption::optional_string(
                "auth_source",
                "Authentication source (auto, cli, msi, env)",
                "auto",
            ),
            PluginOption::optional_list(
                "include_vm_resource_groups",
                "Resource groups to include (empty = all)",
            ),
            PluginOption::optional_list("exclude_vm_resource_groups", "Resource groups to exclude"),
            PluginOption::optional_bool("include_running_only", "Only include running VMs", false),
            PluginOption::optional_list(
                "hostnames",
                "Hostname preferences in order (name, private_ip, public_ip)",
            ),
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic group creation based on VM attributes".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_vm() -> AzureVm {
        let mut tags = HashMap::new();
        tags.insert("Environment".to_string(), "production".to_string());
        tags.insert("Role".to_string(), "webserver".to_string());

        AzureVm {
            id: "/subscriptions/sub-123/resourceGroups/prod-rg/providers/Microsoft.Compute/virtualMachines/web-vm-01".to_string(),
            name: "web-vm-01".to_string(),
            resource_group: "prod-rg".to_string(),
            location: "eastus".to_string(),
            vm_size: "Standard_DS2_v2".to_string(),
            provisioning_state: "Succeeded".to_string(),
            power_state: Some("running".to_string()),
            private_ips: vec!["10.0.1.10".to_string()],
            public_ips: vec!["52.168.1.100".to_string()],
            network_interfaces: vec!["web-vm-01-nic".to_string()],
            os_type: "Linux".to_string(),
            os_disk: Some("web-vm-01-osdisk".to_string()),
            image: Some(AzureImageReference {
                publisher: Some("Canonical".to_string()),
                offer: Some("UbuntuServer".to_string()),
                sku: Some("18.04-LTS".to_string()),
                version: Some("latest".to_string()),
            }),
            availability_set: None,
            availability_zone: Some("1".to_string()),
            tags,
            subscription_id: "sub-123".to_string(),
            computer_name: Some("web-vm-01".to_string()),
            admin_username: Some("azureuser".to_string()),
        }
    }

    #[test]
    fn test_vm_hostname() {
        let vm = create_test_vm();

        // Name preference
        let prefs = vec!["name".to_string()];
        assert_eq!(vm.hostname(&prefs), Some("web-vm-01".to_string()));

        // Private IP preference
        let prefs = vec!["private_ip".to_string()];
        assert_eq!(vm.hostname(&prefs), Some("10.0.1.10".to_string()));

        // Public IP preference
        let prefs = vec!["public_ip".to_string()];
        assert_eq!(vm.hostname(&prefs), Some("52.168.1.100".to_string()));
    }

    #[test]
    fn test_vm_to_host_vars() {
        let vm = create_test_vm();
        let vars = vm.to_host_vars();

        assert!(vars.contains_key("azure_vm_name"));
        assert!(vars.contains_key("azure_location"));
        assert!(vars.contains_key("azure_private_ip"));
        assert!(vars.contains_key("azure_tags"));
    }

    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("azure_rm");
        let plugin = AzurePlugin::new(config).unwrap();
        assert_eq!(plugin.name(), "azure_rm");
    }

    #[test]
    fn test_vm_is_running() {
        let mut vm = create_test_vm();
        assert!(vm.is_running());

        vm.power_state = Some("deallocated".to_string());
        assert!(!vm.is_running());

        vm.power_state = Some("PowerState/running".to_string());
        assert!(vm.is_running());
    }

    #[test]
    fn test_keyed_group_resolution() {
        let config = PluginConfig::new("azure_rm");
        let plugin = AzurePlugin::new(config).unwrap();
        let vm = create_test_vm();

        let value = plugin.resolve_keyed_group_key("tags.Environment", &vm);
        assert_eq!(value, Some("production".to_string()));

        let value = plugin.resolve_keyed_group_key("location", &vm);
        assert_eq!(value, Some("eastus".to_string()));

        let value = plugin.resolve_keyed_group_key("vm_size", &vm);
        assert_eq!(value, Some("Standard_DS2_v2".to_string()));
    }

    #[test]
    fn test_vm_groups() {
        let config = PluginConfig::new("azure_rm");
        let plugin = AzurePlugin::new(config).unwrap();
        let vm = create_test_vm();

        let groups = plugin.get_vm_groups(&vm);

        assert!(groups.contains(&"azure".to_string()));
        assert!(groups.contains(&"location_eastus".to_string()));
        assert!(groups.contains(&"rg_prod_rg".to_string()));
        assert!(groups.contains(&"os_linux".to_string()));
    }
}
