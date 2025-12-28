//! AWS EC2 Dynamic Inventory Plugin
//!
//! This plugin discovers AWS EC2 instances and creates inventory entries
//! with proper grouping based on tags, instance types, availability zones, etc.
//!
//! # Configuration
//!
//! ```yaml
//! plugin: aws_ec2
//! regions:
//!   - us-east-1
//!   - us-west-2
//! filters:
//!   tag:Environment: production
//!   instance-state-name: running
//! keyed_groups:
//!   - key: tags.Role
//!     prefix: role
//!   - key: placement.availability_zone
//!     prefix: az
//!   - key: instance_type
//!     prefix: type
//! hostnames:
//!   - tag:Name
//!   - private-dns-name
//!   - dns-name
//! compose:
//!   ansible_host: private_ip_address
//!   ansible_user: ec2-user
//! ```
//!
//! # Authentication
//!
//! The plugin uses standard AWS credential chain:
//! 1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
//! 2. Shared credentials file (~/.aws/credentials)
//! 3. IAM role (if running on EC2)
//!
//! # Features
//!
//! - Multi-region support
//! - Tag-based filtering and grouping
//! - Automatic group creation based on AWS attributes
//! - Caching for improved performance
//! - Support for VPC and security group grouping

use super::config::{sanitize_group_name, PluginConfig, PluginConfigError};
use super::{DynamicInventoryPlugin, PluginOption, PluginOptionType};
use crate::inventory::{Group, Host, Inventory, InventoryError, InventoryResult};
use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// AWS EC2 instance data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ec2Instance {
    /// Instance ID
    pub instance_id: String,
    /// Instance type (e.g., t2.micro)
    pub instance_type: String,
    /// Instance state (running, stopped, etc.)
    pub state: String,
    /// Private IP address
    pub private_ip_address: Option<String>,
    /// Public IP address
    pub public_ip_address: Option<String>,
    /// Private DNS name
    pub private_dns_name: Option<String>,
    /// Public DNS name
    pub public_dns_name: Option<String>,
    /// Availability zone
    pub availability_zone: String,
    /// VPC ID
    pub vpc_id: Option<String>,
    /// Subnet ID
    pub subnet_id: Option<String>,
    /// Security group IDs
    pub security_groups: Vec<String>,
    /// IAM instance profile ARN
    pub iam_instance_profile: Option<String>,
    /// Instance tags
    pub tags: HashMap<String, String>,
    /// Launch time
    pub launch_time: Option<String>,
    /// Architecture (x86_64, arm64)
    pub architecture: Option<String>,
    /// Platform (windows, linux)
    pub platform: Option<String>,
    /// Key name for SSH
    pub key_name: Option<String>,
    /// Region
    pub region: String,
}

impl Ec2Instance {
    /// Get instance name from tags
    pub fn name(&self) -> Option<&str> {
        self.tags.get("Name").map(|s| s.as_str())
    }

    /// Get a tag value
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|s| s.as_str())
    }

    /// Get the best hostname based on preferences
    pub fn hostname(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            let value = match pref.as_str() {
                "tag:Name" | "name" => self.tags.get("Name").cloned(),
                "private-dns-name" | "private_dns_name" => self.private_dns_name.clone(),
                "dns-name" | "public_dns_name" => self.public_dns_name.clone(),
                "private-ip-address" | "private_ip_address" => self.private_ip_address.clone(),
                "ip-address" | "public_ip_address" => self.public_ip_address.clone(),
                "instance-id" | "instance_id" => Some(self.instance_id.clone()),
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
        self.private_dns_name
            .clone()
            .or_else(|| self.private_ip_address.clone())
            .or_else(|| Some(self.instance_id.clone()))
    }

    /// Convert to host variables
    pub fn to_host_vars(&self) -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();

        // Core instance attributes
        vars.insert(
            "ec2_instance_id".to_string(),
            serde_yaml::Value::String(self.instance_id.clone()),
        );
        vars.insert(
            "ec2_instance_type".to_string(),
            serde_yaml::Value::String(self.instance_type.clone()),
        );
        vars.insert(
            "ec2_state".to_string(),
            serde_yaml::Value::String(self.state.clone()),
        );
        vars.insert(
            "ec2_region".to_string(),
            serde_yaml::Value::String(self.region.clone()),
        );
        vars.insert(
            "ec2_availability_zone".to_string(),
            serde_yaml::Value::String(self.availability_zone.clone()),
        );

        // Network attributes
        if let Some(ref ip) = self.private_ip_address {
            vars.insert(
                "ec2_private_ip_address".to_string(),
                serde_yaml::Value::String(ip.clone()),
            );
        }
        if let Some(ref ip) = self.public_ip_address {
            vars.insert(
                "ec2_public_ip_address".to_string(),
                serde_yaml::Value::String(ip.clone()),
            );
        }
        if let Some(ref dns) = self.private_dns_name {
            vars.insert(
                "ec2_private_dns_name".to_string(),
                serde_yaml::Value::String(dns.clone()),
            );
        }
        if let Some(ref dns) = self.public_dns_name {
            vars.insert(
                "ec2_public_dns_name".to_string(),
                serde_yaml::Value::String(dns.clone()),
            );
        }
        if let Some(ref vpc) = self.vpc_id {
            vars.insert(
                "ec2_vpc_id".to_string(),
                serde_yaml::Value::String(vpc.clone()),
            );
        }
        if let Some(ref subnet) = self.subnet_id {
            vars.insert(
                "ec2_subnet_id".to_string(),
                serde_yaml::Value::String(subnet.clone()),
            );
        }

        // Security groups
        if !self.security_groups.is_empty() {
            vars.insert(
                "ec2_security_groups".to_string(),
                serde_yaml::Value::Sequence(
                    self.security_groups
                        .iter()
                        .map(|s| serde_yaml::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

        // Other attributes
        if let Some(ref profile) = self.iam_instance_profile {
            vars.insert(
                "ec2_iam_instance_profile".to_string(),
                serde_yaml::Value::String(profile.clone()),
            );
        }
        if let Some(ref arch) = self.architecture {
            vars.insert(
                "ec2_architecture".to_string(),
                serde_yaml::Value::String(arch.clone()),
            );
        }
        if let Some(ref platform) = self.platform {
            vars.insert(
                "ec2_platform".to_string(),
                serde_yaml::Value::String(platform.clone()),
            );
        }
        if let Some(ref key) = self.key_name {
            vars.insert(
                "ec2_key_name".to_string(),
                serde_yaml::Value::String(key.clone()),
            );
        }
        if let Some(ref launch) = self.launch_time {
            vars.insert(
                "ec2_launch_time".to_string(),
                serde_yaml::Value::String(launch.clone()),
            );
        }

        // All tags as nested structure
        if !self.tags.is_empty() {
            let mut tags_map = serde_yaml::Mapping::new();
            for (k, v) in &self.tags {
                tags_map.insert(
                    serde_yaml::Value::String(k.clone()),
                    serde_yaml::Value::String(v.clone()),
                );
            }
            vars.insert("ec2_tags".to_string(), serde_yaml::Value::Mapping(tags_map));
        }

        vars
    }
}

/// AWS EC2 inventory plugin
#[derive(Debug)]
pub struct AwsEc2Plugin {
    config: PluginConfig,
    /// Cached instances (region -> instances)
    #[allow(dead_code)]
    cached_instances: std::sync::RwLock<Option<Vec<Ec2Instance>>>,
}

impl AwsEc2Plugin {
    /// Create a new AWS EC2 plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        Ok(Self {
            config,
            cached_instances: std::sync::RwLock::new(None),
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("aws_ec2");
        Self::new(config)
    }

    /// Get configured regions or default
    fn get_regions(&self) -> Vec<String> {
        if !self.config.regions.is_empty() {
            return self.config.regions.clone();
        }

        // Try environment variables
        if let Ok(region) = std::env::var("AWS_REGION") {
            return vec![region];
        }
        if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
            return vec![region];
        }

        // Default region
        vec!["us-east-1".to_string()]
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
            "private-dns-name".to_string(),
            "dns-name".to_string(),
        ]
    }

    /// Check if instance passes filters
    fn instance_passes_filters(&self, instance: &Ec2Instance) -> bool {
        for (key, filter_config) in &self.config.filters {
            let filter_values = filter_config.values();
            let instance_value = match key.as_str() {
                "instance-state-name" | "state" => Some(instance.state.as_str()),
                "instance-type" => Some(instance.instance_type.as_str()),
                "availability-zone" => Some(instance.availability_zone.as_str()),
                "vpc-id" => instance.vpc_id.as_deref(),
                "subnet-id" => instance.subnet_id.as_deref(),
                "instance-id" => Some(instance.instance_id.as_str()),
                "architecture" => instance.architecture.as_deref(),
                "platform" => instance.platform.as_deref(),
                k if k.starts_with("tag:") => {
                    let tag_name = &k[4..];
                    instance.tags.get(tag_name).map(|s| s.as_str())
                }
                _ => None,
            };

            // If filter key doesn't match any known attribute, skip it
            let Some(value) = instance_value else {
                continue;
            };

            // Check if any filter value matches
            if !filter_values.iter().any(|fv| *fv == value) {
                return false;
            }
        }

        true
    }

    /// Get groups for an instance based on keyed_groups configuration
    fn get_instance_groups(&self, instance: &Ec2Instance) -> Vec<String> {
        let mut groups = vec!["aws_ec2".to_string()];

        // Add region group
        groups.push(format!("region_{}", sanitize_group_name(&instance.region)));

        // Add availability zone group
        groups.push(format!(
            "az_{}",
            sanitize_group_name(&instance.availability_zone)
        ));

        // Add instance type group
        groups.push(format!(
            "type_{}",
            sanitize_group_name(&instance.instance_type)
        ));

        // Add VPC group
        if let Some(ref vpc) = instance.vpc_id {
            groups.push(format!("vpc_{}", sanitize_group_name(vpc)));
        }

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

        // Add tag-based groups
        for (key, value) in &instance.tags {
            let safe_key = sanitize_group_name(key);
            let safe_value = sanitize_group_name(value);
            groups.push(format!("tag_{}_{}", safe_key, safe_value));
        }

        groups
    }

    /// Resolve a keyed group key to a value
    fn resolve_keyed_group_key(&self, key: &str, instance: &Ec2Instance) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();

        match parts.as_slice() {
            ["tags" | "tag", tag_name] => instance.tags.get(*tag_name).cloned(),
            ["placement", "availability_zone"] => Some(instance.availability_zone.clone()),
            ["placement", "region"] => Some(instance.region.clone()),
            ["instance_type"] => Some(instance.instance_type.clone()),
            ["instance_id"] => Some(instance.instance_id.clone()),
            ["state" | "instance_state"] => Some(instance.state.clone()),
            ["vpc_id"] => instance.vpc_id.clone(),
            ["subnet_id"] => instance.subnet_id.clone(),
            ["architecture"] => instance.architecture.clone(),
            ["platform"] => instance.platform.clone(),
            ["key_name"] => instance.key_name.clone(),
            _ => None,
        }
    }

    /// Apply compose configuration to set host variables
    fn apply_compose(&self, host: &mut Host, instance: &Ec2Instance) {
        let compose = &self.config.compose;

        // Set ansible_host
        if let Some(ref expr) = compose.ansible_host {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                host.ansible_host = Some(value);
            }
        } else {
            // Default: use private IP
            if let Some(ref ip) = instance.private_ip_address {
                host.ansible_host = Some(ip.clone());
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
        } else {
            // Default user based on platform
            let user = if instance.platform.as_deref() == Some("windows") {
                "Administrator"
            } else {
                // Common defaults for Linux AMIs
                "ec2-user"
            };
            host.connection.ssh.user = Some(user.to_string());
        }

        // Apply extra vars from compose
        for (key, expr) in &compose.extra_vars {
            if let Some(value) = self.resolve_compose_expression(expr, instance) {
                host.set_var(key, serde_yaml::Value::String(value));
            }
        }
    }

    /// Resolve a compose expression to a value
    fn resolve_compose_expression(&self, expr: &str, instance: &Ec2Instance) -> Option<String> {
        // Simple attribute resolution (not full Jinja2)
        match expr {
            "private_ip_address" => instance.private_ip_address.clone(),
            "public_ip_address" => instance.public_ip_address.clone(),
            "private_dns_name" => instance.private_dns_name.clone(),
            "public_dns_name" => instance.public_dns_name.clone(),
            "instance_id" => Some(instance.instance_id.clone()),
            "instance_type" => Some(instance.instance_type.clone()),
            "availability_zone" => Some(instance.availability_zone.clone()),
            "region" => Some(instance.region.clone()),
            "vpc_id" => instance.vpc_id.clone(),
            "key_name" => instance.key_name.clone(),
            s if s.starts_with("tags.") => {
                let tag_name = &s[5..];
                instance.tags.get(tag_name).cloned()
            }
            _ => Some(expr.to_string()), // Literal value
        }
    }

    /// Fetch instances from AWS (simulated for now)
    ///
    /// Note: A full implementation would use the AWS SDK for Rust (aws-sdk-ec2)
    /// This is a skeleton that demonstrates the plugin architecture
    async fn fetch_instances(&self) -> InventoryResult<Vec<Ec2Instance>> {
        let regions = self.get_regions();

        // Log what we're doing
        tracing::info!(
            "AWS EC2 plugin: Querying {} region(s): {}",
            regions.len(),
            regions.join(", ")
        );

        // In a real implementation, this would call AWS EC2 DescribeInstances API
        // using aws-sdk-ec2 crate. For now, return an empty list.
        //
        // Example with AWS SDK:
        // ```rust
        // let shared_config = aws_config::from_env().region(region).load().await;
        // let client = aws_sdk_ec2::Client::new(&shared_config);
        // let response = client.describe_instances()
        //     .filters(/* filters from config */)
        //     .send()
        //     .await?;
        // ```

        tracing::warn!(
            "AWS EC2 plugin: AWS SDK integration not yet implemented. \
             Configure AWS credentials and install aws-sdk-ec2 for full functionality."
        );

        Ok(Vec::new())
    }

    /// Convert instances to inventory
    fn instances_to_inventory(&self, instances: Vec<Ec2Instance>) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let hostname_prefs = self.get_hostname_preferences();

        // Create base aws_ec2 group
        let mut aws_ec2_group = Group::new("aws_ec2");
        aws_ec2_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("aws_ec2".to_string()),
        );

        // Process each instance
        for instance in &instances {
            // Skip instances that don't pass filters
            if !self.instance_passes_filters(instance) {
                continue;
            }

            // Determine hostname
            let Some(hostname) = instance.hostname(&hostname_prefs) else {
                tracing::warn!(
                    "AWS EC2 plugin: Could not determine hostname for instance {}",
                    instance.instance_id
                );
                continue;
            };

            // Create host
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

        // Add the base aws_ec2 group
        inventory.add_group(aws_ec2_group)?;

        Ok(inventory)
    }
}

#[async_trait]
impl DynamicInventoryPlugin for AwsEc2Plugin {
    fn name(&self) -> &str {
        "aws_ec2"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "AWS EC2 instances dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        // Check for AWS credentials
        let has_env_creds = std::env::var("AWS_ACCESS_KEY_ID").is_ok()
            && std::env::var("AWS_SECRET_ACCESS_KEY").is_ok();

        let has_profile = std::env::var("AWS_PROFILE").is_ok();

        let has_creds_file = dirs::home_dir()
            .map(|h| h.join(".aws/credentials").exists())
            .unwrap_or(false);

        if !has_env_creds && !has_profile && !has_creds_file {
            tracing::warn!(
                "AWS EC2 plugin: No AWS credentials found. \
                 Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY, AWS_PROFILE, \
                 or configure ~/.aws/credentials"
            );
        }

        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        // Verify configuration
        self.verify()?;

        // Fetch instances from AWS
        let instances = self.fetch_instances().await?;

        // Convert to inventory
        self.instances_to_inventory(instances)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        // Clear cache
        let mut cache = self.cached_instances.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::optional_list("regions", "AWS regions to query")
                .with_env_var("AWS_REGION"),
            PluginOption {
                name: "filters".to_string(),
                description: "EC2 instance filters (e.g., tag:Environment, instance-state-name)"
                    .to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption::optional_list(
                "hostnames",
                "Hostname preferences in order (tag:Name, private-dns-name, etc.)",
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
            PluginOption::optional_bool(
                "include_stopped",
                "Include stopped instances in inventory",
                false,
            ),
            PluginOption::optional_bool(
                "use_private_ip",
                "Use private IP for ansible_host (default: true)",
                true,
            ),
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

    fn create_test_instance() -> Ec2Instance {
        let mut tags = HashMap::new();
        tags.insert("Name".to_string(), "web-server-01".to_string());
        tags.insert("Environment".to_string(), "production".to_string());
        tags.insert("Role".to_string(), "webserver".to_string());

        Ec2Instance {
            instance_id: "i-1234567890abcdef0".to_string(),
            instance_type: "t3.medium".to_string(),
            state: "running".to_string(),
            private_ip_address: Some("10.0.1.100".to_string()),
            public_ip_address: Some("54.123.45.67".to_string()),
            private_dns_name: Some("ip-10-0-1-100.ec2.internal".to_string()),
            public_dns_name: Some("ec2-54-123-45-67.compute-1.amazonaws.com".to_string()),
            availability_zone: "us-east-1a".to_string(),
            vpc_id: Some("vpc-12345678".to_string()),
            subnet_id: Some("subnet-12345678".to_string()),
            security_groups: vec!["sg-12345678".to_string()],
            iam_instance_profile: Some(
                "arn:aws:iam::123456789012:instance-profile/WebServer".to_string(),
            ),
            tags,
            launch_time: Some("2024-01-15T10:30:00Z".to_string()),
            architecture: Some("x86_64".to_string()),
            platform: None,
            key_name: Some("my-key-pair".to_string()),
            region: "us-east-1".to_string(),
        }
    }

    #[test]
    fn test_instance_hostname() {
        let instance = create_test_instance();

        // Tag:Name preference
        let prefs = vec!["tag:Name".to_string()];
        assert_eq!(instance.hostname(&prefs), Some("web-server-01".to_string()));

        // Private DNS preference
        let prefs = vec!["private-dns-name".to_string()];
        assert_eq!(
            instance.hostname(&prefs),
            Some("ip-10-0-1-100.ec2.internal".to_string())
        );

        // Instance ID fallback
        let prefs = vec!["nonexistent".to_string(), "instance-id".to_string()];
        assert_eq!(
            instance.hostname(&prefs),
            Some("i-1234567890abcdef0".to_string())
        );
    }

    #[test]
    fn test_instance_to_host_vars() {
        let instance = create_test_instance();
        let vars = instance.to_host_vars();

        assert!(vars.contains_key("ec2_instance_id"));
        assert!(vars.contains_key("ec2_instance_type"));
        assert!(vars.contains_key("ec2_private_ip_address"));
        assert!(vars.contains_key("ec2_tags"));
    }

    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("aws_ec2");
        let plugin = AwsEc2Plugin::new(config).unwrap();
        assert_eq!(plugin.name(), "aws_ec2");
    }

    #[test]
    fn test_keyed_group_resolution() {
        let config = PluginConfig::new("aws_ec2");
        let plugin = AwsEc2Plugin::new(config).unwrap();
        let instance = create_test_instance();

        // Test tag resolution
        let value = plugin.resolve_keyed_group_key("tags.Environment", &instance);
        assert_eq!(value, Some("production".to_string()));

        // Test attribute resolution
        let value = plugin.resolve_keyed_group_key("instance_type", &instance);
        assert_eq!(value, Some("t3.medium".to_string()));

        let value = plugin.resolve_keyed_group_key("placement.availability_zone", &instance);
        assert_eq!(value, Some("us-east-1a".to_string()));
    }

    #[test]
    fn test_filter_matching() {
        let mut config = PluginConfig::new("aws_ec2");
        config.filters.insert(
            "instance-state-name".to_string(),
            super::super::config::FilterConfig::Single("running".to_string()),
        );
        config.filters.insert(
            "tag:Environment".to_string(),
            super::super::config::FilterConfig::Single("production".to_string()),
        );

        let plugin = AwsEc2Plugin::new(config).unwrap();
        let instance = create_test_instance();

        assert!(plugin.instance_passes_filters(&instance));
    }

    #[test]
    fn test_instance_groups() {
        let config = PluginConfig::new("aws_ec2");
        let plugin = AwsEc2Plugin::new(config).unwrap();
        let instance = create_test_instance();

        let groups = plugin.get_instance_groups(&instance);

        assert!(groups.contains(&"aws_ec2".to_string()));
        assert!(groups.contains(&"region_us_east_1".to_string()));
        assert!(groups.contains(&"az_us_east_1a".to_string()));
        assert!(groups.contains(&"type_t3_medium".to_string()));
    }
}
