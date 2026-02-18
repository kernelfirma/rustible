//! Docker Container module - Container lifecycle management
//!
//! This module manages Docker containers using the bollard crate.
//! It supports creating, starting, stopping, restarting, and removing containers.
//!
//! ## Parameters
//!
//! - `name`: Container name (required)
//! - `image`: Docker image to use (required for state=present/started)
//! - `state`: Desired state (present, absent, started, stopped, restarted)
//! - `command`: Command to run in the container
//! - `entrypoint`: Override the default entrypoint
//! - `env`: Environment variables (key=value pairs)
//! - `ports`: Port mappings (host:container format)
//! - `volumes`: Volume mounts (host:container format)
//! - `network`: Network to connect to
//! - `restart_policy`: Restart policy (no, always, on-failure, unless-stopped)
//! - `pull`: Whether to pull the image (always, missing, never)
//! - `recreate`: Recreate container if config changed
//! - `remove_volumes`: Remove volumes when removing container
//! - `force_kill`: Use SIGKILL instead of SIGTERM
//! - `stop_timeout`: Timeout in seconds before SIGKILL
//! - `labels`: Container labels
//! - `hostname`: Container hostname
//! - `user`: User to run as in the container
//! - `working_dir`: Working directory inside the container
//! - `memory`: Memory limit (e.g., "512m", "1g")
//! - `cpus`: CPU limit (e.g., 0.5, 2.0)
//! - `privileged`: Run container in privileged mode
//! - `read_only`: Mount root filesystem as read-only
//! - `capabilities_add`: Linux capabilities to add
//! - `capabilities_drop`: Linux capabilities to drop

#[cfg(feature = "docker")]
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions,
    RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
#[cfg(feature = "docker")]
use bollard::models::{
    ContainerInspectResponse, HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum,
};
#[cfg(feature = "docker")]
use bollard::Docker;

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Desired state for a container
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    /// Container should exist (created but not necessarily running)
    Present,
    /// Container should not exist
    Absent,
    /// Container should be running
    Started,
    /// Container should be stopped
    Stopped,
    /// Container should be restarted
    Restarted,
}

impl ContainerState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(ContainerState::Present),
            "absent" => Ok(ContainerState::Absent),
            "started" | "running" => Ok(ContainerState::Started),
            "stopped" => Ok(ContainerState::Stopped),
            "restarted" => Ok(ContainerState::Restarted),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, started, stopped, restarted",
                s
            ))),
        }
    }
}

/// Image pull policy
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PullPolicy {
    /// Always pull the image
    Always,
    /// Pull only if image is missing (default)
    #[default]
    Missing,
    /// Never pull the image
    Never,
}

impl PullPolicy {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "always" => Ok(PullPolicy::Always),
            "missing" | "if_not_present" => Ok(PullPolicy::Missing),
            "never" => Ok(PullPolicy::Never),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid pull policy '{}'. Valid values: always, missing, never",
                s
            ))),
        }
    }
}

/// Container configuration parsed from parameters
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container name
    pub name: String,
    /// Docker image
    pub image: Option<String>,
    /// Desired state
    pub state: ContainerState,
    /// Command to run
    pub command: Option<Vec<String>>,
    /// Entrypoint override
    pub entrypoint: Option<Vec<String>>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Port mappings (container_port -> host_port)
    pub ports: HashMap<String, String>,
    /// Volume mounts
    pub volumes: Vec<String>,
    /// Network name
    pub network: Option<String>,
    /// Restart policy
    pub restart_policy: Option<String>,
    /// Pull policy
    pub pull: PullPolicy,
    /// Recreate if config changed
    pub recreate: bool,
    /// Remove volumes on removal
    pub remove_volumes: bool,
    /// Force kill instead of graceful stop
    pub force_kill: bool,
    /// Stop timeout in seconds
    pub stop_timeout: Option<i64>,
    /// Container labels
    pub labels: HashMap<String, String>,
    /// Container hostname
    pub hostname: Option<String>,
    /// User to run as
    pub user: Option<String>,
    /// Working directory
    pub working_dir: Option<String>,
    /// Memory limit in bytes
    pub memory: Option<i64>,
    /// CPU limit
    pub cpus: Option<f64>,
    /// Privileged mode
    pub privileged: bool,
    /// Read-only root filesystem
    pub read_only: bool,
    /// Capabilities to add
    pub capabilities_add: Vec<String>,
    /// Capabilities to drop
    pub capabilities_drop: Vec<String>,
}

impl ContainerConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let image = params.get_string("image")?;
        let state = if let Some(s) = params.get_string("state")? {
            ContainerState::from_str(&s)?
        } else {
            ContainerState::Started
        };

        // Parse command - can be string or array
        let command =
            if let Some(cmd) = params.get_string("command")? {
                Some(shell_words::split(&cmd).map_err(|e| {
                    ModuleError::InvalidParameter(format!("Invalid command: {}", e))
                })?)
            } else {
                params.get_vec_string("command")?
            };

        // Parse entrypoint
        let entrypoint =
            if let Some(ep) = params.get_string("entrypoint")? {
                Some(shell_words::split(&ep).map_err(|e| {
                    ModuleError::InvalidParameter(format!("Invalid entrypoint: {}", e))
                })?)
            } else {
                params.get_vec_string("entrypoint")?
            };

        // Parse environment variables
        let env = parse_env_vars(params)?;

        // Parse ports
        let ports = parse_port_mappings(params)?;

        // Parse volumes
        let volumes = params.get_vec_string("volumes")?.unwrap_or_default();

        // Parse labels
        let labels = parse_labels(params)?;

        // Parse pull policy
        let pull = if let Some(p) = params.get_string("pull")? {
            PullPolicy::from_str(&p)?
        } else {
            PullPolicy::default()
        };

        // Parse memory limit
        let memory = if let Some(m) = params.get_string("memory")? {
            Some(parse_memory_string(&m)?)
        } else {
            None
        };

        // Parse capabilities
        let capabilities_add = params
            .get_vec_string("capabilities_add")?
            .unwrap_or_default();
        let capabilities_drop = params
            .get_vec_string("capabilities_drop")?
            .unwrap_or_default();

        Ok(Self {
            name,
            image,
            state,
            command,
            entrypoint,
            env,
            ports,
            volumes,
            network: params.get_string("network")?,
            restart_policy: params.get_string("restart_policy")?,
            pull,
            recreate: params.get_bool_or("recreate", false),
            remove_volumes: params.get_bool_or("remove_volumes", false),
            force_kill: params.get_bool_or("force_kill", false),
            stop_timeout: params.get_i64("stop_timeout")?,
            labels,
            hostname: params.get_string("hostname")?,
            user: params.get_string("user")?,
            working_dir: params.get_string("working_dir")?,
            memory,
            cpus: params.get_string("cpus")?.and_then(|s| s.parse().ok()),
            privileged: params.get_bool_or("privileged", false),
            read_only: params.get_bool_or("read_only", false),
            capabilities_add,
            capabilities_drop,
        })
    }
}

/// Parse environment variables from params
fn parse_env_vars(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut env = HashMap::new();

    if let Some(serde_json::Value::Object(obj)) = params.get("env") {
        for (key, value) in obj {
            if let serde_json::Value::String(v) = value {
                env.insert(key.clone(), v.clone());
            } else {
                env.insert(key.clone(), value.to_string());
            }
        }
    } else if let Some(arr) = params.get_vec_string("env")? {
        for item in arr {
            if let Some((key, value)) = item.split_once('=') {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(env)
}

/// Parse port mappings from params
fn parse_port_mappings(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut ports = HashMap::new();

    if let Some(serde_json::Value::Object(obj)) = params.get("ports") {
        for (container_port, host_port) in obj {
            if let serde_json::Value::String(hp) = host_port {
                ports.insert(container_port.clone(), hp.clone());
            } else if let serde_json::Value::Number(n) = host_port {
                ports.insert(container_port.clone(), n.to_string());
            }
        }
    } else if let Some(arr) = params.get_vec_string("ports")? {
        for item in arr {
            // Format: host_port:container_port or container_port
            if let Some((host, container)) = item.split_once(':') {
                ports.insert(container.to_string(), host.to_string());
            } else {
                ports.insert(item.clone(), item);
            }
        }
    }

    Ok(ports)
}

/// Parse labels from params
fn parse_labels(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut labels = HashMap::new();

    if let Some(serde_json::Value::Object(obj)) = params.get("labels") {
        for (key, value) in obj {
            if let serde_json::Value::String(v) = value {
                labels.insert(key.clone(), v.clone());
            } else {
                labels.insert(key.clone(), value.to_string());
            }
        }
    }

    Ok(labels)
}

/// Parse memory string (e.g., "512m", "1g") to bytes
fn parse_memory_string(s: &str) -> ModuleResult<i64> {
    let s = s.trim().to_lowercase();
    let (num, unit) = if s.ends_with("gb") || s.ends_with('g') {
        let num_str = s.trim_end_matches("gb").trim_end_matches('g');
        (num_str, 1024 * 1024 * 1024i64)
    } else if s.ends_with("mb") || s.ends_with('m') {
        let num_str = s.trim_end_matches("mb").trim_end_matches('m');
        (num_str, 1024 * 1024i64)
    } else if s.ends_with("kb") || s.ends_with('k') {
        let num_str = s.trim_end_matches("kb").trim_end_matches('k');
        (num_str, 1024i64)
    } else if s.ends_with('b') {
        let num_str = s.trim_end_matches('b');
        (num_str, 1i64)
    } else {
        (s.as_str(), 1i64)
    };

    let value: f64 = num
        .parse()
        .map_err(|_| ModuleError::InvalidParameter(format!("Invalid memory value: {}", s)))?;

    Ok((value * unit as f64) as i64)
}

/// Docker Container module
pub struct DockerContainerModule;

#[cfg(feature = "docker")]
impl DockerContainerModule {
    /// Connect to Docker daemon
    async fn connect_docker() -> ModuleResult<Docker> {
        Docker::connect_with_local_defaults().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to connect to Docker: {}", e))
        })
    }

    /// Get container by name
    async fn get_container(
        docker: &Docker,
        name: &str,
    ) -> ModuleResult<Option<ContainerInspectResponse>> {
        match docker.inspect_container(name, None).await {
            Ok(info) => Ok(Some(info)),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(e) => Err(ModuleError::ExecutionFailed(format!(
                "Failed to inspect container: {}",
                e
            ))),
        }
    }

    /// Check if container is running
    fn is_running(container: &ContainerInspectResponse) -> bool {
        container
            .state
            .as_ref()
            .and_then(|s| s.running)
            .unwrap_or(false)
    }

    /// Pull image if needed
    async fn ensure_image(docker: &Docker, image: &str, policy: &PullPolicy) -> ModuleResult<bool> {
        use bollard::image::CreateImageOptions;
        use futures::StreamExt;

        let should_pull = match policy {
            PullPolicy::Always => true,
            PullPolicy::Never => false,
            PullPolicy::Missing => {
                // Check if image exists
                (docker.inspect_image(image).await).is_err()
            }
        };

        if should_pull {
            let options = CreateImageOptions {
                from_image: image,
                ..Default::default()
            };

            let mut stream = docker.create_image(Some(options), None, None);
            while let Some(result) = stream.next().await {
                result.map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to pull image: {}", e))
                })?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Create container
    async fn create_container(docker: &Docker, config: &ContainerConfig) -> ModuleResult<String> {
        let image = config.image.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("image is required for creating containers".to_string())
        })?;

        // Build environment variables
        let env: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Build port bindings
        let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();

        for (container_port, host_port) in &config.ports {
            let port_key = if container_port.contains('/') {
                container_port.clone()
            } else {
                format!("{}/tcp", container_port)
            };

            exposed_ports.insert(port_key.clone(), HashMap::new());
            port_bindings.insert(
                port_key,
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(host_port.clone()),
                }]),
            );
        }

        // Build restart policy
        let restart_policy = config.restart_policy.as_ref().map(|p| {
            let name = match p.as_str() {
                "always" => RestartPolicyNameEnum::ALWAYS,
                "unless-stopped" => RestartPolicyNameEnum::UNLESS_STOPPED,
                "on-failure" => RestartPolicyNameEnum::ON_FAILURE,
                _ => RestartPolicyNameEnum::NO,
            };
            RestartPolicy {
                name: Some(name),
                maximum_retry_count: None,
            }
        });

        // Build host config
        let host_config = HostConfig {
            binds: if config.volumes.is_empty() {
                None
            } else {
                Some(config.volumes.clone())
            },
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            restart_policy,
            memory: config.memory,
            nano_cpus: config.cpus.map(|c| (c * 1_000_000_000.0) as i64),
            privileged: Some(config.privileged),
            readonly_rootfs: Some(config.read_only),
            cap_add: if config.capabilities_add.is_empty() {
                None
            } else {
                Some(config.capabilities_add.clone())
            },
            cap_drop: if config.capabilities_drop.is_empty() {
                None
            } else {
                Some(config.capabilities_drop.clone())
            },
            network_mode: config.network.clone(),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(image.clone()),
            cmd: config.command.clone(),
            entrypoint: config.entrypoint.clone(),
            env: if env.is_empty() { None } else { Some(env) },
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            labels: if config.labels.is_empty() {
                None
            } else {
                Some(config.labels.clone())
            },
            hostname: config.hostname.clone(),
            user: config.user.clone(),
            working_dir: config.working_dir.clone(),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: config.name.as_str(),
            platform: None,
        };

        let response = docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create container: {}", e))
            })?;

        Ok(response.id)
    }

    /// Start container
    async fn start_container(docker: &Docker, name: &str) -> ModuleResult<()> {
        docker
            .start_container(name, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to start container: {}", e)))
    }

    /// Stop container
    async fn stop_container(docker: &Docker, name: &str, timeout: Option<i64>) -> ModuleResult<()> {
        let options = StopContainerOptions {
            t: timeout.unwrap_or(10),
        };
        docker
            .stop_container(name, Some(options))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to stop container: {}", e)))
    }

    /// Restart container
    async fn restart_container(
        docker: &Docker,
        name: &str,
        timeout: Option<i64>,
    ) -> ModuleResult<()> {
        let options = RestartContainerOptions {
            t: timeout.unwrap_or(10) as isize,
        };
        docker
            .restart_container(name, Some(options))
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to restart container: {}", e))
            })
    }

    /// Remove container
    async fn remove_container(
        docker: &Docker,
        name: &str,
        force: bool,
        volumes: bool,
    ) -> ModuleResult<()> {
        let options = RemoveContainerOptions {
            force,
            v: volumes,
            ..Default::default()
        };
        docker
            .remove_container(name, Some(options))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to remove container: {}", e)))
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ContainerConfig::from_params(params)?;
        let docker = Self::connect_docker().await?;

        let existing = Self::get_container(&docker, &config.name).await?;
        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            ContainerState::Absent => {
                if let Some(container) = existing {
                    if context.check_mode {
                        messages.push(format!("Would remove container '{}'", config.name));
                        changed = true;
                    } else {
                        // Stop if running
                        if Self::is_running(&container) {
                            Self::stop_container(&docker, &config.name, config.stop_timeout)
                                .await?;
                        }
                        Self::remove_container(
                            &docker,
                            &config.name,
                            config.force_kill,
                            config.remove_volumes,
                        )
                        .await?;
                        messages.push(format!("Removed container '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Container '{}' does not exist", config.name));
                }
            }

            ContainerState::Present => {
                if existing.is_none() {
                    if context.check_mode {
                        messages.push(format!("Would create container '{}'", config.name));
                        changed = true;
                    } else {
                        // Pull image if needed
                        if let Some(ref image) = config.image {
                            if Self::ensure_image(&docker, image, &config.pull).await? {
                                messages.push(format!("Pulled image '{}'", image));
                            }
                        }
                        Self::create_container(&docker, &config).await?;
                        messages.push(format!("Created container '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Container '{}' already exists", config.name));
                }
            }

            ContainerState::Started => {
                if let Some(container) = existing {
                    if !Self::is_running(&container) {
                        if context.check_mode {
                            messages.push(format!("Would start container '{}'", config.name));
                            changed = true;
                        } else {
                            Self::start_container(&docker, &config.name).await?;
                            messages.push(format!("Started container '{}'", config.name));
                            changed = true;
                        }
                    } else {
                        messages.push(format!("Container '{}' is already running", config.name));
                    }
                } else if context.check_mode {
                    messages.push(format!(
                        "Would create and start container '{}'",
                        config.name
                    ));
                    changed = true;
                } else {
                    // Pull image if needed
                    if let Some(ref image) = config.image {
                        if Self::ensure_image(&docker, image, &config.pull).await? {
                            messages.push(format!("Pulled image '{}'", image));
                        }
                    }
                    Self::create_container(&docker, &config).await?;
                    Self::start_container(&docker, &config.name).await?;
                    messages.push(format!("Created and started container '{}'", config.name));
                    changed = true;
                }
            }

            ContainerState::Stopped => {
                if let Some(container) = existing {
                    if Self::is_running(&container) {
                        if context.check_mode {
                            messages.push(format!("Would stop container '{}'", config.name));
                            changed = true;
                        } else {
                            Self::stop_container(&docker, &config.name, config.stop_timeout)
                                .await?;
                            messages.push(format!("Stopped container '{}'", config.name));
                            changed = true;
                        }
                    } else {
                        messages.push(format!("Container '{}' is already stopped", config.name));
                    }
                } else {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Container '{}' does not exist",
                        config.name
                    )));
                }
            }

            ContainerState::Restarted => {
                if existing.is_some() {
                    if context.check_mode {
                        messages.push(format!("Would restart container '{}'", config.name));
                        changed = true;
                    } else {
                        Self::restart_container(&docker, &config.name, config.stop_timeout).await?;
                        messages.push(format!("Restarted container '{}'", config.name));
                        changed = true;
                    }
                } else {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Container '{}' does not exist",
                        config.name
                    )));
                }
            }
        }

        // Get final container state for output
        let final_container = Self::get_container(&docker, &config.name).await?;
        let container_info = if let Some(c) = final_container {
            serde_json::json!({
                "id": c.id,
                "name": config.name,
                "running": Self::is_running(&c),
                "image": c.config.as_ref().and_then(|cfg| cfg.image.clone()),
            })
        } else {
            serde_json::json!({
                "name": config.name,
                "exists": false,
            })
        };

        let msg = if messages.is_empty() {
            format!("Container '{}' is in desired state", config.name)
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("container", container_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("container", container_info))
        }
    }
}

#[cfg(not(feature = "docker"))]
impl DockerContainerModule {
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

impl Module for DockerContainerModule {
    fn name(&self) -> &'static str {
        "docker_container"
    }

    fn description(&self) -> &'static str {
        "Manage Docker containers"
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
    fn test_container_state_from_str() {
        assert_eq!(
            ContainerState::from_str("present").unwrap(),
            ContainerState::Present
        );
        assert_eq!(
            ContainerState::from_str("absent").unwrap(),
            ContainerState::Absent
        );
        assert_eq!(
            ContainerState::from_str("started").unwrap(),
            ContainerState::Started
        );
        assert_eq!(
            ContainerState::from_str("running").unwrap(),
            ContainerState::Started
        );
        assert_eq!(
            ContainerState::from_str("stopped").unwrap(),
            ContainerState::Stopped
        );
        assert_eq!(
            ContainerState::from_str("restarted").unwrap(),
            ContainerState::Restarted
        );
        assert!(ContainerState::from_str("invalid").is_err());
    }

    #[test]
    fn test_pull_policy_from_str() {
        assert_eq!(PullPolicy::from_str("always").unwrap(), PullPolicy::Always);
        assert_eq!(
            PullPolicy::from_str("missing").unwrap(),
            PullPolicy::Missing
        );
        assert_eq!(PullPolicy::from_str("never").unwrap(), PullPolicy::Never);
        assert!(PullPolicy::from_str("invalid").is_err());
    }

    #[test]
    fn test_parse_memory_string() {
        assert_eq!(parse_memory_string("512m").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_memory_string("1g").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_memory_string("1024k").unwrap(), 1024 * 1024);
        assert_eq!(parse_memory_string("1000").unwrap(), 1000);
    }

    #[test]
    fn test_module_metadata() {
        let module = DockerContainerModule;
        assert_eq!(module.name(), "docker_container");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
