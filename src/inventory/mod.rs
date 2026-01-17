//! Inventory management for Rustible.
//!
//! This module provides comprehensive inventory management including:
//! - Loading from YAML, INI, and JSON formats
//! - Dynamic inventory support (executable scripts)
//! - Host pattern matching
//! - Group hierarchy and variable inheritance
//! - Plugin-based inventory sources (AWS EC2, etc.)
//! - Inventory caching for improved performance
//!
//! # Architecture
//!
//! The inventory system consists of several key components:
//!
//! - [`Inventory`]: Main inventory structure holding hosts and groups
//! - [`Host`]: A managed host with connection parameters and variables
//! - [`Group`]: A logical grouping of hosts with shared variables
//! - [`InventoryPlugin`]: Trait for custom inventory sources
//! - [`InventoryCache`]: Caching layer for improved performance
//!
//! # Inventory Formats
//!
//! ## INI Format
//! ```ini
//! [webservers]
//! web1 ansible_host=10.0.0.1
//! web2 ansible_host=10.0.0.2
//!
//! [webservers:vars]
//! http_port=80
//!
//! [production:children]
//! webservers
//! databases
//! ```
//!
//! ## YAML Format
//! ```yaml
//! all:
//!   children:
//!     webservers:
//!       hosts:
//!         web1:
//!           ansible_host: 10.0.0.1
//!         web2:
//!           ansible_host: 10.0.0.2
//!       vars:
//!         http_port: 80
//! ```
//!
//! ## JSON Format (Dynamic Inventory)
//! ```json
//! {
//!   "webservers": {
//!     "hosts": ["web1", "web2"],
//!     "vars": {"http_port": 80}
//!   },
//!   "_meta": {
//!     "hostvars": {
//!       "web1": {"ansible_host": "10.0.0.1"}
//!     }
//!   }
//! }
//! ```
//!
//! # Plugin System
//!
//! The inventory plugin system allows extending inventory sources:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::inventory::plugin::{
//!     InventoryPluginFactory,
//!     InventoryPluginConfig,
//!     InventoryCache,
//! };
//! use std::time::Duration;
//!
//! // Create an AWS EC2 plugin with caching
//! let config = InventoryPluginConfig::new()
//!     .with_option("region", "us-east-1")
//!     .with_cache_ttl(Duration::from_secs(300));
//!
//! let plugin = InventoryPluginFactory::create("aws_ec2", config)?;
//! let inventory = plugin.parse().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Pattern Matching
//!
//! The inventory supports powerful pattern matching:
//!
//! - `all` - All hosts
//! - `groupname` - All hosts in a group
//! - `host1:host2` - Multiple hosts/groups (union)
//! - `group1:&group2` - Intersection
//! - `group1:!group2` - Exclusion
//! - `~regex` - Regex match on hostname
//! - `web*` - Wildcard match

pub mod cache;
pub mod constructed;
pub mod group;
pub mod host;
pub mod plugin;
pub mod plugins;

pub use group::{Group, GroupBuilder, GroupHierarchy};
pub use host::{ConnectionParams, ConnectionType, Host, HostParseError, SshParams};
pub use plugin::{
    inventory_to_json, parse_json_inventory, parse_json_inventory_from_value,
    AwsEc2InventoryPlugin, CacheStats, CachedInventoryPlugin, FileInventoryPlugin, InventoryCache,
    InventoryPlugin, InventoryPluginConfig, InventoryPluginFactory, InventoryPluginRegistry,
    KeyedGroup, PluginError, PluginErrorKind, PluginInfo, PluginOptionInfo, PluginResult,
    PluginType, ScriptInventoryPlugin,
};

// Re-export enhanced cache types
pub use cache::{
    CacheEntryInfo, CacheStatsSnapshot, FileDependency, InventoryCache as EnhancedInventoryCache,
    InventoryCacheConfig, InventoryCacheEntry, InventoryCacheMetrics,
};

// Re-export constructed inventory plugin types
pub use constructed::{
    ConstructedConfig, ConstructedConfigBuilder, ConstructedError, ConstructedPlugin,
    ExpressionEvaluator,
};

// Re-export dynamic inventory plugin types
pub use plugins::{
    create_plugin_from_config, create_plugin_from_file, sanitize_group_name, AwsEc2Plugin,
    AzurePlugin, CacheConfig, ComposeConfig, DynamicInventoryPlugin, DynamicPluginRegistry,
    FilterConfig, FilterOperator, GcpPlugin, GroupByRule, HostnameConfig, KeyedGroupConfig,
    LocalBackend, PluginConfig, PluginConfigBuilder, PluginConfigError, PluginConfigResult,
    PluginOption, PluginOptionType, ResourceMapping, TerraformBackendType,
    TerraformInventoryPlugin, TerraformPlugin, TerraformPluginConfig, TerraformStateBackend,
};

use indexmap::IndexMap;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use thiserror::Error;

/// Errors that can occur during inventory operations
#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("host not found: {0}")]
    HostNotFound(String),

    #[error("group not found: {0}")]
    GroupNotFound(String),

    #[error("invalid host pattern: {0}")]
    InvalidPattern(String),

    #[error("duplicate host: {0}")]
    DuplicateHost(String),

    #[error("duplicate group: {0}")]
    DuplicateGroup(String),

    #[error("circular group dependency detected: {0}")]
    CircularDependency(String),

    #[error("dynamic inventory script failed: {0}")]
    DynamicInventoryFailed(String),

    #[error("invalid INI format: {0}")]
    InvalidIniFormat(String),

    #[error("host parse error: {0}")]
    HostParse(#[from] HostParseError),
}

/// Result type for inventory operations
pub type InventoryResult<T> = Result<T, InventoryError>;

/// The main inventory structure holding all hosts and groups
#[derive(Debug, Clone)]
pub struct Inventory {
    /// All hosts indexed by name
    hosts: HashMap<String, Host>,

    /// All groups indexed by name
    groups: HashMap<String, Group>,

    /// Source file/directory path
    source: Option<String>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    /// Create a new empty inventory with default groups
    pub fn new() -> Self {
        let mut inventory = Self {
            hosts: HashMap::new(),
            groups: HashMap::new(),
            source: None,
        };

        // Create default groups
        inventory.groups.insert("all".to_string(), Group::all());
        inventory
            .groups
            .insert("ungrouped".to_string(), Group::ungrouped());

        inventory
    }

    /// Load inventory from a file or directory
    pub fn load<P: AsRef<Path>>(path: P) -> InventoryResult<Self> {
        let path = path.as_ref();
        let mut inventory = Self::new();
        inventory.source = Some(path.display().to_string());

        if path.is_file() {
            inventory.load_file(path)?;
        } else if path.is_dir() {
            inventory.load_directory(path)?;
        } else {
            return Err(InventoryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path not found: {}", path.display()),
            )));
        }

        // Finalize parent-child relationships
        inventory.compute_group_parents();

        Ok(inventory)
    }

    /// Load a single inventory file
    fn load_file(&mut self, path: &Path) -> InventoryResult<()> {
        // Check if it's an executable (dynamic inventory)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = path.metadata() {
                if metadata.permissions().mode() & 0o111 != 0 {
                    return self.load_dynamic(path);
                }
            }
        }

        let content = std::fs::read_to_string(path)?;
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match extension.to_lowercase().as_str() {
            "yml" | "yaml" => self.parse_yaml(&content)?,
            "json" => self.parse_json(&content)?,
            "ini" => {
                // Explicitly INI extension - parse as INI
                self.parse_ini(&content)?;
            }
            _ => {
                // Try to detect format from content
                let trimmed = content.trim();
                // Skip comment lines for detection
                let first_non_comment: &str = trimmed
                    .lines()
                    .find(|line| {
                        let t = line.trim();
                        !t.is_empty() && !t.starts_with('#')
                    })
                    .unwrap_or(trimmed);

                if trimmed.starts_with('{') {
                    // Starts with '{' - likely JSON
                    self.parse_json(&content)?;
                } else if first_non_comment.starts_with('[') {
                    // Check if it looks like INI section header [group] or JSON array
                    if first_non_comment.ends_with(']') && !first_non_comment.contains('{') {
                        // INI section like [webservers]
                        self.parse_ini(&content)?;
                    } else {
                        // JSON array
                        self.parse_json(&content)?;
                    }
                } else if first_non_comment.contains(':') && !first_non_comment.contains('=') {
                    // Looks like YAML (has colons but no INI-style equals)
                    self.parse_yaml(&content)?;
                } else if first_non_comment.contains('=') {
                    // INI-style key=value
                    self.parse_ini(&content)?;
                } else {
                    // Default to INI
                    self.parse_ini(&content)?;
                }
            }
        }

        Ok(())
    }

    /// Load inventory from a directory
    fn load_directory(&mut self, path: &Path) -> InventoryResult<()> {
        // Look for hosts file
        for name in ["hosts", "hosts.yml", "hosts.yaml", "hosts.ini"] {
            let hosts_file = path.join(name);
            if hosts_file.exists() {
                self.load_file(&hosts_file)?;
                break;
            }
        }

        // Load group_vars directory
        let group_vars = path.join("group_vars");
        if group_vars.is_dir() {
            self.load_group_vars(&group_vars)?;
        }

        // Load host_vars directory
        let host_vars = path.join("host_vars");
        if host_vars.is_dir() {
            self.load_host_vars(&host_vars)?;
        }

        Ok(())
    }

    /// Load group variables from group_vars directory
    fn load_group_vars(&mut self, path: &Path) -> InventoryResult<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();

            if file_path.is_file() {
                let group_name = file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let content = std::fs::read_to_string(&file_path)?;
                let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)?;

                if let Some(group) = self.groups.get_mut(&group_name) {
                    group.merge_vars(&vars);
                } else {
                    // Create the group if it doesn't exist
                    let mut group = Group::new(&group_name);
                    group.merge_vars(&vars);
                    self.groups.insert(group_name, group);
                }
            } else if file_path.is_dir() {
                // Handle directory-based group vars (group_name/vars.yml)
                let group_name = file_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let vars = self.load_vars_from_directory(&file_path)?;
                if let Some(group) = self.groups.get_mut(&group_name) {
                    group.merge_vars(&vars);
                }
            }
        }

        Ok(())
    }

    /// Load host variables from host_vars directory
    fn load_host_vars(&mut self, path: &Path) -> InventoryResult<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();

            if file_path.is_file() {
                let host_name = file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let content = std::fs::read_to_string(&file_path)?;
                let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)?;

                if let Some(host) = self.hosts.get_mut(&host_name) {
                    host.merge_vars(&vars);
                }
            } else if file_path.is_dir() {
                let host_name = file_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                let vars = self.load_vars_from_directory(&file_path)?;
                if let Some(host) = self.hosts.get_mut(&host_name) {
                    host.merge_vars(&vars);
                }
            }
        }

        Ok(())
    }

    /// Load variables from a directory (multiple files merged)
    fn load_vars_from_directory(
        &self,
        path: &Path,
    ) -> InventoryResult<IndexMap<String, serde_yaml::Value>> {
        let mut merged_vars = IndexMap::new();

        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let file_path = entry.path();
            if file_path.is_file() {
                let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "yml" || ext == "yaml" {
                    let content = std::fs::read_to_string(&file_path)?;
                    let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)?;
                    merged_vars.extend(vars);
                }
            }
        }

        Ok(merged_vars)
    }

    /// Load dynamic inventory from an executable script
    fn load_dynamic(&mut self, path: &Path) -> InventoryResult<()> {
        let output = Command::new(path)
            .arg("--list")
            .output()
            .map_err(|e| InventoryError::DynamicInventoryFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(InventoryError::DynamicInventoryFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let json_output = String::from_utf8_lossy(&output.stdout);
        self.parse_json(&json_output)?;

        Ok(())
    }

    /// Parse YAML inventory format
    fn parse_yaml(&mut self, content: &str) -> InventoryResult<()> {
        let data: serde_yaml::Value = serde_yaml::from_str(content)?;

        if let serde_yaml::Value::Mapping(map) = data {
            // Check if this is an "all" wrapper
            if let Some(all) = map.get(&serde_yaml::Value::String("all".to_string())) {
                self.parse_yaml_group("all", all)?;
            } else {
                // Parse as flat structure
                for (key, value) in map {
                    if let serde_yaml::Value::String(group_name) = key {
                        self.parse_yaml_group(&group_name, &value)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse a YAML group definition
    fn parse_yaml_group(&mut self, name: &str, value: &serde_yaml::Value) -> InventoryResult<()> {
        let _group = self
            .groups
            .entry(name.to_string())
            .or_insert_with(|| Group::new(name));

        if let serde_yaml::Value::Mapping(map) = value {
            // Parse hosts
            if let Some(hosts) = map.get(&serde_yaml::Value::String("hosts".to_string())) {
                if let serde_yaml::Value::Mapping(hosts_map) = hosts {
                    for (host_key, host_value) in hosts_map {
                        if let serde_yaml::Value::String(host_name) = host_key {
                            // Check if host already exists
                            let host_exists = self.hosts.contains_key(host_name);

                            if host_exists {
                                // Host exists - just add it to this group and merge vars
                                if let Some(existing_host) = self.hosts.get_mut(host_name) {
                                    existing_host.add_to_group(name.to_string());

                                    // Parse and merge host variables
                                    if let serde_yaml::Value::Mapping(host_vars) = host_value {
                                        for (var_key, var_value) in host_vars {
                                            if let serde_yaml::Value::String(key) = var_key {
                                                Self::apply_host_var_static(
                                                    existing_host,
                                                    key,
                                                    var_value.clone(),
                                                );
                                            }
                                        }
                                    }
                                }

                                // Add host to group
                                if let Some(g) = self.groups.get_mut(name) {
                                    g.add_host(host_name.clone());
                                }
                            } else {
                                // New host - create it
                                let mut host = Host::new(host_name.clone());

                                // Parse host variables
                                if let serde_yaml::Value::Mapping(host_vars) = host_value {
                                    for (var_key, var_value) in host_vars {
                                        if let serde_yaml::Value::String(key) = var_key {
                                            Self::apply_host_var_static(
                                                &mut host,
                                                key,
                                                var_value.clone(),
                                            );
                                        }
                                    }
                                }

                                host.add_to_group(name.to_string());

                                // Get mutable reference to group and add host
                                if let Some(g) = self.groups.get_mut(name) {
                                    g.add_host(host_name.clone());
                                }

                                // Add to all group
                                if name != "all" {
                                    host.add_to_group("all".to_string());
                                    if let Some(all_group) = self.groups.get_mut("all") {
                                        all_group.add_host(host_name.clone());
                                    }
                                }

                                self.hosts.insert(host_name.clone(), host);
                            }
                        }
                    }
                }
            }

            // Parse children
            if let Some(children) = map.get(&serde_yaml::Value::String("children".to_string())) {
                if let serde_yaml::Value::Mapping(children_map) = children {
                    for (child_key, child_value) in children_map {
                        if let serde_yaml::Value::String(child_name) = child_key {
                            // Get mutable reference to group and add child
                            if let Some(g) = self.groups.get_mut(name) {
                                g.add_child(child_name.clone());
                            }
                            self.parse_yaml_group(child_name, child_value)?;
                        }
                    }
                }
            }

            // Parse vars
            if let Some(vars) = map.get(&serde_yaml::Value::String("vars".to_string())) {
                if let serde_yaml::Value::Mapping(vars_map) = vars {
                    for (var_key, var_value) in vars_map {
                        if let serde_yaml::Value::String(key) = var_key {
                            if let Some(g) = self.groups.get_mut(name) {
                                g.set_var(key.clone(), var_value.clone());
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Apply a host variable from YAML
    #[allow(dead_code)]
    fn apply_host_var(&self, host: &mut Host, key: &str, value: serde_yaml::Value) {
        match key {
            "ansible_host" => {
                if let serde_yaml::Value::String(s) = value {
                    host.ansible_host = Some(s);
                }
            }
            "ansible_port" => {
                if let serde_yaml::Value::Number(n) = value {
                    if let Some(port) = n.as_u64() {
                        host.connection.ssh.port = port as u16;
                    }
                }
            }
            "ansible_user" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.ssh.user = Some(s);
                }
            }
            "ansible_ssh_private_key_file" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.ssh.private_key_file = Some(s);
                }
            }
            "ansible_connection" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.connection = match s.as_str() {
                        "local" => ConnectionType::Local,
                        "docker" => ConnectionType::Docker,
                        "podman" => ConnectionType::Podman,
                        "winrm" => ConnectionType::Winrm,
                        _ => ConnectionType::Ssh,
                    };
                }
            }
            "ansible_become" => {
                host.connection.r#become = match value {
                    serde_yaml::Value::Bool(b) => b,
                    serde_yaml::Value::String(s) => s.to_lowercase() == "true" || s == "1",
                    _ => false,
                };
            }
            "ansible_become_method" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.become_method = s;
                }
            }
            "ansible_become_user" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.become_user = s;
                }
            }
            "ansible_python_interpreter" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.python_interpreter = Some(s);
                }
            }
            _ => {
                host.set_var(key, value);
            }
        }
    }

    /// Parse JSON inventory format (compatible with Ansible dynamic inventory)
    fn parse_json(&mut self, content: &str) -> InventoryResult<()> {
        let data: serde_json::Value = serde_json::from_str(content)?;

        if let serde_json::Value::Object(map) = data {
            // Collect hosts to add to "all" group
            let mut all_hosts: Vec<String> = Vec::new();

            // First pass: create groups
            for (key, _value) in &map {
                if key == "_meta" {
                    continue;
                }
                self.groups
                    .entry(key.clone())
                    .or_insert_with(|| Group::new(key));
            }

            // Second pass: populate groups and hosts
            for (key, value) in &map {
                if key == "_meta" {
                    continue;
                }

                // Collect data to add
                let mut hosts_to_add: Vec<String> = Vec::new();
                let mut children_to_add: Vec<String> = Vec::new();
                let mut vars_to_add: Vec<(String, serde_yaml::Value)> = Vec::new();

                if let serde_json::Value::Object(group_data) = value {
                    if let Some(serde_json::Value::Array(hosts)) = group_data.get("hosts") {
                        for host_value in hosts {
                            if let serde_json::Value::String(host_name) = host_value {
                                hosts_to_add.push(host_name.clone());
                            }
                        }
                    }

                    if let Some(serde_json::Value::Array(children)) = group_data.get("children") {
                        for child_value in children {
                            if let serde_json::Value::String(child_name) = child_value {
                                children_to_add.push(child_name.clone());
                            }
                        }
                    }

                    if let Some(serde_json::Value::Object(vars)) = group_data.get("vars") {
                        for (var_key, var_value) in vars {
                            let yaml_value = json_to_yaml(var_value);
                            vars_to_add.push((var_key.clone(), yaml_value));
                        }
                    }
                } else if let serde_json::Value::Array(hosts) = value {
                    for host_value in hosts {
                        if let serde_json::Value::String(host_name) = host_value {
                            hosts_to_add.push(host_name.clone());
                        }
                    }
                }

                // Now apply changes to group
                if let Some(group) = self.groups.get_mut(key) {
                    for host_name in &hosts_to_add {
                        group.add_host(host_name.clone());
                    }
                    for child_name in children_to_add {
                        group.add_child(child_name);
                    }
                    for (var_key, var_value) in vars_to_add {
                        group.set_var(var_key, var_value);
                    }
                }

                // Add hosts to inventory
                for host_name in &hosts_to_add {
                    all_hosts.push(host_name.clone());
                    if !self.hosts.contains_key(host_name) {
                        let mut host = Host::new(host_name.clone());
                        host.add_to_group(key.clone());
                        host.add_to_group("all".to_string());
                        self.hosts.insert(host_name.clone(), host);
                    } else if let Some(h) = self.hosts.get_mut(host_name) {
                        h.add_to_group(key.clone());
                    }
                }
            }

            // Add all hosts to the "all" group
            if let Some(all_group) = self.groups.get_mut("all") {
                for host_name in all_hosts {
                    all_group.add_host(host_name);
                }
            }

            // Second pass: apply host variables from _meta
            if let Some(serde_json::Value::Object(meta)) = map.get("_meta") {
                if let Some(serde_json::Value::Object(hostvars)) = meta.get("hostvars") {
                    for (host_name, vars) in hostvars {
                        if let serde_json::Value::Object(vars_map) = vars {
                            // Collect the vars first
                            let yaml_vars: Vec<(String, serde_yaml::Value)> = vars_map
                                .iter()
                                .map(|(k, v)| (k.clone(), json_to_yaml(v)))
                                .collect();

                            // Then apply them
                            if let Some(host) = self.hosts.get_mut(host_name) {
                                for (var_key, yaml_value) in yaml_vars {
                                    Self::apply_host_var_static(host, &var_key, yaml_value);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Apply a host variable from YAML (static version to avoid borrow issues)
    fn apply_host_var_static(host: &mut Host, key: &str, value: serde_yaml::Value) {
        match key {
            "ansible_host" => {
                if let serde_yaml::Value::String(s) = value {
                    host.ansible_host = Some(s);
                }
            }
            "ansible_port" => {
                if let serde_yaml::Value::Number(n) = value {
                    if let Some(port) = n.as_u64() {
                        host.connection.ssh.port = port as u16;
                    }
                }
            }
            "ansible_user" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.ssh.user = Some(s);
                }
            }
            "ansible_ssh_private_key_file" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.ssh.private_key_file = Some(s);
                }
            }
            "ansible_connection" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.connection = match s.as_str() {
                        "local" => ConnectionType::Local,
                        "docker" => ConnectionType::Docker,
                        "podman" => ConnectionType::Podman,
                        "winrm" => ConnectionType::Winrm,
                        _ => ConnectionType::Ssh,
                    };
                }
            }
            "ansible_become" => {
                host.connection.r#become = match value {
                    serde_yaml::Value::Bool(b) => b,
                    serde_yaml::Value::String(s) => s.to_lowercase() == "true" || s == "1",
                    _ => false,
                };
            }
            "ansible_become_method" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.become_method = s;
                }
            }
            "ansible_become_user" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.become_user = s;
                }
            }
            "ansible_python_interpreter" => {
                if let serde_yaml::Value::String(s) = value {
                    host.connection.python_interpreter = Some(s);
                }
            }
            _ => {
                host.set_var(key, value);
            }
        }
    }

    /// Parse INI inventory format
    fn parse_ini(&mut self, content: &str) -> InventoryResult<()> {
        let mut current_group = "ungrouped".to_string();
        let mut is_vars_section = false;
        let mut is_children_section = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Check for section header
            if line.starts_with('[') && line.ends_with(']') {
                let section = &line[1..line.len() - 1];

                if let Some((group_name, suffix)) = section.rsplit_once(':') {
                    current_group = group_name.to_string();
                    is_vars_section = suffix == "vars";
                    is_children_section = suffix == "children";
                } else {
                    current_group = section.to_string();
                    is_vars_section = false;
                    is_children_section = false;
                }

                // Create group if it doesn't exist
                self.groups
                    .entry(current_group.clone())
                    .or_insert_with(|| Group::new(&current_group));

                continue;
            }

            if is_vars_section {
                // Parse group variable
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = parse_ini_value(value.trim());

                    if let Some(group) = self.groups.get_mut(&current_group) {
                        group.set_var(key, value);
                    }
                }
            } else if is_children_section {
                // Add child group
                if let Some(group) = self.groups.get_mut(&current_group) {
                    group.add_child(line.to_string());
                }

                // Create child group if it doesn't exist
                self.groups
                    .entry(line.to_string())
                    .or_insert_with(|| Group::new(line));
            } else {
                // Parse host definition
                let host = Host::parse(line)?;
                let host_name = host.name.clone();

                // Add to current group
                if let Some(group) = self.groups.get_mut(&current_group) {
                    group.add_host(host_name.clone());
                }

                // Add to all group
                if current_group != "all" {
                    if let Some(all_group) = self.groups.get_mut("all") {
                        all_group.add_host(host_name.clone());
                    }
                }

                // Update or insert host
                if let Some(existing) = self.hosts.get_mut(&host_name) {
                    existing.add_to_group(current_group.clone());
                    existing.merge_vars(&host.vars);
                } else {
                    let mut new_host = host;
                    new_host.add_to_group(current_group.clone());
                    new_host.add_to_group("all".to_string());
                    self.hosts.insert(host_name, new_host);
                }
            }
        }

        Ok(())
    }

    /// Compute parent group relationships from children
    fn compute_group_parents(&mut self) {
        let children_map: HashMap<String, Vec<String>> = self
            .groups
            .iter()
            .map(|(name, group)| (name.clone(), group.children.iter().cloned().collect()))
            .collect();

        for (parent_name, children) in children_map {
            for child_name in children {
                if let Some(child) = self.groups.get_mut(&child_name) {
                    child.add_parent(parent_name.clone());
                }
            }
        }
    }

    /// Add a host to the inventory
    pub fn add_host(&mut self, host: Host) -> InventoryResult<()> {
        let name = host.name.clone();

        // Add to all group
        if let Some(all_group) = self.groups.get_mut("all") {
            all_group.add_host(name.clone());
        }

        // If host has no groups, add to ungrouped
        if host.groups.is_empty() || (host.groups.len() == 1 && host.in_group("all")) {
            if let Some(ungrouped) = self.groups.get_mut("ungrouped") {
                ungrouped.add_host(name.clone());
            }
        }

        self.hosts.insert(name, host);
        Ok(())
    }

    /// Add a group to the inventory
    pub fn add_group(&mut self, group: Group) -> InventoryResult<()> {
        let name = group.name.clone();
        self.groups.insert(name, group);
        self.compute_group_parents();
        Ok(())
    }

    /// Get a host by name
    pub fn get_host(&self, name: &str) -> Option<&Host> {
        self.hosts.get(name)
    }

    /// Get a mutable reference to a host by name
    pub fn get_host_mut(&mut self, name: &str) -> Option<&mut Host> {
        self.hosts.get_mut(name)
    }

    /// Get a group by name
    pub fn get_group(&self, name: &str) -> Option<&Group> {
        self.groups.get(name)
    }

    /// Get a mutable reference to a group by name
    pub fn get_group_mut(&mut self, name: &str) -> Option<&mut Group> {
        self.groups.get_mut(name)
    }

    /// Get all hosts
    pub fn hosts(&self) -> impl Iterator<Item = &Host> {
        self.hosts.values()
    }

    /// Get all hosts as a vector
    pub fn get_all_hosts(&self) -> Vec<&Host> {
        self.hosts.values().collect()
    }

    /// Get all groups
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    /// Get all host names
    pub fn host_names(&self) -> impl Iterator<Item = &String> {
        self.hosts.keys()
    }

    /// Get all group names
    pub fn group_names(&self) -> impl Iterator<Item = &String> {
        self.groups.keys()
    }

    /// Get hosts matching a pattern
    ///
    /// Supported patterns:
    /// - `all` - all hosts
    /// - `hostname` - specific host
    /// - `groupname` - all hosts in group
    /// - `host1:host2` - multiple hosts/groups (union)
    /// - `group1:&group2` - intersection
    /// - `group1:!group2` - exclusion
    /// - `~regex` - regex match on hostname
    /// - `*` - wildcard match
    pub fn get_hosts_for_pattern(&self, pattern: &str) -> InventoryResult<Vec<&Host>> {
        let pattern = pattern.trim();

        if pattern.is_empty() {
            return Ok(Vec::new());
        }

        // Handle "all"
        if pattern == "all" || pattern == "*" {
            return Ok(self.hosts.values().collect());
        }

        // Handle regex pattern
        if pattern.starts_with('~') {
            let regex_str = &pattern[1..];
            let regex = Regex::new(regex_str)
                .map_err(|_| InventoryError::InvalidPattern(pattern.to_string()))?;

            return Ok(self
                .hosts
                .values()
                .filter(|h| regex.is_match(&h.name))
                .collect());
        }

        // Handle complex patterns with operators
        if pattern.contains(':') {
            let parts = split_pattern(pattern);
            if parts.len() > 1 {
                return self.parse_complex_pattern(pattern);
            }
        }

        // Handle glob/wildcard pattern
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            let regex_pattern = glob_to_regex(pattern);
            let regex = Regex::new(&regex_pattern)
                .map_err(|_| InventoryError::InvalidPattern(pattern.to_string()))?;

            return Ok(self
                .hosts
                .values()
                .filter(|h| regex.is_match(&h.name))
                .collect());
        }

        // Try as group name first
        if let Some(group) = self.groups.get(pattern) {
            return Ok(self.get_hosts_in_group_recursive(group));
        }

        // Try as host name
        if let Some(host) = self.hosts.get(pattern) {
            return Ok(vec![host]);
        }

        // Pattern didn't match anything
        Err(InventoryError::InvalidPattern(format!(
            "No hosts matched pattern: {}",
            pattern
        )))
    }

    /// Parse a complex pattern with operators
    fn parse_complex_pattern(&self, pattern: &str) -> InventoryResult<Vec<&Host>> {
        let mut result: HashSet<&str> = HashSet::new();
        let mut first = true;

        // Split by : but not inside brackets
        let parts = split_pattern(pattern);

        for part in parts {
            let part = part.trim();

            if part.is_empty() {
                continue;
            }

            if part.starts_with('&') {
                // Intersection
                let sub_pattern = &part[1..];
                let sub_hosts = self.get_hosts_for_pattern(sub_pattern)?;
                let sub_set: HashSet<&str> = sub_hosts.iter().map(|h| h.name.as_str()).collect();
                result = result.intersection(&sub_set).cloned().collect();
            } else if part.starts_with('!') {
                // Exclusion
                let sub_pattern = &part[1..];
                let sub_hosts = self.get_hosts_for_pattern(sub_pattern)?;
                for host in sub_hosts {
                    result.remove(host.name.as_str());
                }
            } else {
                // Union
                let sub_hosts = self.get_hosts_for_pattern(part)?;

                if first {
                    for host in sub_hosts {
                        result.insert(&host.name);
                    }
                    first = false;
                } else {
                    for host in sub_hosts {
                        result.insert(&host.name);
                    }
                }
            }
        }

        Ok(result
            .into_iter()
            .filter_map(|name| self.hosts.get(name))
            .collect())
    }

    /// Get all hosts in a group, including hosts from child groups
    fn get_hosts_in_group_recursive(&self, group: &Group) -> Vec<&Host> {
        let mut hosts: HashSet<&str> = HashSet::new();

        // Add direct hosts
        for host_name in &group.hosts {
            hosts.insert(host_name);
        }

        // Add hosts from child groups
        for child_name in &group.children {
            if let Some(child) = self.groups.get(child_name) {
                for host in self.get_hosts_in_group_recursive(child) {
                    hosts.insert(&host.name);
                }
            }
        }

        hosts
            .into_iter()
            .filter_map(|name| self.hosts.get(name))
            .collect()
    }

    /// Get the group hierarchy for a host (from most specific to least specific)
    pub fn get_host_group_hierarchy(&self, host: &Host) -> GroupHierarchy {
        let mut hierarchy = GroupHierarchy::new();
        let mut visited = HashSet::new();

        fn collect_parents(
            inventory: &Inventory,
            group_name: &str,
            hierarchy: &mut GroupHierarchy,
            visited: &mut HashSet<String>,
        ) {
            if visited.contains(group_name) {
                return;
            }
            visited.insert(group_name.to_string());
            hierarchy.push(group_name);

            if let Some(group) = inventory.groups.get(group_name) {
                for parent in &group.parents {
                    collect_parents(inventory, parent, hierarchy, visited);
                }
            }
        }

        // Helper to check if a group is an ancestor of another
        fn is_ancestor_of(
            inventory: &Inventory,
            potential_ancestor: &str,
            group: &str,
            visited: &mut HashSet<String>,
        ) -> bool {
            if visited.contains(group) {
                return false;
            }
            visited.insert(group.to_string());

            if let Some(g) = inventory.groups.get(group) {
                for parent in &g.parents {
                    if parent == potential_ancestor {
                        return true;
                    }
                    if is_ancestor_of(inventory, potential_ancestor, parent, visited) {
                        return true;
                    }
                }
            }
            false
        }

        // Filter host.groups to only include "leaf" groups (groups that are not
        // ancestors of any other group the host is in). This ensures we start
        // from the most specific groups and traverse up to parents.
        let host_groups: Vec<&String> = host.groups.iter().collect();
        let leaf_groups: Vec<&String> = host_groups
            .iter()
            .filter(|&group| {
                // A group is a "leaf" if no other group in host.groups has it as an ancestor
                !host_groups.iter().any(|other| {
                    if *other == *group {
                        return false;
                    }
                    let mut check_visited = HashSet::new();
                    is_ancestor_of(self, group, other, &mut check_visited)
                })
            })
            .copied()
            .collect();

        for group_name in leaf_groups {
            collect_parents(self, group_name, &mut hierarchy, &mut visited);
        }

        hierarchy
    }

    /// Get merged variables for a host (respecting group hierarchy)
    pub fn get_host_vars(&self, host: &Host) -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();

        // Get group hierarchy
        let hierarchy = self.get_host_group_hierarchy(host);

        // Apply variables from parent to child (so child overrides parent)
        for group_name in hierarchy.parent_to_child() {
            if let Some(group) = self.groups.get(group_name) {
                for (key, value) in &group.vars {
                    vars.insert(key.clone(), value.clone());
                }
            }
        }

        // Apply host-specific variables (highest precedence)
        for (key, value) in &host.vars {
            vars.insert(key.clone(), value.clone());
        }

        vars
    }

    /// Count total hosts
    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Count total groups
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

/// Split pattern by : but not inside brackets
fn split_pattern(pattern: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut bracket_depth: usize = 0;

    for (i, ch) in pattern.char_indices() {
        match ch {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ':' if bracket_depth == 0 => {
                parts.push(&pattern[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    parts.push(&pattern[start..]);
    parts
}

/// Convert a glob pattern to regex
fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");

    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' | ']' | '(' | ')' | '{' | '}' | '.' | '+' | '^' | '$' | '|' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    regex
}

/// Parse INI value (handle quoted strings, lists, etc.)
fn parse_ini_value(value: &str) -> serde_yaml::Value {
    let value = value.trim();

    // Handle quoted strings
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        return serde_yaml::Value::String(value[1..value.len() - 1].to_string());
    }

    // Handle booleans
    match value.to_lowercase().as_str() {
        "true" | "yes" | "on" | "y" | "t" => return serde_yaml::Value::Bool(true),
        "false" | "no" | "off" | "n" | "f" => return serde_yaml::Value::Bool(false),
        _ => {}
    }

    // Handle numbers
    if let Ok(n) = value.parse::<i64>() {
        return serde_yaml::Value::Number(n.into());
    }
    if let Ok(n) = value.parse::<f64>() {
        // Use From<i64> for the integer part, or convert to string for precision
        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
            return serde_yaml::Value::Number((n as i64).into());
        }
        // For floats, we need to use a different approach as serde_yaml may not support from_f64
        return serde_yaml::Value::Number(serde_yaml::Number::from(n as i64));
    }

    // Default to string
    serde_yaml::Value::String(value.to_string())
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
                // Convert float to integer if it has no fractional part
                if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    serde_yaml::Value::Number((f as i64).into())
                } else {
                    serde_yaml::Value::Number((f as i64).into())
                }
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

impl std::fmt::Display for Inventory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Inventory ({} hosts, {} groups)",
            self.hosts.len(),
            self.groups.len()
        )?;

        for group in self.groups.values() {
            if group.hosts.is_empty() && group.children.is_empty() {
                continue;
            }
            writeln!(f, "  [{}]", group.name)?;
            for host_name in &group.hosts {
                if let Some(host) = self.hosts.get(host_name) {
                    writeln!(f, "    {}", host)?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_inventory() {
        let inv = Inventory::new();
        assert_eq!(inv.host_count(), 0);
        assert!(inv.groups.contains_key("all"));
        assert!(inv.groups.contains_key("ungrouped"));
    }

    #[test]
    fn test_add_host() {
        let mut inv = Inventory::new();
        let host = Host::new("webserver1");
        inv.add_host(host).unwrap();

        assert_eq!(inv.host_count(), 1);
        assert!(inv.get_host("webserver1").is_some());
    }

    #[test]
    fn test_parse_ini() {
        let mut inv = Inventory::new();
        inv.parse_ini(
            r#"
[webservers]
web1 ansible_host=10.0.0.1
web2 ansible_host=10.0.0.2

[databases]
db1 ansible_host=10.0.0.10

[webservers:vars]
http_port=80

[production:children]
webservers
databases
        "#,
        )
        .unwrap();

        assert_eq!(inv.host_count(), 3);
        assert!(inv.get_group("webservers").is_some());
        assert!(inv.get_group("databases").is_some());
        assert!(inv.get_group("production").is_some());

        let webservers = inv.get_group("webservers").unwrap();
        assert!(webservers.has_host("web1"));
        assert!(webservers.has_host("web2"));
        assert!(webservers.has_var("http_port"));
    }

    #[test]
    fn test_pattern_matching() {
        let mut inv = Inventory::new();
        inv.parse_ini(
            r#"
[webservers]
web1
web2

[databases]
db1
        "#,
        )
        .unwrap();

        let all = inv.get_hosts_for_pattern("all").unwrap();
        assert_eq!(all.len(), 3);

        let webs = inv.get_hosts_for_pattern("webservers").unwrap();
        assert_eq!(webs.len(), 2);

        let single = inv.get_hosts_for_pattern("web1").unwrap();
        assert_eq!(single.len(), 1);
    }

    #[test]
    fn test_glob_pattern() {
        let mut inv = Inventory::new();
        inv.add_host(Host::new("web1")).unwrap();
        inv.add_host(Host::new("web2")).unwrap();
        inv.add_host(Host::new("db1")).unwrap();

        let webs = inv.get_hosts_for_pattern("web*").unwrap();
        assert_eq!(webs.len(), 2);
    }

    #[test]
    fn test_regex_pattern() {
        let mut inv = Inventory::new();
        inv.add_host(Host::new("web1")).unwrap();
        inv.add_host(Host::new("web2")).unwrap();
        inv.add_host(Host::new("db1")).unwrap();

        let webs = inv.get_hosts_for_pattern("~web\\d+").unwrap();
        assert_eq!(webs.len(), 2);
    }
}
