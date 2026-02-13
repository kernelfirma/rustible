//! Slurm HPC Dynamic Inventory Plugin
//!
//! This plugin discovers compute nodes from a Slurm workload manager cluster
//! and creates inventory entries with proper grouping based on partitions,
//! node states, features, and GRES (Generic Resources).
//!
//! # Configuration
//!
//! ```yaml
//! plugin: slurm
//! # Optional: path to scontrol binary (default: auto-detect via PATH)
//! scontrol_path: /usr/bin/scontrol
//! # Optional: filter by partition
//! partitions:
//!   - compute
//!   - gpu
//! # Optional: filter by node state
//! states:
//!   - idle
//!   - mixed
//!   - allocated
//! keyed_groups:
//!   - key: slurm_partition
//!     prefix: partition
//!   - key: slurm_state
//!     prefix: state
//! compose:
//!   ansible_host: slurm_node_addr
//! ```
//!
//! # Authentication
//!
//! The plugin requires access to the `scontrol` command, which is typically
//! available on Slurm login/admin nodes. No additional authentication is needed
//! beyond having the Slurm client tools installed.
//!
//! # Features
//!
//! - Automatic node discovery via `scontrol show nodes`
//! - Grouping by partition, state, features, and GRES
//! - Node resource information (CPUs, memory, GPUs) as host variables
//! - Support for keyed_groups and compose configuration
//! - Caching for improved performance

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

/// Parsed Slurm node data
#[derive(Debug, Clone)]
pub struct SlurmNode {
    /// Node name
    pub node_name: String,
    /// Node state (e.g., idle, allocated, mixed, down, drained)
    pub state: String,
    /// Comma-separated list of partitions the node belongs to
    pub partitions: Vec<String>,
    /// Number of CPUs
    pub cpus: u32,
    /// Real memory in MB
    pub real_memory: u64,
    /// Comma-separated feature list
    pub features: Vec<String>,
    /// Generic resources (e.g., gpu:4)
    pub gres: Vec<String>,
    /// Node address (if different from node name)
    pub node_addr: Option<String>,
    /// Additional raw key-value pairs from scontrol output
    pub extra: HashMap<String, String>,
}

/// Slurm HPC inventory plugin
#[derive(Debug)]
pub struct SlurmPlugin {
    config: PluginConfig,
    /// Cached node data
    #[allow(dead_code)]
    cached_nodes: RwLock<Option<Vec<SlurmNode>>>,
}

impl SlurmPlugin {
    /// Create a new Slurm plugin with configuration
    pub fn new(config: PluginConfig) -> Result<Self, PluginConfigError> {
        Ok(Self {
            config,
            cached_nodes: RwLock::new(None),
        })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        let config = PluginConfig::new("slurm");
        Self::new(config)
    }

    /// Get the path to scontrol binary
    fn scontrol_path(&self) -> String {
        self.config
            .get_string("scontrol_path")
            .unwrap_or_else(|| "scontrol".to_string())
    }

    /// Parse a single line of `scontrol show nodes -o` output into a SlurmNode.
    ///
    /// Each line is a space-separated list of Key=Value pairs.
    fn parse_node_line(line: &str) -> Option<SlurmNode> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let mut kv: HashMap<String, String> = HashMap::new();

        // scontrol -o output uses space-separated Key=Value pairs.
        // Some values may themselves contain spaces when quoted, but the
        // one-liner format produced by -o generally avoids that.
        for token in line.split_whitespace() {
            if let Some((key, value)) = token.split_once('=') {
                kv.insert(key.to_string(), value.to_string());
            }
        }

        let node_name = kv.get("NodeName")?.clone();
        if node_name.is_empty() {
            return None;
        }

        let state = kv
            .get("State")
            .cloned()
            .unwrap_or_else(|| "UNKNOWN".to_string())
            .to_lowercase();

        let partitions: Vec<String> = kv
            .get("Partitions")
            .map(|p| {
                p.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let cpus: u32 = kv.get("CPUTot").and_then(|v| v.parse().ok()).unwrap_or(0);

        let real_memory: u64 = kv
            .get("RealMemory")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let features: Vec<String> = kv
            .get("AvailableFeatures")
            .or_else(|| kv.get("Features"))
            .map(|f| {
                f.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s != "(null)")
                    .collect()
            })
            .unwrap_or_default();

        let gres: Vec<String> = kv
            .get("Gres")
            .map(|g| {
                g.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s != "(null)")
                    .collect()
            })
            .unwrap_or_default();

        let node_addr = kv.get("NodeAddr").cloned().filter(|a| a != &node_name);

        // Collect remaining key-value pairs as extra
        let reserved_keys = [
            "NodeName",
            "State",
            "Partitions",
            "CPUTot",
            "RealMemory",
            "AvailableFeatures",
            "Features",
            "Gres",
            "NodeAddr",
        ];
        let extra: HashMap<String, String> = kv
            .into_iter()
            .filter(|(k, _)| !reserved_keys.contains(&k.as_str()))
            .collect();

        Some(SlurmNode {
            node_name,
            state,
            partitions,
            cpus,
            real_memory,
            features,
            gres,
            node_addr,
            extra,
        })
    }

    /// Execute `scontrol show nodes -o` and parse the output into SlurmNode entries.
    async fn fetch_nodes(&self) -> InventoryResult<Vec<SlurmNode>> {
        let scontrol = self.scontrol_path();

        tracing::info!("Slurm plugin: Running '{}' show nodes -o", scontrol);

        let output = tokio::process::Command::new(&scontrol)
            .args(["show", "nodes", "-o"])
            .output()
            .await
            .map_err(|e| {
                InventoryError::DynamicInventoryFailed(format!(
                    "Failed to execute '{}': {}",
                    scontrol, e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InventoryError::DynamicInventoryFailed(format!(
                "'scontrol show nodes' failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut nodes: Vec<SlurmNode> = Vec::new();

        for line in stdout.lines() {
            if let Some(node) = Self::parse_node_line(line) {
                nodes.push(node);
            }
        }

        // Apply partition filter if configured
        let partition_filter: Option<Vec<String>> = self.config.get_string_list("partitions");
        // Apply state filter if configured
        let state_filter: Option<Vec<String>> = self.config.get_string_list("states");

        let nodes: Vec<SlurmNode> = nodes
            .into_iter()
            .filter(|node| {
                if let Some(ref partitions) = partition_filter {
                    if !node.partitions.iter().any(|p| partitions.contains(p)) {
                        return false;
                    }
                }
                if let Some(ref states) = state_filter {
                    // Slurm states can have modifiers like "idle*" or "down+drain"
                    let base_state = node
                        .state
                        .split(&['+', '*', '~'][..])
                        .next()
                        .unwrap_or(&node.state);
                    if !states.iter().any(|s| s.to_lowercase() == base_state) {
                        return false;
                    }
                }
                true
            })
            .collect();

        tracing::info!("Slurm plugin: Discovered {} nodes", nodes.len());

        // Cache the results
        if let Ok(mut cache) = self.cached_nodes.write() {
            *cache = Some(nodes.clone());
        }

        Ok(nodes)
    }

    /// Convert parsed SlurmNode entries into an Inventory.
    fn nodes_to_inventory(&self, nodes: Vec<SlurmNode>) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();
        let mut groups_map: HashMap<String, Group> = HashMap::new();

        // Create the base slurm group
        let mut slurm_group = Group::new("slurm");
        slurm_group.set_var(
            "plugin".to_string(),
            serde_yaml::Value::String("slurm".to_string()),
        );

        for node in &nodes {
            let mut host = Host::new(&node.node_name);

            // Set ansible_host from compose or node address
            if let Some(ref expr) = self.config.compose.ansible_host {
                if let Some(value) = self.resolve_compose_expression(expr, node) {
                    host.ansible_host = Some(value);
                }
            } else if let Some(ref addr) = node.node_addr {
                host.ansible_host = Some(addr.clone());
            }

            // Apply compose ansible_user
            if let Some(ref expr) = self.config.compose.ansible_user {
                if let Some(value) = self.resolve_compose_expression(expr, node) {
                    host.connection.ssh.user = Some(value);
                }
            }

            // Apply compose ansible_port
            if let Some(ref expr) = self.config.compose.ansible_port {
                if let Some(value) = self.resolve_compose_expression(expr, node) {
                    if let Ok(port) = value.parse::<u16>() {
                        host.connection.ssh.port = port;
                    }
                }
            }

            // Apply extra vars from compose
            for (key, expr) in &self.config.compose.extra_vars {
                if let Some(value) = self.resolve_compose_expression(expr, node) {
                    host.set_var(key, serde_yaml::Value::String(value));
                }
            }

            // Set slurm-specific host variables
            host.vars.insert(
                "slurm_state".to_string(),
                serde_yaml::Value::String(node.state.clone()),
            );
            host.vars.insert(
                "slurm_cpus".to_string(),
                serde_yaml::Value::Number(serde_yaml::Number::from(node.cpus as u64)),
            );
            host.vars.insert(
                "slurm_real_memory".to_string(),
                serde_yaml::Value::Number(serde_yaml::Number::from(node.real_memory)),
            );

            if !node.partitions.is_empty() {
                host.vars.insert(
                    "slurm_partitions".to_string(),
                    serde_yaml::Value::Sequence(
                        node.partitions
                            .iter()
                            .map(|p| serde_yaml::Value::String(p.clone()))
                            .collect(),
                    ),
                );
                // Also store the first partition as slurm_partition for simple keyed_groups
                host.vars.insert(
                    "slurm_partition".to_string(),
                    serde_yaml::Value::String(node.partitions[0].clone()),
                );
            }

            if !node.features.is_empty() {
                host.vars.insert(
                    "slurm_features".to_string(),
                    serde_yaml::Value::Sequence(
                        node.features
                            .iter()
                            .map(|f| serde_yaml::Value::String(f.clone()))
                            .collect(),
                    ),
                );
            }

            if !node.gres.is_empty() {
                host.vars.insert(
                    "slurm_gres".to_string(),
                    serde_yaml::Value::Sequence(
                        node.gres
                            .iter()
                            .map(|g| serde_yaml::Value::String(g.clone()))
                            .collect(),
                    ),
                );
            }

            if let Some(ref addr) = node.node_addr {
                host.vars.insert(
                    "slurm_node_addr".to_string(),
                    serde_yaml::Value::String(addr.clone()),
                );
            }

            // Build group membership

            // Base slurm group
            let group_names = self.get_node_groups(node);
            for group_name in &group_names {
                host.add_to_group(group_name.clone());

                groups_map
                    .entry(group_name.clone())
                    .or_insert_with(|| Group::new(group_name))
                    .add_host(node.node_name.clone());
            }

            // Add host to base slurm group
            host.add_to_group("slurm".to_string());
            slurm_group.add_host(node.node_name.clone());

            inventory.add_host(host)?;
        }

        // Add all discovered groups to inventory
        for (_, group) in groups_map {
            inventory.add_group(group)?;
        }
        inventory.add_group(slurm_group)?;

        Ok(inventory)
    }

    /// Determine the set of groups a node should belong to.
    fn get_node_groups(&self, node: &SlurmNode) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();

        // State-based group (e.g., slurm_state_idle)
        let base_state = node
            .state
            .split(&['+', '*', '~'][..])
            .next()
            .unwrap_or(&node.state);
        groups.push(format!("slurm_state_{}", sanitize_group_name(base_state)));

        // Partition-based groups (e.g., slurm_partition_compute)
        for partition in &node.partitions {
            groups.push(format!(
                "slurm_partition_{}",
                sanitize_group_name(partition)
            ));
        }

        // Feature-based groups (e.g., slurm_feature_avx2)
        for feature in &node.features {
            groups.push(format!("slurm_feature_{}", sanitize_group_name(feature)));
        }

        // GRES-based groups (e.g., slurm_gres_gpu)
        for gres_entry in &node.gres {
            // GRES format is typically "name:type:count" or "name:count"
            let gres_name = gres_entry.split(':').next().unwrap_or(gres_entry);
            groups.push(format!("slurm_gres_{}", sanitize_group_name(gres_name)));
        }

        // Process keyed_groups configuration
        for keyed_group in &self.config.keyed_groups {
            if let Some(value) = self.resolve_keyed_group_key(&keyed_group.key, node) {
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

    /// Resolve a keyed group key to a value from node data.
    fn resolve_keyed_group_key(&self, key: &str, node: &SlurmNode) -> Option<String> {
        match key {
            "slurm_state" | "state" => Some(node.state.clone()),
            "slurm_partition" | "partition" => node.partitions.first().cloned(),
            "slurm_cpus" | "cpus" => Some(node.cpus.to_string()),
            "slurm_real_memory" | "real_memory" => Some(node.real_memory.to_string()),
            "slurm_node_addr" | "node_addr" => node.node_addr.clone(),
            _ => {
                // Try extra fields
                node.extra.get(key).cloned()
            }
        }
    }

    /// Resolve a compose expression to a value from node data.
    fn resolve_compose_expression(&self, expr: &str, node: &SlurmNode) -> Option<String> {
        match expr {
            "slurm_node_addr" | "node_addr" => node
                .node_addr
                .clone()
                .or_else(|| Some(node.node_name.clone())),
            "slurm_state" | "state" => Some(node.state.clone()),
            "slurm_partition" | "partition" => node.partitions.first().cloned(),
            "slurm_cpus" | "cpus" => Some(node.cpus.to_string()),
            "slurm_real_memory" | "real_memory" => Some(node.real_memory.to_string()),
            "node_name" => Some(node.node_name.clone()),
            _ => {
                // Try extra fields, fall back to literal value
                node.extra
                    .get(expr)
                    .cloned()
                    .or_else(|| Some(expr.to_string()))
            }
        }
    }
}

#[async_trait]
impl DynamicInventoryPlugin for SlurmPlugin {
    fn name(&self) -> &str {
        "slurm"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Slurm HPC workload manager dynamic inventory plugin"
    }

    fn verify(&self) -> InventoryResult<()> {
        let scontrol = self.scontrol_path();

        // Check if scontrol is available
        match which::which(&scontrol) {
            Ok(path) => {
                tracing::debug!("Slurm plugin: Found scontrol at {}", path.display());
                Ok(())
            }
            Err(_) => Err(InventoryError::DynamicInventoryFailed(format!(
                "Slurm plugin: '{}' not found in PATH. \
                 Ensure Slurm client tools are installed.",
                scontrol
            ))),
        }
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        self.verify()?;

        let nodes = self.fetch_nodes().await?;
        self.nodes_to_inventory(nodes)
    }

    async fn refresh(&self) -> InventoryResult<()> {
        let mut cache = self.cached_nodes.write().map_err(|e| {
            InventoryError::DynamicInventoryFailed(format!("Failed to acquire cache lock: {}", e))
        })?;
        *cache = None;
        Ok(())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption::optional_string(
                "scontrol_path",
                "Path to scontrol binary (default: auto-detect)",
                "scontrol",
            ),
            PluginOption::optional_list("partitions", "Filter nodes by partition name(s)"),
            PluginOption::optional_list(
                "states",
                "Filter nodes by state (idle, allocated, mixed, down, drained)",
            ),
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic group creation based on node attributes".to_string(),
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

    fn sample_scontrol_line() -> &'static str {
        "NodeName=node001 Arch=x86_64 CoresPerSocket=16 CPUAlloc=0 CPUTot=32 \
         CPULoad=0.01 AvailableFeatures=avx2,sse4 ActiveFeatures=avx2,sse4 \
         Gres=gpu:a100:4 NodeAddr=10.0.1.1 NodeHostName=node001 \
         OS=Linux RealMemory=256000 State=idle Partitions=compute,gpu"
    }

    #[test]
    fn test_parse_node_line() {
        let node = SlurmPlugin::parse_node_line(sample_scontrol_line()).unwrap();

        assert_eq!(node.node_name, "node001");
        assert_eq!(node.state, "idle");
        assert_eq!(node.cpus, 32);
        assert_eq!(node.real_memory, 256000);
        assert_eq!(node.partitions, vec!["compute", "gpu"]);
        assert_eq!(node.features, vec!["avx2", "sse4"]);
        assert_eq!(node.gres, vec!["gpu:a100:4"]);
        assert_eq!(node.node_addr, Some("10.0.1.1".to_string()));
    }

    #[test]
    fn test_parse_empty_line() {
        assert!(SlurmPlugin::parse_node_line("").is_none());
        assert!(SlurmPlugin::parse_node_line("   ").is_none());
    }

    #[test]
    fn test_parse_minimal_line() {
        let node =
            SlurmPlugin::parse_node_line("NodeName=simple State=DOWN CPUTot=4 RealMemory=8000")
                .unwrap();
        assert_eq!(node.node_name, "simple");
        assert_eq!(node.state, "down");
        assert_eq!(node.cpus, 4);
        assert_eq!(node.real_memory, 8000);
        assert!(node.partitions.is_empty());
        assert!(node.features.is_empty());
        assert!(node.gres.is_empty());
        assert!(node.node_addr.is_none());
    }

    #[test]
    fn test_plugin_creation() {
        let config = PluginConfig::new("slurm");
        let plugin = SlurmPlugin::new(config).unwrap();
        assert_eq!(plugin.name(), "slurm");
        assert_eq!(plugin.version(), "1.0.0");
    }

    #[test]
    fn test_with_defaults() {
        let plugin = SlurmPlugin::with_defaults().unwrap();
        assert_eq!(plugin.name(), "slurm");
    }

    #[test]
    fn test_node_groups() {
        let config = PluginConfig::new("slurm");
        let plugin = SlurmPlugin::new(config).unwrap();

        let node = SlurmPlugin::parse_node_line(sample_scontrol_line()).unwrap();
        let groups = plugin.get_node_groups(&node);

        assert!(groups.contains(&"slurm_state_idle".to_string()));
        assert!(groups.contains(&"slurm_partition_compute".to_string()));
        assert!(groups.contains(&"slurm_partition_gpu".to_string()));
        assert!(groups.contains(&"slurm_feature_avx2".to_string()));
        assert!(groups.contains(&"slurm_feature_sse4".to_string()));
        assert!(groups.contains(&"slurm_gres_gpu".to_string()));
    }

    #[test]
    fn test_resolve_keyed_group_key() {
        let config = PluginConfig::new("slurm");
        let plugin = SlurmPlugin::new(config).unwrap();

        let node = SlurmPlugin::parse_node_line(sample_scontrol_line()).unwrap();

        assert_eq!(
            plugin.resolve_keyed_group_key("slurm_state", &node),
            Some("idle".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("slurm_partition", &node),
            Some("compute".to_string())
        );
        assert_eq!(
            plugin.resolve_keyed_group_key("cpus", &node),
            Some("32".to_string())
        );
    }

    #[test]
    fn test_resolve_compose_expression() {
        let config = PluginConfig::new("slurm");
        let plugin = SlurmPlugin::new(config).unwrap();

        let node = SlurmPlugin::parse_node_line(sample_scontrol_line()).unwrap();

        assert_eq!(
            plugin.resolve_compose_expression("slurm_node_addr", &node),
            Some("10.0.1.1".to_string())
        );
        assert_eq!(
            plugin.resolve_compose_expression("node_name", &node),
            Some("node001".to_string())
        );
    }

    #[test]
    fn test_nodes_to_inventory() {
        let config = PluginConfig::new("slurm");
        let plugin = SlurmPlugin::new(config).unwrap();

        let nodes = vec![
            SlurmPlugin::parse_node_line(sample_scontrol_line()).unwrap(),
            SlurmPlugin::parse_node_line(
                "NodeName=node002 CPUTot=64 RealMemory=512000 State=ALLOCATED \
                 Partitions=compute Gres=(null) AvailableFeatures=avx2",
            )
            .unwrap(),
        ];

        let inventory = plugin.nodes_to_inventory(nodes).unwrap();

        assert_eq!(inventory.host_count(), 2);
        assert!(inventory.get_host("node001").is_some());
        assert!(inventory.get_host("node002").is_some());

        // Verify host variables
        let host = inventory.get_host("node001").unwrap();
        assert_eq!(
            host.vars.get("slurm_state"),
            Some(&serde_yaml::Value::String("idle".to_string()))
        );
        assert_eq!(
            host.vars.get("slurm_cpus"),
            Some(&serde_yaml::Value::Number(serde_yaml::Number::from(32u64)))
        );
    }
}
