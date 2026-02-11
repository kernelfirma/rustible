//! Docker Volume module - Volume management
//!
//! This module manages Docker volumes using the bollard crate.
//! It supports creating, inspecting, and removing volumes.
//!
//! ## Parameters
//!
//! - `name`: Volume name (required)
//! - `state`: Desired state (present, absent)
//! - `driver`: Volume driver (default: local)
//! - `driver_options`: Driver-specific options
//! - `labels`: Volume labels
//! - `force`: Force removal even if volume is in use
//! - `recreate`: Recreate volume if exists (will remove and recreate)

#[cfg(feature = "docker")]
use bollard::volume::{CreateVolumeOptions, ListVolumesOptions, RemoveVolumeOptions};
#[cfg(feature = "docker")]
use bollard::Docker;

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Desired state for a volume
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeState {
    /// Volume should exist
    Present,
    /// Volume should not exist
    Absent,
}

impl VolumeState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(VolumeState::Present),
            "absent" => Ok(VolumeState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Volume configuration parsed from parameters
#[derive(Debug, Clone)]
pub struct VolumeConfig {
    /// Volume name
    pub name: String,
    /// Desired state
    pub state: VolumeState,
    /// Volume driver
    pub driver: String,
    /// Driver-specific options
    pub driver_options: HashMap<String, String>,
    /// Volume labels
    pub labels: HashMap<String, String>,
    /// Force removal
    pub force: bool,
    /// Recreate volume
    pub recreate: bool,
}

impl VolumeConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            VolumeState::from_str(&s)?
        } else {
            VolumeState::Present
        };

        let driver = params
            .get_string("driver")?
            .unwrap_or_else(|| "local".to_string());

        // Parse driver options
        let mut driver_options = HashMap::new();
        if let Some(serde_json::Value::Object(obj)) = params.get("driver_options") {
            for (k, v) in obj {
                if let serde_json::Value::String(val) = v {
                    driver_options.insert(k.clone(), val.clone());
                }
            }
        }

        // Parse labels
        let mut labels = HashMap::new();
        if let Some(serde_json::Value::Object(obj)) = params.get("labels") {
            for (k, v) in obj {
                if let serde_json::Value::String(val) = v {
                    labels.insert(k.clone(), val.clone());
                }
            }
        }

        Ok(Self {
            name,
            state,
            driver,
            driver_options,
            labels,
            force: params.get_bool_or("force", false),
            recreate: params.get_bool_or("recreate", false),
        })
    }
}

/// Docker Volume module
pub struct DockerVolumeModule;

#[cfg(feature = "docker")]
impl DockerVolumeModule {
    /// Connect to Docker daemon
    async fn connect_docker() -> ModuleResult<Docker> {
        Docker::connect_with_local_defaults().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to connect to Docker: {}", e))
        })
    }

    /// Check if volume exists
    async fn volume_exists(docker: &Docker, name: &str) -> ModuleResult<bool> {
        match docker.inspect_volume(name).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(ModuleError::ExecutionFailed(format!(
                "Failed to inspect volume: {}",
                e
            ))),
        }
    }

    /// Get volume info
    async fn get_volume_info(docker: &Docker, name: &str) -> ModuleResult<serde_json::Value> {
        match docker.inspect_volume(name).await {
            Ok(info) => Ok(serde_json::json!({
                "name": info.name,
                "driver": info.driver,
                "mountpoint": info.mountpoint,
                "scope": info.scope,
                "labels": info.labels,
                "options": info.options,
                "created_at": info.created_at,
            })),
            Err(_) => Ok(serde_json::json!({
                "exists": false,
            })),
        }
    }

    /// Create volume
    async fn create_volume(docker: &Docker, config: &VolumeConfig) -> ModuleResult<()> {
        let options = CreateVolumeOptions {
            name: config.name.clone(),
            driver: config.driver.clone(),
            driver_opts: config.driver_options.clone(),
            labels: config.labels.clone(),
        };

        docker
            .create_volume(options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to create volume: {}", e)))?;

        Ok(())
    }

    /// Remove volume
    async fn remove_volume(docker: &Docker, name: &str, force: bool) -> ModuleResult<()> {
        let options = RemoveVolumeOptions { force };

        docker
            .remove_volume(name, Some(options))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to remove volume: {}", e)))
    }

    /// List volumes matching a filter
    async fn list_volumes(docker: &Docker, filter: Option<&str>) -> ModuleResult<Vec<String>> {
        let mut filters = HashMap::new();
        if let Some(name) = filter {
            filters.insert("name", vec![name]);
        }

        let options = ListVolumesOptions { filters };

        let response = docker
            .list_volumes(Some(options))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to list volumes: {}", e)))?;

        let volumes = response
            .volumes
            .map(|v| v.into_iter().map(|vol| vol.name).collect())
            .unwrap_or_default();

        Ok(volumes)
    }

    /// Prune unused volumes
    #[allow(dead_code)]
    async fn prune_volumes(docker: &Docker) -> ModuleResult<Vec<String>> {
        let response = docker
            .prune_volumes::<&str>(None)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to prune volumes: {}", e)))?;

        Ok(response.volumes_deleted.unwrap_or_default())
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = VolumeConfig::from_params(params)?;
        let docker = Self::connect_docker().await?;

        let exists = Self::volume_exists(&docker, &config.name).await?;
        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            VolumeState::Absent => {
                if exists {
                    if context.check_mode {
                        messages.push(format!("Would remove volume '{}'", config.name));
                        changed = true;
                    } else {
                        Self::remove_volume(&docker, &config.name, config.force).await?;
                        messages.push(format!("Removed volume '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Volume '{}' does not exist", config.name));
                }
            }

            VolumeState::Present => {
                if exists && config.recreate {
                    // Recreate volume
                    if context.check_mode {
                        messages.push(format!("Would recreate volume '{}'", config.name));
                        changed = true;
                    } else {
                        Self::remove_volume(&docker, &config.name, config.force).await?;
                        Self::create_volume(&docker, &config).await?;
                        messages.push(format!("Recreated volume '{}'", config.name));
                        changed = true;
                    }
                } else if !exists {
                    if context.check_mode {
                        messages.push(format!("Would create volume '{}'", config.name));
                        changed = true;
                    } else {
                        Self::create_volume(&docker, &config).await?;
                        messages.push(format!("Created volume '{}'", config.name));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Volume '{}' already exists", config.name));
                }
            }
        }

        // Get volume info for output
        let volume_info = Self::get_volume_info(&docker, &config.name).await?;

        let msg = if messages.is_empty() {
            format!("Volume '{}' is in desired state", config.name)
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("volume", volume_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("volume", volume_info))
        }
    }
}

#[cfg(not(feature = "docker"))]
impl DockerVolumeModule {
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

impl Module for DockerVolumeModule {
    fn name(&self) -> &'static str {
        "docker_volume"
    }

    fn description(&self) -> &'static str {
        "Manage Docker volumes"
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
    fn test_volume_state_from_str() {
        assert_eq!(
            VolumeState::from_str("present").unwrap(),
            VolumeState::Present
        );
        assert_eq!(
            VolumeState::from_str("absent").unwrap(),
            VolumeState::Absent
        );
        assert!(VolumeState::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_metadata() {
        let module = DockerVolumeModule;
        assert_eq!(module.name(), "docker_volume");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_volume_config_defaults() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test-volume"));

        let config = VolumeConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-volume");
        assert_eq!(config.state, VolumeState::Present);
        assert_eq!(config.driver, "local");
        assert!(!config.force);
        assert!(!config.recreate);
    }
}
