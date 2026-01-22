//! Proxmox inventory plugin for dynamic hosts.

use crate::inventory::{Group, Host, Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use indexmap::IndexMap;
use reqwest::{header, Certificate, Client};
use serde::Deserialize;
use serde_yaml::{Number, Value};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use url::Url;

use super::{
    sanitize_group_name, DynamicInventoryPlugin, FilterConfig, FilterOperator, PluginConfig,
    PluginConfigError, PluginOption, PluginOptionType,
};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Deserialize)]
struct ClusterResourcesResponse {
    data: Vec<ProxmoxResource>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProxmoxResource {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    resource_type: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    vmid: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    maxcpu: Option<f64>,
    #[serde(default)]
    maxmem: Option<u64>,
    #[serde(default)]
    maxdisk: Option<u64>,
    #[serde(default)]
    cpu: Option<f64>,
    #[serde(default)]
    mem: Option<u64>,
    #[serde(default)]
    uptime: Option<u64>,
    #[serde(default)]
    template: Option<u64>,
}

impl ProxmoxResource {
    fn resource_type(&self) -> Option<&str> {
        if !self.resource_type.is_empty() {
            Some(self.resource_type.as_str())
        } else if !self.id.is_empty() {
            self.id.split('/').next()
        } else {
            None
        }
    }

    fn resolved_vmid(&self) -> Option<u64> {
        if let Some(vmid) = self.vmid {
            Some(vmid)
        } else if !self.id.is_empty() {
            self.id
                .split('/')
                .nth(1)
                .and_then(|value| value.parse::<u64>().ok())
        } else {
            None
        }
    }

    fn tags_list(&self) -> Vec<String> {
        self.tags.as_deref().map(parse_tags).unwrap_or_default()
    }

    fn hostname(&self, prefs: &[String]) -> Option<String> {
        let tags = self.tags_list();

        for pref in prefs {
            let value = match pref.as_str() {
                "name" | "hostname" => self.name.clone(),
                "vmid" => self.resolved_vmid().map(|v| v.to_string()),
                "id" => {
                    if self.id.is_empty() {
                        None
                    } else {
                        Some(self.id.clone())
                    }
                }
                "node" => self.node.clone(),
                "status" => self.status.clone(),
                "type" | "resource_type" => self.resource_type().map(|s| s.to_string()),
                s if s.starts_with("tag:") => {
                    let tag = &s[4..];
                    if tags.iter().any(|t| t == tag) {
                        Some(tag.to_string())
                    } else {
                        None
                    }
                }
                s if s.starts_with("tags.") => {
                    let tag = &s[5..];
                    if tags.iter().any(|t| t == tag) {
                        Some(tag.to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(value) = value {
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }

        self.name
            .clone()
            .or_else(|| self.resolved_vmid().map(|v| v.to_string()))
            .or_else(|| {
                if self.id.is_empty() {
                    None
                } else {
                    Some(self.id.clone())
                }
            })
    }

    fn to_host_vars(&self) -> IndexMap<String, Value> {
        let mut vars = IndexMap::new();

        if !self.id.is_empty() {
            vars.insert("proxmox_id".to_string(), Value::String(self.id.clone()));
        }

        if let Some(resource_type) = self.resource_type() {
            vars.insert(
                "proxmox_type".to_string(),
                Value::String(resource_type.to_string()),
            );
        }

        if let Some(ref node) = self.node {
            vars.insert("proxmox_node".to_string(), Value::String(node.clone()));
            vars.insert("node".to_string(), Value::String(node.clone()));
        }

        if let Some(ref status) = self.status {
            vars.insert("proxmox_status".to_string(), Value::String(status.clone()));
            vars.insert("status".to_string(), Value::String(status.clone()));
        }

        if let Some(vmid) = self.resolved_vmid() {
            vars.insert(
                "proxmox_vmid".to_string(),
                Value::Number(Number::from(vmid)),
            );
            vars.insert("vmid".to_string(), Value::Number(Number::from(vmid)));
        }

        if let Some(ref name) = self.name {
            vars.insert("proxmox_name".to_string(), Value::String(name.clone()));
        }

        let tags = self.tags_list();
        if !tags.is_empty() {
            let values = tags.iter().map(|tag| Value::String(tag.clone())).collect();
            vars.insert("proxmox_tags".to_string(), Value::Sequence(values));
        }

        if let Some(maxcpu) = self.maxcpu {
            vars.insert(
                "proxmox_maxcpu".to_string(),
                Value::String(maxcpu.to_string()),
            );
        }
        if let Some(maxmem) = self.maxmem {
            vars.insert(
                "proxmox_maxmem".to_string(),
                Value::Number(Number::from(maxmem)),
            );
        }
        if let Some(maxdisk) = self.maxdisk {
            vars.insert(
                "proxmox_maxdisk".to_string(),
                Value::Number(Number::from(maxdisk)),
            );
        }
        if let Some(cpu) = self.cpu {
            vars.insert("proxmox_cpu".to_string(), Value::String(cpu.to_string()));
        }
        if let Some(mem) = self.mem {
            vars.insert("proxmox_mem".to_string(), Value::Number(Number::from(mem)));
        }
        if let Some(uptime) = self.uptime {
            vars.insert(
                "proxmox_uptime".to_string(),
                Value::Number(Number::from(uptime)),
            );
        }
        if let Some(template) = self.template {
            vars.insert(
                "proxmox_template".to_string(),
                Value::Number(Number::from(template)),
            );
        }

        vars
    }
}

#[derive(Debug, Clone)]
struct CachedResources {
    fetched_at: Instant,
    resources: Vec<ProxmoxResource>,
}

/// Proxmox inventory plugin.
#[derive(Debug)]
pub struct ProxmoxPlugin {
    config: PluginConfig,
    api_base: String,
    token_id: String,
    token_secret: String,
    include_qemu: bool,
    include_lxc: bool,
    client: Client,
    cached_resources: RwLock<Option<CachedResources>>,
}

impl ProxmoxPlugin {
    /// Create a new Proxmox plugin with configuration.
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        let api_url = config
            .get_string("api_url")
            .or_else(|| std::env::var("PROXMOX_API_URL").ok())
            .ok_or_else(|| PluginConfigError::MissingField("api_url".to_string()))?;

        let token_id = config
            .get_string("api_token_id")
            .or_else(|| std::env::var("PROXMOX_API_TOKEN_ID").ok())
            .ok_or_else(|| PluginConfigError::MissingField("api_token_id".to_string()))?;

        let token_secret = config
            .get_string("api_token_secret")
            .or_else(|| std::env::var("PROXMOX_API_TOKEN_SECRET").ok())
            .ok_or_else(|| PluginConfigError::MissingField("api_token_secret".to_string()))?;

        let api_base = normalize_api_base(&api_url)?;

        let validate_certs = config.get_bool("validate_certs").unwrap_or(true);
        let insecure_skip_tls_verify = config.get_bool("insecure_skip_tls_verify").unwrap_or(false);
        let timeout_secs = config
            .get_i64("timeout")
            .unwrap_or(DEFAULT_TIMEOUT_SECS as i64);
        if timeout_secs <= 0 {
            return Err(PluginConfigError::Invalid(
                "timeout must be positive".to_string(),
            ));
        }

        let include_qemu = config.get_bool("include_qemu").unwrap_or(true);
        let include_lxc = config.get_bool("include_lxc").unwrap_or(true);
        if !include_qemu && !include_lxc {
            return Err(PluginConfigError::Invalid(
                "include_qemu and include_lxc cannot both be false".to_string(),
            ));
        }

        let ca_cert_path = config.get_string("ca_cert_path");
        if !validate_certs && !insecure_skip_tls_verify {
            return Err(PluginConfigError::Invalid(
                "validate_certs=false requires insecure_skip_tls_verify=true".to_string(),
            ));
        }

        if !validate_certs {
            tracing::warn!(
                "TLS certificate validation disabled for Proxmox inventory. \
                 Set validate_certs=true for production use."
            );
        }

        let client = build_http_client(
            validate_certs,
            Duration::from_secs(timeout_secs as u64),
            ca_cert_path.as_deref(),
        )?;

        Ok(Self {
            config,
            api_base,
            token_id,
            token_secret,
            include_qemu,
            include_lxc,
            client,
            cached_resources: RwLock::new(None),
        })
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("proxmox");
        Self::new(config)
    }

    fn get_hostname_preferences(&self) -> Vec<String> {
        if !self.config.hostnames.is_empty() {
            return self
                .config
                .hostnames
                .iter()
                .map(|h| h.name().to_string())
                .collect();
        }

        vec![
            "name".to_string(),
            "hostname".to_string(),
            "vmid".to_string(),
            "id".to_string(),
        ]
    }

    fn resource_included(&self, resource: &ProxmoxResource) -> bool {
        match resource.resource_type() {
            Some("qemu") => self.include_qemu,
            Some("lxc") => self.include_lxc,
            _ => false,
        }
    }

    fn resource_passes_filters(&self, resource: &ProxmoxResource) -> bool {
        for (key, filter) in &self.config.filters {
            let matches = match key.as_str() {
                "node" => resource
                    .node
                    .as_deref()
                    .map(|value| filter_matches(filter, value))
                    .unwrap_or(false),
                "status" => resource
                    .status
                    .as_deref()
                    .map(|value| filter_matches(filter, value))
                    .unwrap_or(false),
                "vmid" => resource
                    .resolved_vmid()
                    .map(|value| filter_matches(filter, &value.to_string()))
                    .unwrap_or(false),
                "name" => resource
                    .name
                    .as_deref()
                    .map(|value| filter_matches(filter, value))
                    .unwrap_or(false),
                "type" | "resource_type" => resource
                    .resource_type()
                    .map(|value| filter_matches(filter, value))
                    .unwrap_or(false),
                "id" => {
                    if resource.id.is_empty() {
                        false
                    } else {
                        filter_matches(filter, &resource.id)
                    }
                }
                "tags" | "tag" => {
                    let tags = resource.tags_list();
                    if tags.is_empty() {
                        false
                    } else {
                        filter_matches_list(filter, &tags)
                    }
                }
                _ => true,
            };

            if !matches {
                return false;
            }
        }

        true
    }

    fn get_resource_groups(&self, resource: &ProxmoxResource) -> Vec<String> {
        let mut groups = vec!["proxmox".to_string()];

        if let Some(node) = resource.node.as_deref() {
            groups.push(format!("node_{}", sanitize_group_name(node)));
        }

        if let Some(status) = resource.status.as_deref() {
            groups.push(format!("status_{}", sanitize_group_name(status)));
        }

        if let Some(resource_type) = resource.resource_type() {
            groups.push(format!("type_{}", sanitize_group_name(resource_type)));
        }

        for tag in resource.tags_list() {
            groups.push(format!("tag_{}", sanitize_group_name(&tag)));
        }

        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, resource) {
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

    fn resolve_keyed_group_key(&self, key: &str, resource: &ProxmoxResource) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();

        match parts.as_slice() {
            ["node"] => resource.node.clone(),
            ["status"] => resource.status.clone(),
            ["vmid"] => resource.resolved_vmid().map(|v| v.to_string()),
            ["name"] => resource.name.clone(),
            ["id"] => {
                if resource.id.is_empty() {
                    None
                } else {
                    Some(resource.id.clone())
                }
            }
            ["type"] | ["resource_type"] => resource.resource_type().map(|s| s.to_string()),
            ["tags", tag] | ["tag", tag] => {
                let tags = resource.tags_list();
                if tags.iter().any(|t| t == tag) {
                    Some(tag.to_string())
                } else {
                    None
                }
            }
            ["tags"] => {
                let tags = resource.tags_list();
                if tags.is_empty() {
                    None
                } else {
                    Some(tags.join(","))
                }
            }
            _ => None,
        }
    }

    fn apply_compose(&self, host: &mut Host, resource: &ProxmoxResource) {
        let compose = &self.config.compose;

        if let Some(ref expr) = compose.ansible_host {
            if let Some(value) = self.resolve_compose_expression(expr, resource) {
                host.ansible_host = Some(value);
            }
        }

        if let Some(ref expr) = compose.ansible_port {
            if let Some(value) = self.resolve_compose_expression(expr, resource) {
                if let Ok(port) = value.parse::<u16>() {
                    host.connection.ssh.port = port;
                }
            }
        }

        if let Some(ref expr) = compose.ansible_user {
            if let Some(value) = self.resolve_compose_expression(expr, resource) {
                host.connection.ssh.user = Some(value);
            }
        }

        for (key, expr) in &compose.extra_vars {
            if let Some(value) = self.resolve_compose_expression(expr, resource) {
                host.set_var(key, Value::String(value));
            }
        }
    }

    fn resolve_compose_expression(&self, expr: &str, resource: &ProxmoxResource) -> Option<String> {
        match expr {
            "name" | "hostname" => resource.name.clone(),
            "node" => resource.node.clone(),
            "status" => resource.status.clone(),
            "vmid" => resource.resolved_vmid().map(|v| v.to_string()),
            "id" => {
                if resource.id.is_empty() {
                    None
                } else {
                    Some(resource.id.clone())
                }
            }
            "type" | "resource_type" => resource.resource_type().map(|s| s.to_string()),
            "tags" => {
                let tags = resource.tags_list();
                if tags.is_empty() {
                    None
                } else {
                    Some(tags.join(","))
                }
            }
            s if s.starts_with("tags.") => {
                let tag = &s[5..];
                let tags = resource.tags_list();
                if tags.iter().any(|t| t == tag) {
                    Some(tag.to_string())
                } else {
                    None
                }
            }
            _ => Some(expr.to_string()),
        }
    }

    async fn fetch_resources(&self) -> InventoryResult<Vec<ProxmoxResource>> {
        let ttl = Duration::from_secs(self.config.cache_ttl);
        if ttl > Duration::ZERO {
            let cache = self.cached_resources.read().map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to acquire cache lock: {}",
                    e
                ))
            })?;
            if let Some(cached) = cache.as_ref() {
                if cached.fetched_at.elapsed() < ttl {
                    return Ok(cached.resources.clone());
                }
            }
        }

        let url = format!("{}/cluster/resources", self.api_base);
        let auth = format!("PVEAPIToken={}={}", self.token_id, self.token_secret);

        let response = self
            .client
            .get(&url)
            .query(&[("type", "vm")])
            .header(header::AUTHORIZATION, auth)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status();
        let body = response.text().await.map_err(map_reqwest_error)?;

        if !status.is_success() {
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "Proxmox API error {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                body
            )));
        }

        let response: ClusterResourcesResponse = serde_json::from_str(&body).map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!(
                "Failed to parse Proxmox response: {}",
                e
            ))
        })?;

        let mut resources: Vec<ProxmoxResource> = response
            .data
            .into_iter()
            .filter(|resource| self.resource_included(resource))
            .collect();

        resources.sort_by(|a, b| {
            let left = a.resolved_vmid().unwrap_or(0);
            let right = b.resolved_vmid().unwrap_or(0);
            left.cmp(&right)
        });

        if ttl > Duration::ZERO {
            let mut cache = self.cached_resources.write().map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to acquire cache lock: {}",
                    e
                ))
            })?;
            *cache = Some(CachedResources {
                fetched_at: Instant::now(),
                resources: resources.clone(),
            });
        }

        Ok(resources)
    }

    fn resources_to_inventory(
        &self,
        resources: Vec<ProxmoxResource>,
    ) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let hostname_prefs = self.get_hostname_preferences();

        for resource in &resources {
            if !self.resource_passes_filters(resource) {
                continue;
            }

            let Some(hostname) = resource.hostname(&hostname_prefs) else {
                tracing::warn!(
                    "Proxmox plugin: Could not determine hostname for resource {:?}",
                    resource.id
                );
                continue;
            };

            let mut host = Host::new(&hostname);

            for (key, value) in resource.to_host_vars() {
                host.set_var(&key, value);
            }

            self.apply_compose(&mut host, resource);

            let groups = self.get_resource_groups(resource);
            for group_name in &groups {
                host.add_to_group(group_name.clone());

                if inventory.get_group(group_name).is_none() {
                    inventory.add_group(Group::new(group_name))?;
                }

                if let Some(group) = inventory.get_group_mut(group_name) {
                    group.add_host(hostname.clone());
                }
            }

            inventory.add_host(host)?;
        }

        if let Some(group) = inventory.get_group_mut("proxmox") {
            group.set_var("plugin".to_string(), Value::String("proxmox".to_string()));
        } else {
            let mut proxmox_group = Group::new("proxmox");
            proxmox_group.set_var("plugin".to_string(), Value::String("proxmox".to_string()));
            inventory.add_group(proxmox_group)?;
        }

        Ok(inventory)
    }
}

#[async_trait]
impl DynamicInventoryPlugin for ProxmoxPlugin {
    fn name(&self) -> &str {
        "proxmox"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Proxmox VE dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        if self.api_base.is_empty() {
            return Err(InventoryError::DynamicInventoryFailed(
                "Proxmox API URL is empty".to_string(),
            ));
        }
        if self.token_id.is_empty() || self.token_secret.is_empty() {
            return Err(InventoryError::DynamicInventoryFailed(
                "Proxmox API token is missing".to_string(),
            ));
        }
        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;

        let resources = self.fetch_resources().await?;
        self.resources_to_inventory(resources)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        let mut cache = self.cached_resources.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::required_string("api_url", "Proxmox API base URL")
                .with_env_var("PROXMOX_API_URL"),
            PluginOption::required_string("api_token_id", "Proxmox API token ID")
                .with_env_var("PROXMOX_API_TOKEN_ID"),
            PluginOption::required_string("api_token_secret", "Proxmox API token secret")
                .with_env_var("PROXMOX_API_TOKEN_SECRET"),
            PluginOption::optional_bool(
                "validate_certs",
                "Validate TLS certificates for the API",
                true,
            ),
            PluginOption::optional_bool(
                "insecure_skip_tls_verify",
                "Allow disabling TLS certificate validation (unsafe)",
                false,
            ),
            PluginOption::optional_string(
                "ca_cert_path",
                "Path to custom CA certificate (PEM)",
                "",
            ),
            PluginOption {
                name: "timeout".to_string(),
                description: "API request timeout in seconds".to_string(),
                required: false,
                default: Some(DEFAULT_TIMEOUT_SECS.to_string()),
                option_type: PluginOptionType::Int,
                env_var: None,
            },
            PluginOption::optional_bool("include_qemu", "Include QEMU VMs", true),
            PluginOption::optional_bool("include_lxc", "Include LXC containers", true),
            PluginOption::optional_list("hostnames", "Hostname preferences (name, vmid, id, node)"),
            PluginOption {
                name: "filters".to_string(),
                description: "Filters by node, status, tags, or type".to_string(),
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

fn parse_tags(raw: &str) -> Vec<String> {
    raw.split(|c| c == ';' || c == ',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn filter_matches(filter: &FilterConfig, value: &str) -> bool {
    match filter {
        FilterConfig::Single(item) => value_matches_operator(FilterOperator::Eq, value, item),
        FilterConfig::Multiple(values) => values
            .iter()
            .any(|item| value_matches_operator(FilterOperator::Eq, value, item)),
        FilterConfig::Complex {
            operator: FilterOperator::Ne,
            values,
        } => values
            .iter()
            .all(|item| value_matches_operator(FilterOperator::Ne, value, item)),
        FilterConfig::Complex { operator, values } => values
            .iter()
            .any(|item| value_matches_operator(*operator, value, item)),
    }
}

fn filter_matches_list(filter: &FilterConfig, values: &[String]) -> bool {
    match filter {
        FilterConfig::Complex {
            operator: FilterOperator::Ne,
            values: filter_values,
        } => !values.iter().any(|value| {
            filter_values
                .iter()
                .any(|item| value_matches_operator(FilterOperator::Eq, value, item))
        }),
        _ => values.iter().any(|value| filter_matches(filter, value)),
    }
}

fn value_matches_operator(operator: FilterOperator, value: &str, filter_value: &str) -> bool {
    match operator {
        FilterOperator::Eq => value == filter_value,
        FilterOperator::Ne => value != filter_value,
        FilterOperator::Contains => value.contains(filter_value),
        FilterOperator::StartsWith => value.starts_with(filter_value),
        FilterOperator::EndsWith => value.ends_with(filter_value),
        FilterOperator::Regex => regex::Regex::new(filter_value)
            .map(|re| re.is_match(value))
            .unwrap_or(false),
    }
}

fn normalize_api_base(api_url: &str) -> Result<String, PluginConfigError> {
    let mut url = Url::parse(api_url)
        .map_err(|e| PluginConfigError::Invalid(format!("Invalid api_url '{}': {}", api_url, e)))?;

    let mut path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() || path == "/" {
        path = "/api2/json".to_string();
    } else if !path.ends_with("/api2/json") {
        path = format!("{}/api2/json", path);
    }

    url.set_path(&path);

    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn load_ca_certificate(path: &str) -> Result<Certificate, PluginConfigError> {
    let data = std::fs::read(path)
        .map_err(|e| PluginConfigError::Invalid(format!("Failed to read CA cert: {}", e)))?;
    Certificate::from_pem(&data)
        .map_err(|e| PluginConfigError::Invalid(format!("Invalid CA cert: {}", e)))
}

fn build_http_client(
    validate_certs: bool,
    timeout: Duration,
    ca_cert_path: Option<&str>,
) -> Result<Client, PluginConfigError> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .danger_accept_invalid_certs(!validate_certs);

    if let Some(path) = ca_cert_path {
        if !path.is_empty() {
            let cert = load_ca_certificate(path)?;
            builder = builder.add_root_certificate(cert);
        }
    }

    builder
        .build()
        .map_err(|e| PluginConfigError::Invalid(format!("Failed to build HTTP client: {}", e)))
}

fn map_reqwest_error(error: reqwest::Error) -> InventoryError {
    let message = if error.is_timeout() {
        format!("Proxmox API request timed out: {}", error)
    } else {
        format!("Proxmox API request failed: {}", error)
    };
    InventoryError::DynamicInventoryFailed(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> PluginConfig {
        let mut config = PluginConfig::new("proxmox");
        config.extra.insert(
            "api_url".to_string(),
            serde_yaml::Value::String("https://pve.example:8006".to_string()),
        );
        config.extra.insert(
            "api_token_id".to_string(),
            serde_yaml::Value::String("root@pam!token".to_string()),
        );
        config.extra.insert(
            "api_token_secret".to_string(),
            serde_yaml::Value::String("secret".to_string()),
        );
        config
    }

    fn sample_resource() -> ProxmoxResource {
        ProxmoxResource {
            id: "qemu/100".to_string(),
            resource_type: "qemu".to_string(),
            node: Some("pve1".to_string()),
            status: Some("running".to_string()),
            vmid: Some(100),
            name: Some("web-01".to_string()),
            tags: Some("prod;web".to_string()),
            maxcpu: Some(4.0),
            maxmem: Some(8192),
            maxdisk: None,
            cpu: None,
            mem: None,
            uptime: None,
            template: None,
        }
    }

    #[test]
    fn test_parse_tags() {
        let tags = parse_tags("prod;web,blue");
        assert_eq!(tags, vec!["prod", "web", "blue"]);
    }

    #[test]
    fn test_hostname_preferences() {
        let resource = sample_resource();
        let prefs = vec!["vmid".to_string(), "name".to_string()];
        assert_eq!(resource.hostname(&prefs), Some("100".to_string()));

        let prefs = vec!["name".to_string()];
        assert_eq!(resource.hostname(&prefs), Some("web-01".to_string()));
    }

    #[test]
    fn test_resource_passes_filters() {
        let mut config = base_config();
        config
            .filters
            .insert("node".to_string(), FilterConfig::Single("pve1".to_string()));
        config.filters.insert(
            "status".to_string(),
            FilterConfig::Single("running".to_string()),
        );
        config
            .filters
            .insert("tags".to_string(), FilterConfig::Single("prod".to_string()));

        let plugin = ProxmoxPlugin::new(config).unwrap();
        let resource = sample_resource();
        assert!(plugin.resource_passes_filters(&resource));
    }

    #[test]
    fn test_resource_groups_include_tags() {
        let plugin = ProxmoxPlugin::new(base_config()).unwrap();
        let resource = sample_resource();
        let groups = plugin.get_resource_groups(&resource);
        assert!(groups.contains(&"proxmox".to_string()));
        assert!(groups.contains(&"node_pve1".to_string()));
        assert!(groups.contains(&"status_running".to_string()));
        assert!(groups.contains(&"type_qemu".to_string()));
        assert!(groups.contains(&"tag_prod".to_string()));
        assert!(groups.contains(&"tag_web".to_string()));
    }

    #[test]
    fn test_host_vars_include_core_fields() {
        let resource = sample_resource();
        let vars = resource.to_host_vars();
        assert_eq!(
            vars.get("proxmox_node"),
            Some(&Value::String("pve1".to_string()))
        );
        assert_eq!(
            vars.get("proxmox_status"),
            Some(&Value::String("running".to_string()))
        );
        assert!(vars.contains_key("proxmox_vmid"));
        assert!(vars.contains_key("vmid"));
    }
}
