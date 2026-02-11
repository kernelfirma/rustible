//! Docker Network module - Network management
//!
//! This module manages Docker networks using the bollard crate.
//! It supports creating, configuring, connecting containers, and removing networks.
//!
//! ## Parameters
//!
//! - `name`: Network name (required)
//! - `state`: Desired state (present, absent)
//! - `driver`: Network driver (bridge, overlay, host, none, macvlan)
//! - `driver_options`: Driver-specific options
//! - `ipam`: IP Address Management configuration
//!   - `driver`: IPAM driver (default: default)
//!   - `config`: IPAM config (subnet, gateway, ip_range)
//! - `internal`: Restrict external access to network
//! - `attachable`: Enable manual container attachment (for overlay networks)
//! - `scope`: Network scope (local, global, swarm)
//! - `labels`: Network labels
//! - `enable_ipv6`: Enable IPv6 on the network
//! - `connected`: List of containers to connect
//! - `force`: Force removal even if containers are connected

#[cfg(feature = "docker")]
use bollard::models::{Ipam, IpamConfig, NetworkCreateResponse};
#[cfg(feature = "docker")]
use bollard::network::{
    ConnectNetworkOptions, CreateNetworkOptions, DisconnectNetworkOptions, ListNetworksOptions,
};
#[cfg(feature = "docker")]
use bollard::Docker;

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Desired state for a network
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkState {
    /// Network should exist
    Present,
    /// Network should not exist
    Absent,
}

impl NetworkState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(NetworkState::Present),
            "absent" => Ok(NetworkState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Network driver type
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NetworkDriver {
    #[default]
    Bridge,
    Overlay,
    Host,
    None,
    Macvlan,
    Custom(String),
}

impl NetworkDriver {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bridge" => NetworkDriver::Bridge,
            "overlay" => NetworkDriver::Overlay,
            "host" => NetworkDriver::Host,
            "none" => NetworkDriver::None,
            "macvlan" => NetworkDriver::Macvlan,
            other => NetworkDriver::Custom(other.to_string()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            NetworkDriver::Bridge => "bridge",
            NetworkDriver::Overlay => "overlay",
            NetworkDriver::Host => "host",
            NetworkDriver::None => "none",
            NetworkDriver::Macvlan => "macvlan",
            NetworkDriver::Custom(s) => s,
        }
    }
}

/// IPAM configuration
#[derive(Debug, Clone, Default)]
pub struct IpamConfiguration {
    /// IPAM driver
    pub driver: String,
    /// Subnet configurations
    pub config: Vec<SubnetConfig>,
}

/// Subnet configuration
#[derive(Debug, Clone)]
pub struct SubnetConfig {
    /// Subnet in CIDR notation (e.g., "172.20.0.0/16")
    pub subnet: String,
    /// Gateway IP address
    pub gateway: Option<String>,
    /// IP range for allocation
    pub ip_range: Option<String>,
}

/// Network configuration parsed from parameters
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Network name
    pub name: String,
    /// Desired state
    pub state: NetworkState,
    /// Network driver
    pub driver: NetworkDriver,
    /// Driver-specific options
    pub driver_options: HashMap<String, String>,
    /// IPAM configuration
    pub ipam: Option<IpamConfiguration>,
    /// Internal network (no external access)
    pub internal: bool,
    /// Attachable for overlay networks
    pub attachable: bool,
    /// Network scope
    pub scope: Option<String>,
    /// Network labels
    pub labels: HashMap<String, String>,
    /// Enable IPv6
    pub enable_ipv6: bool,
    /// Containers to connect
    pub connected: Vec<String>,
    /// Force removal
    pub force: bool,
}

impl NetworkConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            NetworkState::from_str(&s)?
        } else {
            NetworkState::Present
        };

        let driver = if let Some(d) = params.get_string("driver")? {
            NetworkDriver::from_str(&d)
        } else {
            NetworkDriver::default()
        };

        // Parse driver options
        let mut driver_options = HashMap::new();
        if let Some(serde_json::Value::Object(obj)) = params.get("driver_options") {
            for (k, v) in obj {
                if let serde_json::Value::String(val) = v {
                    driver_options.insert(k.clone(), val.clone());
                }
            }
        }

        // Parse IPAM configuration
        let ipam = if let Some(serde_json::Value::Object(obj)) = params.get("ipam") {
            let driver = obj
                .get("driver")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();

            let mut config = Vec::new();
            if let Some(serde_json::Value::Array(configs)) = obj.get("config") {
                for cfg in configs {
                    if let serde_json::Value::Object(cfg_obj) = cfg {
                        let subnet = cfg_obj
                            .get("subnet")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !subnet.is_empty() {
                            config.push(SubnetConfig {
                                subnet,
                                gateway: cfg_obj
                                    .get("gateway")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                ip_range: cfg_obj
                                    .get("ip_range")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            });
                        }
                    }
                }
            }

            Some(IpamConfiguration { driver, config })
        } else {
            None
        };

        // Parse labels
        let mut labels = HashMap::new();
        if let Some(serde_json::Value::Object(obj)) = params.get("labels") {
            for (k, v) in obj {
                if let serde_json::Value::String(val) = v {
                    labels.insert(k.clone(), val.clone());
                }
            }
        }

        // Parse connected containers
        let connected = params.get_vec_string("connected")?.unwrap_or_default();

        Ok(Self {
            name,
            state,
            driver,
            driver_options,
            ipam,
            internal: params.get_bool_or("internal", false),
            attachable: params.get_bool_or("attachable", false),
            scope: params.get_string("scope")?,
            labels,
            enable_ipv6: params.get_bool_or("enable_ipv6", false),
            connected,
            force: params.get_bool_or("force", false),
        })
    }
}

/// Docker Network module
pub struct DockerNetworkModule;

#[cfg(feature = "docker")]
impl DockerNetworkModule {
    /// Connect to Docker daemon
    async fn connect_docker() -> ModuleResult<Docker> {
        Docker::connect_with_local_defaults().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to connect to Docker: {}", e))
        })
    }

    /// Check if network exists
    async fn network_exists(docker: &Docker, name: &str) -> ModuleResult<bool> {
        match docker.inspect_network::<&str>(name, None).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(ModuleError::ExecutionFailed(format!(
                "Failed to inspect network: {}",
                e
            ))),
        }
    }

    /// Get network info
    async fn get_network_info(docker: &Docker, name: &str) -> ModuleResult<serde_json::Value> {
        match docker.inspect_network::<&str>(name, None).await {
            Ok(info) => Ok(serde_json::json!({
                "id": info.id,
                "name": info.name,
                "driver": info.driver,
                "scope": info.scope,
                "internal": info.internal,
                "attachable": info.attachable,
                "ipam": info.ipam,
                "containers": info.containers,
            })),
            Err(_) => Ok(serde_json::json!({
                "exists": false,
            })),
        }
    }

    /// Create network
    async fn create_network(docker: &Docker, config: &NetworkConfig) -> ModuleResult<String> {
        // Build IPAM config
        let ipam = config.ipam.as_ref().map(|ipam_config| {
            let configs: Vec<IpamConfig> = ipam_config
                .config
                .iter()
                .map(|c| IpamConfig {
                    subnet: Some(c.subnet.clone()),
                    gateway: c.gateway.clone(),
                    ip_range: c.ip_range.clone(),
                    ..Default::default()
                })
                .collect();

            Ipam {
                driver: Some(ipam_config.driver.clone()),
                config: Some(configs),
                ..Default::default()
            }
        });

        let options = CreateNetworkOptions {
            name: config.name.clone(),
            driver: config.driver.as_str().to_string(),
            options: config.driver_options.clone(),
            ipam: ipam.unwrap_or_default(),
            internal: config.internal,
            attachable: config.attachable,
            labels: config.labels.clone(),
            enable_ipv6: config.enable_ipv6,
            ..Default::default()
        };

        let response = docker.create_network(options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create network: {}", e))
        })?;

        Ok(response.id.unwrap_or_default())
    }

    /// Remove network
    async fn remove_network(docker: &Docker, name: &str) -> ModuleResult<()> {
        docker
            .remove_network(name)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to remove network: {}", e)))
    }

    /// Connect container to network
    async fn connect_container(
        docker: &Docker,
        network: &str,
        container: &str,
    ) -> ModuleResult<()> {
        let options = ConnectNetworkOptions {
            container,
            ..Default::default()
        };

        docker.connect_network(network, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to connect container '{}' to network '{}': {}",
                container, network, e
            ))
        })
    }

    /// Disconnect container from network
    async fn disconnect_container(
        docker: &Docker,
        network: &str,
        container: &str,
        force: bool,
    ) -> ModuleResult<()> {
        let options = DisconnectNetworkOptions { container, force };

        docker
            .disconnect_network(network, options)
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to disconnect container '{}' from network '{}': {}",
                    container, network, e
                ))
            })
    }

    /// Get containers connected to network
    async fn get_connected_containers(docker: &Docker, name: &str) -> ModuleResult<Vec<String>> {
        match docker.inspect_network::<&str>(name, None).await {
            Ok(info) => {
                let containers = info
                    .containers
                    .map(|c| c.keys().cloned().collect())
                    .unwrap_or_default();
                Ok(containers)
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = NetworkConfig::from_params(params)?;
        let docker = Self::connect_docker().await?;

        let exists = Self::network_exists(&docker, &config.name).await?;
        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            NetworkState::Absent => {
                if exists {
                    if context.check_mode {
                        messages.push(format!("Would remove network '{}'", config.name));
                        changed = true;
                    } else {
                        // Disconnect all containers if force is set
                        if config.force {
                            let connected =
                                Self::get_connected_containers(&docker, &config.name).await?;
                            for container in connected {
                                Self::disconnect_container(&docker, &config.name, &container, true)
                                    .await?;
                            }
                        }
                        Self::remove_network(&docker, &config.name).await?;
                        messages.push(format!("Removed network '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Network '{}' does not exist", config.name));
                }
            }

            NetworkState::Present => {
                if !exists {
                    if context.check_mode {
                        messages.push(format!("Would create network '{}'", config.name));
                        changed = true;
                    } else {
                        Self::create_network(&docker, &config).await?;
                        messages.push(format!("Created network '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Network '{}' already exists", config.name));
                }

                // Handle container connections
                if !config.connected.is_empty() && (exists || !context.check_mode) {
                    let currently_connected =
                        Self::get_connected_containers(&docker, &config.name).await?;

                    for container in &config.connected {
                        if !currently_connected.contains(container) {
                            if context.check_mode {
                                messages.push(format!(
                                    "Would connect container '{}' to network",
                                    container
                                ));
                                changed = true;
                            } else {
                                Self::connect_container(&docker, &config.name, container).await?;
                                messages.push(format!(
                                    "Connected container '{}' to network",
                                    container
                                ));
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Get network info for output
        let network_info = Self::get_network_info(&docker, &config.name).await?;

        let msg = if messages.is_empty() {
            format!("Network '{}' is in desired state", config.name)
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("network", network_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("network", network_info))
        }
    }
}

#[cfg(not(feature = "docker"))]
impl DockerNetworkModule {
    fn execute_stub(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        Err(ModuleError::Unsupported(
            "Docker module requires 'docker' feature to be enabled".to_string(),
        ))
    }
}

impl Module for DockerNetworkModule {
    fn name(&self) -> &'static str {
        "docker_network"
    }

    fn description(&self) -> &'static str {
        "Manage Docker networks"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        #[cfg(feature = "docker")]
        {
            let rt = tokio::runtime::Handle::try_current().map_err(|_| {
                ModuleError::ExecutionFailed("No tokio runtime available".to_string())
            })?;

            let params = params.clone();
            let context = context.clone();
            std::thread::scope(|s| {
                s.spawn(|| rt.block_on(self.execute_async(&params, &context)))
                    .join()
                    .unwrap()
            })
        }

        #[cfg(not(feature = "docker"))]
        {
            self.execute_stub(params, context)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_state_from_str() {
        assert_eq!(
            NetworkState::from_str("present").unwrap(),
            NetworkState::Present
        );
        assert_eq!(
            NetworkState::from_str("absent").unwrap(),
            NetworkState::Absent
        );
        assert!(NetworkState::from_str("invalid").is_err());
    }

    #[test]
    fn test_network_driver_from_str() {
        assert_eq!(NetworkDriver::from_str("bridge"), NetworkDriver::Bridge);
        assert_eq!(NetworkDriver::from_str("overlay"), NetworkDriver::Overlay);
        assert_eq!(NetworkDriver::from_str("host"), NetworkDriver::Host);
        assert_eq!(NetworkDriver::from_str("none"), NetworkDriver::None);
        assert_eq!(NetworkDriver::from_str("macvlan"), NetworkDriver::Macvlan);
        assert!(matches!(
            NetworkDriver::from_str("custom_driver"),
            NetworkDriver::Custom(_)
        ));
    }

    #[test]
    fn test_module_metadata() {
        let module = DockerNetworkModule;
        assert_eq!(module.name(), "docker_network");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
