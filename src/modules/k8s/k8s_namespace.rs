//! Kubernetes Namespace module - Namespace resource management
//!
//! This module manages Kubernetes Namespace resources using the kube-rs crate.
//! Namespaces provide a mechanism for isolating groups of resources within a
//! single cluster.
//!
//! ## Parameters
//!
//! - `name`: Namespace name (required)
//! - `state`: Desired state (present, absent) (default: "present")
//! - `labels`: Labels to apply to the Namespace
//! - `annotations`: Annotations to apply to the Namespace
//! - `wait`: Wait for namespace to be active (default: false)
//! - `wait_timeout`: Timeout in seconds for wait (default: 60)
//!
//! ## Example
//!
//! ```yaml
//! - name: Create production namespace
//!   k8s_namespace:
//!     name: production
//!     labels:
//!       environment: production
//!       team: platform
//!     annotations:
//!       description: "Production workloads"
//!     state: present
//! ```

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use std::collections::HashMap;

#[cfg(feature = "kubernetes")]
use k8s_openapi::api::core::v1::Namespace;
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
#[cfg(feature = "kubernetes")]
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client,
};

/// Desired state for a Kubernetes Namespace
#[derive(Debug, Clone, PartialEq)]
pub enum NamespaceState {
    /// Namespace should exist
    Present,
    /// Namespace should not exist
    Absent,
}

impl NamespaceState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(NamespaceState::Present),
            "absent" => Ok(NamespaceState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Namespace module configuration
#[derive(Debug, Clone)]
struct NamespaceConfig {
    name: String,
    state: NamespaceState,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    wait: bool,
    wait_timeout: u64,
}

impl NamespaceConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = NamespaceState::from_str(&state_str)?;
        let wait = params.get_bool_or("wait", false);
        let wait_timeout = params.get_i64("wait_timeout")?.unwrap_or(60) as u64;

        let labels = Self::parse_string_map(params, "labels")?;
        let annotations = Self::parse_string_map(params, "annotations")?;

        Ok(Self {
            name,
            state,
            labels,
            annotations,
            wait,
            wait_timeout,
        })
    }

    fn parse_string_map(params: &ModuleParams, key: &str) -> ModuleResult<HashMap<String, String>> {
        match params.get(key) {
            Some(serde_json::Value::Object(map)) => {
                let mut result = HashMap::new();
                for (k, v) in map {
                    let value = match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string().trim_matches('"').to_string(),
                    };
                    result.insert(k.clone(), value);
                }
                Ok(result)
            }
            Some(_) => Err(ModuleError::InvalidParameter(format!(
                "{} must be an object/map",
                key
            ))),
            None => Ok(HashMap::new()),
        }
    }
}

/// Module for Kubernetes Namespace management
pub struct K8sNamespaceModule;

#[cfg(feature = "kubernetes")]
impl K8sNamespaceModule {
    /// Build a Namespace resource from configuration
    fn build_namespace(config: &NamespaceConfig) -> Namespace {
        Namespace {
            metadata: ObjectMeta {
                name: Some(config.name.clone()),
                labels: if config.labels.is_empty() {
                    None
                } else {
                    Some(
                        config
                            .labels
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    )
                },
                annotations: if config.annotations.is_empty() {
                    None
                } else {
                    Some(
                        config
                            .annotations
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    )
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Execute the namespace module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = NamespaceConfig::from_params(params)?;

        // Validate namespace name
        Self::validate_namespace_name(&config.name)?;

        // Create Kubernetes client
        let client = Client::try_default().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create Kubernetes client: {}", e))
        })?;

        let namespaces: Api<Namespace> = Api::all(client.clone());

        match config.state {
            NamespaceState::Absent => {
                // Check if namespace exists
                match namespaces.get(&config.name).await {
                    Ok(existing) => {
                        // Check if namespace is terminating
                        if let Some(status) = &existing.status {
                            if status.phase.as_deref() == Some("Terminating") {
                                return Ok(ModuleOutput::ok(format!(
                                    "Namespace '{}' is already being terminated",
                                    config.name
                                )));
                            }
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would delete Namespace '{}'",
                                config.name
                            )));
                        }

                        // Delete the namespace
                        namespaces
                            .delete(&config.name, &DeleteParams::default())
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to delete Namespace: {}",
                                    e
                                ))
                            })?;

                        // Wait for namespace to be fully deleted if requested
                        if config.wait {
                            Self::wait_for_deletion(&namespaces, &config.name, config.wait_timeout)
                                .await?;
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Deleted Namespace '{}'",
                            config.name
                        )))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => Ok(ModuleOutput::ok(format!(
                        "Namespace '{}' already absent",
                        config.name
                    ))),
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Namespace: {}",
                        e
                    ))),
                }
            }
            NamespaceState::Present => {
                let namespace = Self::build_namespace(&config);

                // Check if namespace exists
                match namespaces.get(&config.name).await {
                    Ok(existing) => {
                        // Check if namespace is terminating
                        if let Some(status) = &existing.status {
                            if status.phase.as_deref() == Some("Terminating") {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Namespace '{}' is being terminated and cannot be updated",
                                    config.name
                                )));
                            }
                        }

                        // Compare and update if needed
                        let needs_update = Self::needs_update(&existing, &namespace);

                        if !needs_update {
                            return Ok(ModuleOutput::ok(format!(
                                "Namespace '{}' is up to date",
                                config.name
                            ))
                            .with_data(
                                "status",
                                serde_json::json!(existing
                                    .status
                                    .as_ref()
                                    .and_then(|s| s.phase.clone())),
                            ));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update Namespace '{}'",
                                config.name
                            )));
                        }

                        // Apply patch to update
                        let patch = Patch::Merge(&namespace);
                        let result = namespaces
                            .patch(&config.name, &PatchParams::apply("rustible"), &patch)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to update Namespace: {}",
                                    e
                                ))
                            })?;

                        Ok(
                            ModuleOutput::changed(format!("Updated Namespace '{}'", config.name))
                                .with_data(
                                    "status",
                                    serde_json::json!(result
                                        .status
                                        .as_ref()
                                        .and_then(|s| s.phase.clone())),
                                ),
                        )
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create Namespace '{}'",
                                config.name
                            )));
                        }

                        // Create new namespace
                        let result = namespaces
                            .create(&PostParams::default(), &namespace)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to create Namespace: {}",
                                    e
                                ))
                            })?;

                        // Wait for namespace to be active if requested
                        if config.wait {
                            Self::wait_for_active(&namespaces, &config.name, config.wait_timeout)
                                .await?;
                        }

                        Ok(
                            ModuleOutput::changed(format!("Created Namespace '{}'", config.name))
                                .with_data(
                                    "status",
                                    serde_json::json!(result
                                        .status
                                        .as_ref()
                                        .and_then(|s| s.phase.clone())),
                                ),
                        )
                    }
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Namespace: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Validate namespace name according to Kubernetes naming rules
    fn validate_namespace_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Namespace name cannot be empty".to_string(),
            ));
        }

        if name.len() > 63 {
            return Err(ModuleError::InvalidParameter(
                "Namespace name cannot exceed 63 characters".to_string(),
            ));
        }

        // Must start with alphanumeric character
        if !name
            .chars()
            .next()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false)
        {
            return Err(ModuleError::InvalidParameter(
                "Namespace name must start with an alphanumeric character".to_string(),
            ));
        }

        // Must end with alphanumeric character
        if !name
            .chars()
            .last()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false)
        {
            return Err(ModuleError::InvalidParameter(
                "Namespace name must end with an alphanumeric character".to_string(),
            ));
        }

        // Only lowercase alphanumeric and hyphens
        for c in name.chars() {
            if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
                return Err(ModuleError::InvalidParameter(format!(
                    "Namespace name can only contain lowercase letters, digits, and hyphens. Invalid character: '{}'",
                    c
                )));
            }
        }

        Ok(())
    }

    /// Check if namespace needs to be updated
    fn needs_update(existing: &Namespace, desired: &Namespace) -> bool {
        // Compare labels
        if existing.metadata.labels != desired.metadata.labels {
            return true;
        }

        // Compare annotations
        if existing.metadata.annotations != desired.metadata.annotations {
            return true;
        }

        false
    }

    /// Wait for namespace to be active
    async fn wait_for_active(
        namespaces: &Api<Namespace>,
        name: &str,
        timeout_secs: u64,
    ) -> ModuleResult<()> {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Timeout waiting for Namespace '{}' to be active",
                    name
                )));
            }

            match namespaces.get(name).await {
                Ok(namespace) => {
                    if let Some(status) = namespace.status {
                        if status.phase.as_deref() == Some("Active") {
                            return Ok(());
                        }
                    }
                }
                Err(kube::Error::Api(e)) if e.code == 404 => {
                    // Still waiting for creation
                }
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Error checking Namespace status: {}",
                        e
                    )));
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Wait for namespace to be deleted
    async fn wait_for_deletion(
        namespaces: &Api<Namespace>,
        name: &str,
        timeout_secs: u64,
    ) -> ModuleResult<()> {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Timeout waiting for Namespace '{}' to be deleted",
                    name
                )));
            }

            match namespaces.get(name).await {
                Ok(_) => {
                    // Still exists, keep waiting
                }
                Err(kube::Error::Api(e)) if e.code == 404 => {
                    // Successfully deleted
                    return Ok(());
                }
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Error checking Namespace deletion: {}",
                        e
                    )));
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

impl Module for K8sNamespaceModule {
    fn name(&self) -> &'static str {
        "k8s_namespace"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes Namespaces"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    #[cfg(feature = "kubernetes")]
    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }

    #[cfg(not(feature = "kubernetes"))]
    fn execute(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        Err(ModuleError::Unsupported(
            "Kubernetes support requires the 'kubernetes' feature".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_state_from_str() {
        assert_eq!(
            NamespaceState::from_str("present").unwrap(),
            NamespaceState::Present
        );
        assert_eq!(
            NamespaceState::from_str("absent").unwrap(),
            NamespaceState::Absent
        );
        assert!(NamespaceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_namespace_config_from_params() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("production"));
        params.insert(
            "labels".to_string(),
            serde_json::json!({
                "environment": "production"
            }),
        );

        let config = NamespaceConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "production");
        assert_eq!(config.labels.len(), 1);
        assert_eq!(config.state, NamespaceState::Present);
    }

    #[test]
    fn test_namespace_module_metadata() {
        let module = K8sNamespaceModule;
        assert_eq!(module.name(), "k8s_namespace");
        assert_eq!(module.required_params(), &["name"]);
    }

    #[cfg(feature = "kubernetes")]
    #[test]
    fn test_validate_namespace_name() {
        // Valid names
        assert!(K8sNamespaceModule::validate_namespace_name("default").is_ok());
        assert!(K8sNamespaceModule::validate_namespace_name("kube-system").is_ok());
        assert!(K8sNamespaceModule::validate_namespace_name("my-ns-123").is_ok());
        assert!(K8sNamespaceModule::validate_namespace_name("a").is_ok());

        // Invalid names
        assert!(K8sNamespaceModule::validate_namespace_name("").is_err());
        assert!(K8sNamespaceModule::validate_namespace_name("-invalid").is_err());
        assert!(K8sNamespaceModule::validate_namespace_name("invalid-").is_err());
        assert!(K8sNamespaceModule::validate_namespace_name("UPPERCASE").is_err());
        assert!(K8sNamespaceModule::validate_namespace_name("has_underscore").is_err());
        assert!(K8sNamespaceModule::validate_namespace_name("has.dot").is_err());
    }
}
