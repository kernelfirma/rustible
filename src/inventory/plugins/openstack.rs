//! OpenStack Dynamic Inventory Plugin
//!
//! This plugin discovers compute instances from an OpenStack cloud and creates
//! inventory entries with grouping based on metadata, project, availability zone,
//! and server status.
//!
//! # Configuration
//!
//! ```yaml
//! plugin: openstack
//! auth_url: https://keystone.example.com:5000/v3
//! username: admin
//! password: secret
//! project_name: my-project
//! domain_name: Default
//! region: RegionOne
//! keyed_groups:
//!   - key: openstack_az
//!     prefix: az
//!   - key: openstack_status
//!     prefix: status
//! compose:
//!   ansible_host: openstack_access_ip
//! ```
//!
//! # Authentication
//!
//! The plugin supports Keystone v3 token-based authentication. Credentials can be
//! provided via the plugin configuration or environment variables:
//! - `OS_AUTH_URL`
//! - `OS_USERNAME`
//! - `OS_PASSWORD`
//! - `OS_PROJECT_NAME`
//! - `OS_USER_DOMAIN_NAME`
//! - `OS_REGION_NAME`
//!
//! # Features
//!
//! - Keystone v3 authentication
//! - Server discovery via Nova API
//! - Grouping by metadata, project, availability zone, and status
//! - Network address resolution (fixed and floating IPs)
//! - Support for keyed_groups and compose configuration
//!
//! # Feature Gate
//!
//! Full HTTP-based OpenStack API calls are gated behind the `openstack` feature.
//! Without the feature enabled, the plugin will return an error indicating that
//! the OpenStack feature is not compiled in.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use super::{
    sanitize_group_name, DynamicInventoryPlugin, PluginConfig, PluginConfigError, PluginOption,
    PluginOptionType,
};
use crate::inventory::group::Group;
use crate::inventory::host::Host;
use crate::inventory::{Inventory, InventoryError, InventoryResult};

/// Parsed OpenStack server data
#[derive(Debug, Clone)]
pub struct OpenstackServer {
    /// Server UUID
    pub id: String,
    /// Server name
    pub name: String,
    /// Server status (ACTIVE, SHUTOFF, BUILD, ERROR, etc.)
    pub status: String,
    /// Availability zone
    pub availability_zone: String,
    /// Flavor (instance type) name or ID
    pub flavor: String,
    /// Image name or ID
    pub image: String,
    /// Project/tenant ID
    pub project_id: String,
    /// Key pair name
    pub key_name: Option<String>,
    /// Server metadata
    pub metadata: HashMap<String, String>,
    /// Network addresses: network_name -> list of IPs
    pub addresses: HashMap<String, Vec<OpenstackAddress>>,
    /// Security groups
    pub security_groups: Vec<String>,
    /// Created timestamp
    pub created: Option<String>,
}

/// An IP address attached to an OpenStack server
#[derive(Debug, Clone)]
pub struct OpenstackAddress {
    /// IP address
    pub addr: String,
    /// Address type: "fixed" or "floating"
    pub addr_type: String,
    /// IP version (4 or 6)
    pub version: u8,
}

impl OpenstackServer {
    /// Get the best IP address for ansible_host, preferring floating IPs.
    pub fn access_ip(&self) -> Option<String> {
        // First try floating IPs
        for addrs in self.addresses.values() {
            for addr in addrs {
                if addr.addr_type == "floating" && addr.version == 4 {
                    return Some(addr.addr.clone());
                }
            }
        }
        // Fall back to fixed IPv4
        for addrs in self.addresses.values() {
            for addr in addrs {
                if addr.addr_type == "fixed" && addr.version == 4 {
                    return Some(addr.addr.clone());
                }
            }
        }
        None
    }

    /// Get all IPv4 addresses.
    pub fn all_ipv4(&self) -> Vec<String> {
        self.addresses
            .values()
            .flat_map(|addrs| {
                addrs
                    .iter()
                    .filter(|a| a.version == 4)
                    .map(|a| a.addr.clone())
            })
            .collect()
    }
}

/// OpenStack inventory plugin
#[derive(Debug)]
pub struct OpenstackPlugin {
    config: PluginConfig,
    /// Cached server data
    #[allow(dead_code)]
    cached_servers: RwLock<Option<Vec<OpenstackServer>>>,
}

impl OpenstackPlugin {
    /// Create a new OpenStack plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        Ok(Self {
            config,
            cached_servers: RwLock::new(None),
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("openstack");
        Self::new(config)
    }

    /// Get the auth URL from config or environment
    fn auth_url(&self) -> Option<String> {
        self.config
            .get_string("auth_url")
            .or_else(|| std::env::var("OS_AUTH_URL").ok())
    }

    /// Get the username from config or environment
    fn username(&self) -> Option<String> {
        self.config
            .get_string("username")
            .or_else(|| std::env::var("OS_USERNAME").ok())
    }

    /// Get the password from config or environment
    fn password(&self) -> Option<String> {
        self.config
            .get_string("password")
            .or_else(|| std::env::var("OS_PASSWORD").ok())
    }

    /// Get the project name from config or environment
    fn project_name(&self) -> Option<String> {
        self.config
            .get_string("project_name")
            .or_else(|| std::env::var("OS_PROJECT_NAME").ok())
    }

    /// Get the domain name from config or environment
    fn domain_name(&self) -> String {
        self.config
            .get_string("domain_name")
            .or_else(|| std::env::var("OS_USER_DOMAIN_NAME").ok())
            .unwrap_or_else(|| "Default".to_string())
    }

    /// Get the region from config or environment
    fn region(&self) -> Option<String> {
        self.config
            .get_string("region")
            .or_else(|| std::env::var("OS_REGION_NAME").ok())
    }

    /// Authenticate with Keystone v3 and fetch servers via Nova.
    ///
    /// This is gated behind the `openstack` feature flag. Without the feature,
    /// a stub implementation returns an error.
    #[cfg(feature = "openstack")]
    async fn fetch_servers(&self) -> InventoryResult<Vec<OpenstackServer>> {
        let auth_url = self.auth_url().ok_or_else(|| {
            InventoryError::DynamicInventoryFailed("OpenStack auth_url not configured".to_string())
        })?;
        let username = self.username().ok_or_else(|| {
            InventoryError::DynamicInventoryFailed("OpenStack username not configured".to_string())
        })?;
        let password = self.password().ok_or_else(|| {
            InventoryError::DynamicInventoryFailed("OpenStack password not configured".to_string())
        })?;
        let project_name = self.project_name().ok_or_else(|| {
            InventoryError::DynamicInventoryFailed(
                "OpenStack project_name not configured".to_string(),
            )
        })?;
        let domain_name = self.domain_name();

        tracing::info!(
            "OpenStack plugin: Authenticating with Keystone at {}",
            auth_url
        );

        // Build Keystone v3 auth payload
        let auth_body = serde_json::json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {
                        "user": {
                            "name": username,
                            "domain": { "name": &domain_name },
                            "password": password
                        }
                    }
                },
                "scope": {
                    "project": {
                        "name": project_name,
                        "domain": { "name": &domain_name }
                    }
                }
            }
        });

        let client = reqwest::Client::new();
        let token_url = format!("{}/auth/tokens", auth_url.trim_end_matches('/'));

        let auth_resp = client
            .post(&token_url)
            .json(&auth_body)
            .send()
            .await
            .map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Keystone auth request failed: {}",
                    e
                ))
            })?;

        if !auth_resp.status().is_success() {
            let status = auth_resp.status();
            let body = auth_resp.text().await.unwrap_or_default();
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "Keystone auth failed (HTTP {}): {}",
                status, body
            )));
        }

        let token = auth_resp
            .headers()
            .get("X-Subject-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                InventoryError::DynamicInventoryFailed(
                    "Missing X-Subject-Token in Keystone response".to_string(),
                )
            })?
            .to_string();

        // Parse the service catalog from the auth response to find the compute endpoint
        let auth_json: serde_json::Value = auth_resp.json().await.map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to parse Keystone response: {}",
                e
            ))
        })?;

        let compute_url = self.find_compute_endpoint(&auth_json)?;

        tracing::info!("OpenStack plugin: Listing servers from {}", compute_url);

        // List all servers with details
        let servers_url = format!("{}/servers/detail", compute_url.trim_end_matches('/'));

        let servers_resp = client
            .get(&servers_url)
            .header("X-Auth-Token", &token)
            .send()
            .await
            .map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Nova server list request failed: {}",
                    e
                ))
            })?;

        if !servers_resp.status().is_success() {
            let status = servers_resp.status();
            let body = servers_resp.text().await.unwrap_or_default();
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "Nova server list failed (HTTP {}): {}",
                status, body
            )));
        }

        let servers_json: serde_json::Value = servers_resp.json().await.map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to parse Nova response: {}", e))
        })?;

        self.parse_servers_response(&servers_json)
    }

    /// Stub implementation when the openstack feature is not enabled.
    #[cfg(not(feature = "openstack"))]
    async fn fetch_servers(&self) -> InventoryResult<Vec<OpenstackServer>> {
        tracing::warn!(
            "OpenStack plugin: The 'openstack' feature is not enabled. \
             Rebuild with `--features openstack` for full OpenStack API support."
        );

        Err(InventoryError::DynamicInventoryFailed(
            "OpenStack plugin requires the 'openstack' feature to be enabled. \
             Rebuild with `cargo build --features openstack`."
                .to_string(),
        ))
    }

    /// Find the compute (Nova) endpoint in the Keystone service catalog.
    #[cfg(feature = "openstack")]
    fn find_compute_endpoint(&self, auth_json: &serde_json::Value) -> InventoryResult<String> {
        let region = self.region();

        let catalog = auth_json
            .pointer("/token/catalog")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                InventoryError::DynamicInventoryFailed(
                    "No service catalog in Keystone response".to_string(),
                )
            })?;

        for service in catalog {
            let svc_type = service.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if svc_type != "compute" {
                continue;
            }

            let endpoints = service
                .get("endpoints")
                .and_then(|e| e.as_array())
                .unwrap_or(&Vec::new())
                .clone();

            for endpoint in &endpoints {
                let interface = endpoint
                    .get("interface")
                    .and_then(|i| i.as_str())
                    .unwrap_or("");
                if interface != "public" {
                    continue;
                }

                // Check region if specified
                if let Some(ref wanted_region) = region {
                    let ep_region = endpoint
                        .get("region_id")
                        .or_else(|| endpoint.get("region"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");
                    if ep_region != wanted_region.as_str() {
                        continue;
                    }
                }

                if let Some(url) = endpoint.get("url").and_then(|u| u.as_str()) {
                    return Ok(url.to_string());
                }
            }
        }

        Err(InventoryError::DynamicInventoryFailed(
            "Could not find compute (Nova) public endpoint in service catalog".to_string(),
        ))
    }

    /// Parse the Nova server list response into OpenstackServer objects.
    #[cfg(feature = "openstack")]
    fn parse_servers_response(
        &self,
        json: &serde_json::Value,
    ) -> InventoryResult<Vec<OpenstackServer>> {
        let servers_arr = json
            .get("servers")
            .and_then(|s| s.as_array())
            .ok_or_else(|| {
                InventoryError::DynamicInventoryFailed(
                    "Invalid Nova response: missing 'servers' array".to_string(),
                )
            })?;

        let mut servers = Vec::new();

        for server_val in servers_arr {
            let id = server_val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = server_val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = server_val
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN")
                .to_string();
            let availability_zone = server_val
                .get("OS-EXT-AZ:availability_zone")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let flavor = server_val
                .pointer("/flavor/original_name")
                .or_else(|| server_val.pointer("/flavor/id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let image = server_val
                .pointer("/image/id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let project_id = server_val
                .get("tenant_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key_name = server_val
                .get("key_name")
                .and_then(|v| v.as_str())
                .map(String::from);
            let created = server_val
                .get("created")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Parse metadata
            let metadata: HashMap<String, String> = server_val
                .get("metadata")
                .and_then(|m| m.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            // Parse addresses
            let mut addresses: HashMap<String, Vec<OpenstackAddress>> = HashMap::new();
            if let Some(addrs_obj) = server_val.get("addresses").and_then(|a| a.as_object()) {
                for (net_name, net_addrs) in addrs_obj {
                    if let Some(arr) = net_addrs.as_array() {
                        let parsed: Vec<OpenstackAddress> = arr
                            .iter()
                            .filter_map(|a| {
                                let addr = a.get("addr").and_then(|v| v.as_str())?.to_string();
                                let addr_type = a
                                    .get("OS-EXT-IPS:type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("fixed")
                                    .to_string();
                                let version =
                                    a.get("version").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
                                Some(OpenstackAddress {
                                    addr,
                                    addr_type,
                                    version,
                                })
                            })
                            .collect();
                        addresses.insert(net_name.clone(), parsed);
                    }
                }
            }

            // Parse security groups
            let security_groups: Vec<String> = server_val
                .get("security_groups")
                .and_then(|sg| sg.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|sg| sg.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            if !name.is_empty() {
                servers.push(OpenstackServer {
                    id,
                    name,
                    status,
                    availability_zone,
                    flavor,
                    image,
                    project_id,
                    key_name,
                    metadata,
                    addresses,
                    security_groups,
                    created,
                });
            }
        }

        tracing::info!("OpenStack plugin: Discovered {} servers", servers.len());

        // Cache the results
        if let Ok(mut cache) = self.cached_servers.write() {
            *cache = Some(servers.clone());
        }

        Ok(servers)
    }

    /// Convert parsed servers into an Inventory.
    fn servers_to_inventory(&self, servers: Vec<OpenstackServer>) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let mut groups_map: HashMap<String, Group> = HashMap::new();

        // Create the base openstack group
        let mut os_group = Group::new("openstack");
        os_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("openstack".to_string()),
        );

        for server in &servers {
            let mut host = Host::new(&server.name);

            // Set ansible_host from compose or access IP
            if let Some(ref expr) = self.config.compose.ansible_host {
                if let Some(value) = self.resolve_compose_expression(expr, server) {
                    host.ansible_host = Some(value);
                }
            } else if let Some(ip) = server.access_ip() {
                host.ansible_host = Some(ip);
            }

            // Apply compose ansible_user
            if let Some(ref expr) = self.config.compose.ansible_user {
                if let Some(value) = self.resolve_compose_expression(expr, server) {
                    host.connection.ssh.user = Some(value);
                }
            }

            // Apply compose ansible_port
            if let Some(ref expr) = self.config.compose.ansible_port {
                if let Some(value) = self.resolve_compose_expression(expr, server) {
                    if let Ok(port) = value.parse::<u16>() {
                        host.connection.ssh.port = port;
                    }
                }
            }

            // Apply extra vars from compose
            for (key, expr) in &self.config.compose.extra_vars {
                if let Some(value) = self.resolve_compose_expression(expr, server) {
                    host.set_var(key, serde_yaml::Value::String(value));
                }
            }

            // Set openstack-specific host variables
            host.vars.insert(
                "openstack_id".to_string(),
                serde_yaml::Value::String(server.id.clone()),
            );
            host.vars.insert(
                "openstack_name".to_string(),
                serde_yaml::Value::String(server.name.clone()),
            );
            host.vars.insert(
                "openstack_status".to_string(),
                serde_yaml::Value::String(server.status.clone()),
            );
            host.vars.insert(
                "openstack_az".to_string(),
                serde_yaml::Value::String(server.availability_zone.clone()),
            );
            host.vars.insert(
                "openstack_flavor".to_string(),
                serde_yaml::Value::String(server.flavor.clone()),
            );
            host.vars.insert(
                "openstack_image".to_string(),
                serde_yaml::Value::String(server.image.clone()),
            );
            host.vars.insert(
                "openstack_project_id".to_string(),
                serde_yaml::Value::String(server.project_id.clone()),
            );

            if let Some(ref key_name) = server.key_name {
                host.vars.insert(
                    "openstack_key_name".to_string(),
                    serde_yaml::Value::String(key_name.clone()),
                );
            }

            if let Some(ip) = server.access_ip() {
                host.vars.insert(
                    "openstack_access_ip".to_string(),
                    serde_yaml::Value::String(ip),
                );
            }

            // Store all IPs
            let all_ips = server.all_ipv4();
            if !all_ips.is_empty() {
                host.vars.insert(
                    "openstack_ips".to_string(),
                    serde_yaml::Value::Sequence(
                        all_ips
                            .iter()
                            .map(|ip| serde_yaml::Value::String(ip.clone()))
                            .collect(),
                    ),
                );
            }

            // Store metadata as variables
            if !server.metadata.is_empty() {
                let mut meta_map = serde_yaml::Mapping::new();
                for (k, v) in &server.metadata {
                    meta_map.insert(
                        serde_yaml::Value::String(k.clone()),
                        serde_yaml::Value::String(v.clone()),
                    );
                }
                host.vars.insert(
                    "openstack_metadata".to_string(),
                    serde_yaml::Value::Mapping(meta_map),
                );
            }

            if !server.security_groups.is_empty() {
                host.vars.insert(
                    "openstack_security_groups".to_string(),
                    serde_yaml::Value::Sequence(
                        server
                            .security_groups
                            .iter()
                            .map(|sg| serde_yaml::Value::String(sg.clone()))
                            .collect(),
                    ),
                );
            }

            if let Some(ref created) = server.created {
                host.vars.insert(
                    "openstack_created".to_string(),
                    serde_yaml::Value::String(created.clone()),
                );
            }

            // Build group membership
            let group_names = self.get_server_groups(server);
            for group_name in &group_names {
                host.add_to_group(group_name.clone());

                groups_map
                    .entry(group_name.clone())
                    .or_insert_with(|| Group::new(group_name))
                    .add_host(server.name.clone());
            }

            // Add to base openstack group
            host.add_to_group("openstack".to_string());
            os_group.add_host(server.name.clone());

            inventory.add_host(host)?;
        }

        // Add all discovered groups to inventory
        for (_, group) in groups_map {
            inventory.add_group(group)?;
        }
        inventory.add_group(os_group)?;

        Ok(inventory)
    }

    /// Determine the set of groups a server should belong to.
    fn get_server_groups(&self, server: &OpenstackServer) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();

        // Status-based group (e.g., openstack_status_active)
        groups.push(format!(
            "openstack_status_{}",
            sanitize_group_name(&server.status.to_lowercase())
        ));

        // Availability zone group
        if !server.availability_zone.is_empty() {
            groups.push(format!(
                "openstack_az_{}",
                sanitize_group_name(&server.availability_zone)
            ));
        }

        // Project group
        if !server.project_id.is_empty() {
            groups.push(format!(
                "openstack_project_{}",
                sanitize_group_name(&server.project_id)
            ));
        }

        // Flavor group
        if !server.flavor.is_empty() {
            groups.push(format!(
                "openstack_flavor_{}",
                sanitize_group_name(&server.flavor)
            ));
        }

        // Metadata-based groups
        for (key, value) in &server.metadata {
            let safe_key = sanitize_group_name(key);
            let safe_value = sanitize_group_name(value);
            groups.push(format!("openstack_meta_{}_{}", safe_key, safe_value));
        }

        // Security group groups
        for sg in &server.security_groups {
            groups.push(format!("openstack_sg_{}", sanitize_group_name(sg)));
        }

        // Process keyed_groups configuration
        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, server) {
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

        groups
    }

    /// Resolve a keyed group key to a value from server data.
    fn resolve_keyed_group_key(&self, key: &str, server: &OpenstackServer) -> Option<String> {
        match key {
            "openstack_status" | "status" => Some(server.status.clone()),
            "openstack_az" | "availability_zone" => {
                if server.availability_zone.is_empty() {
                    None
                } else {
                    Some(server.availability_zone.clone())
                }
            }
            "openstack_flavor" | "flavor" => {
                if server.flavor.is_empty() {
                    None
                } else {
                    Some(server.flavor.clone())
                }
            }
            "openstack_image" | "image" => {
                if server.image.is_empty() {
                    None
                } else {
                    Some(server.image.clone())
                }
            }
            "openstack_project_id" | "project_id" => {
                if server.project_id.is_empty() {
                    None
                } else {
                    Some(server.project_id.clone())
                }
            }
            "openstack_key_name" | "key_name" => server.key_name.clone(),
            k if k.starts_with("metadata.") => {
                let meta_key = &k[9..];
                server.metadata.get(meta_key).cloned()
            }
            _ => None,
        }
    }

    /// Resolve a compose expression to a value from server data.
    fn resolve_compose_expression(&self, expr: &str, server: &OpenstackServer) -> Option<String> {
        match expr {
            "openstack_access_ip" | "access_ip" => server.access_ip(),
            "openstack_id" | "id" => Some(server.id.clone()),
            "openstack_name" | "name" => Some(server.name.clone()),
            "openstack_status" | "status" => Some(server.status.clone()),
            "openstack_az" | "availability_zone" => Some(server.availability_zone.clone()),
            "openstack_flavor" | "flavor" => Some(server.flavor.clone()),
            "openstack_image" | "image" => Some(server.image.clone()),
            "openstack_project_id" | "project_id" => Some(server.project_id.clone()),
            "openstack_key_name" | "key_name" => server.key_name.clone(),
            k if k.starts_with("metadata.") => {
                let meta_key = &k[9..];
                server.metadata.get(meta_key).cloned()
            }
            _ => Some(expr.to_string()), // Literal value
        }
    }
}

#[async_trait]
impl DynamicInventoryPlugin for OpenstackPlugin {
    fn name(&self) -> &str {
        "openstack"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "OpenStack cloud dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        let has_auth_url = self.auth_url().is_some();
        let has_username = self.username().is_some();
        let has_password = self.password().is_some();
        let has_project = self.project_name().is_some();

        if !has_auth_url {
            return Err(InventoryError::DynamicInventoryFailed(
                "OpenStack plugin: auth_url not configured. \
                 Set 'auth_url' in config or OS_AUTH_URL environment variable."
                    .to_string(),
            ));
        }
        if !has_username {
            return Err(InventoryError::DynamicInventoryFailed(
                "OpenStack plugin: username not configured. \
                 Set 'username' in config or OS_USERNAME environment variable."
                    .to_string(),
            ));
        }
        if !has_password {
            return Err(InventoryError::DynamicInventoryFailed(
                "OpenStack plugin: password not configured. \
                 Set 'password' in config or OS_PASSWORD environment variable."
                    .to_string(),
            ));
        }
        if !has_project {
            return Err(InventoryError::DynamicInventoryFailed(
                "OpenStack plugin: project_name not configured. \
                 Set 'project_name' in config or OS_PROJECT_NAME environment variable."
                    .to_string(),
            ));
        }

        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;

        let servers = self.fetch_servers().await?;
        self.servers_to_inventory(servers)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        let mut cache = self.cached_servers.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::required_string("auth_url", "Keystone v3 authentication URL")
                .with_env_var("OS_AUTH_URL"),
            PluginOption::required_string("username", "OpenStack username")
                .with_env_var("OS_USERNAME"),
            PluginOption::required_string("password", "OpenStack password")
                .with_env_var("OS_PASSWORD"),
            PluginOption::required_string("project_name", "OpenStack project/tenant name")
                .with_env_var("OS_PROJECT_NAME"),
            PluginOption::optional_string("domain_name", "Keystone domain name", "Default")
                .with_env_var("OS_USER_DOMAIN_NAME"),
            PluginOption::optional_string("region", "OpenStack region name", "")
                .with_env_var("OS_REGION_NAME"),
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic group creation based on server attributes".to_string(),
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

    fn create_test_server() -> OpenstackServer {
        let mut metadata = HashMap::new();
        metadata.insert("Environment".to_string(), "production".to_string());
        metadata.insert("Role".to_string(), "webserver".to_string());

        let mut addresses = HashMap::new();
        addresses.insert(
            "private-net".to_string(),
            vec![
                OpenstackAddress {
                    addr: "10.0.0.5".to_string(),
                    addr_type: "fixed".to_string(),
                    version: 4,
                },
                OpenstackAddress {
                    addr: "203.0.113.5".to_string(),
                    addr_type: "floating".to_string(),
                    version: 4,
                },
            ],
        );

        OpenstackServer {
            id: "abc-123-def".to_string(),
            name: "web-server-01".to_string(),
            status: "ACTIVE".to_string(),
            availability_zone: "nova".to_string(),
            flavor: "m1.small".to_string(),
            image: "ubuntu-22.04".to_string(),
            project_id: "proj-456".to_string(),
            key_name: Some("my-keypair".to_string()),
            metadata,
            addresses,
            security_groups: vec!["default".to_string(), "web".to_string()],
            created: Some("2024-06-01T12:00:00Z".to_string()),
        }
    }

    #[test]
    fn test_access_ip_prefers_floating() {
        let server = create_test_server();
        assert_eq!(server.access_ip(), Some("203.0.113.5".to_string()));
    }

    #[test]
    fn test_access_ip_falls_back_to_fixed() {
        let mut server = create_test_server();
        // Remove floating IP
        for addrs in server.addresses.values_mut() {
            addrs.retain(|a| a.addr_type != "floating");
        }
        assert_eq!(server.access_ip(), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_all_ipv4() {
        let server = create_test_server();
        let ips = server.all_ipv4();
        assert_eq!(ips.len(), 2);
        assert!(ips.contains(&"10.0.0.5".to_string()));
        assert!(ips.contains(&"203.0.113.5".to_string()));
    }

    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();
        assert_eq!(plugin.name(), "openstack");
        assert_eq!(plugin.version(), "1.0.0");
    }

    #[test]
    fn test_with_defaults() {
        let plugin = OpenstackPlugin::with_defaults().unwrap();
        assert_eq!(plugin.name(), "openstack");
    }

    #[test]
    fn test_server_groups() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();
        let server = create_test_server();

        let groups = plugin.get_server_groups(&server);

        assert!(groups.contains(&"openstack_status_active".to_string()));
        assert!(groups.contains(&"openstack_az_nova".to_string()));
        assert!(groups.contains(&"openstack_flavor_m1_small".to_string()));
        assert!(groups.contains(&"openstack_sg_default".to_string()));
        assert!(groups.contains(&"openstack_sg_web".to_string()));
    }

    #[test]
    fn test_resolve_keyed_group_key() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();
        let server = create_test_server();

        assert_eq!(
            plugin.resolve_keyed_group_key("openstack_status", &server),
            Some("ACTIVE".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("openstack_az", &server),
            Some("nova".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("metadata.Role", &server),
            Some("webserver".to_string())
        );
    }

    #[test]
    fn test_resolve_compose_expression() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();
        let server = create_test_server();

        assert_eq!(
            plugin.resolve_compose_expression("openstack_access_ip", &server),
            Some("203.0.113.5".to_string())
        );
        assert_eq!(
            plugin.resolve_compose_expression("openstack_name", &server),
            Some("web-server-01".to_string())
        );
    }

    #[test]
    fn test_servers_to_inventory() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();

        let servers = vec![create_test_server()];
        let inventory = plugin.servers_to_inventory(servers).unwrap();

        assert_eq!(inventory.host_count(), 1);
        assert!(inventory.get_host("web-server-01").is_some());

        let host = inventory.get_host("web-server-01").unwrap();
        assert_eq!(
            host.vars.get("openstack_status"),
            Some(&serde_yaml::Value::String("ACTIVE".to_string()))
        );
        assert_eq!(host.ansible_host, Some("203.0.113.5".to_string()));
    }

    #[test]
    fn test_verify_without_credentials() {
        let config = PluginConfig::new("openstack");
        let plugin = OpenstackPlugin::new(config).unwrap();

        // Clear relevant env vars for test isolation
        // The verify function checks config first, then env vars.
        // With an empty config and (likely) no env vars, it should fail.
        let result = plugin.verify();
        // It will fail on auth_url since we did not configure it
        assert!(result.is_err());
    }
}
