//! Kubernetes ConfigMap module - ConfigMap resource management
//!
//! This module manages Kubernetes ConfigMap resources using the kube-rs crate.
//! ConfigMaps allow you to decouple configuration artifacts from image content
//! to keep containerized applications portable.
//!
//! ## Parameters
//!
//! - `name`: ConfigMap name (required)
//! - `namespace`: Kubernetes namespace (default: "default")
//! - `state`: Desired state (present, absent) (default: "present")
//! - `data`: Key-value pairs for the ConfigMap data
//! - `binary_data`: Binary data as base64-encoded strings
//! - `from_file`: Create ConfigMap from file(s)
//! - `from_literal`: Create from literal key=value pairs
//! - `labels`: Labels to apply to the ConfigMap
//! - `annotations`: Annotations to apply to the ConfigMap
//! - `immutable`: If true, ensures the ConfigMap cannot be updated after creation
//!
//! ## Example
//!
//! ```yaml
//! - name: Create application config
//!   k8s_configmap:
//!     name: app-config
//!     namespace: default
//!     data:
//!       app.properties: |
//!         database.url=jdbc:postgresql://db:5432/app
//!         cache.enabled=true
//!       log.level: INFO
//!     labels:
//!       app: myapp
//!       environment: production
//!     state: present
//! ```

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
#[cfg(not(feature = "kubernetes"))]
use crate::utils::shell_escape;
use std::collections::HashMap;

#[cfg(feature = "kubernetes")]
use k8s_openapi::api::core::v1::ConfigMap;
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
#[cfg(feature = "kubernetes")]
use k8s_openapi::ByteString;
#[cfg(feature = "kubernetes")]
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client,
};

/// Desired state for a Kubernetes ConfigMap
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigMapState {
    /// ConfigMap should exist
    Present,
    /// ConfigMap should not exist
    Absent,
}

impl ConfigMapState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(ConfigMapState::Present),
            "absent" => Ok(ConfigMapState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// ConfigMap module configuration
#[derive(Debug, Clone)]
struct ConfigMapConfig {
    name: String,
    namespace: String,
    state: ConfigMapState,
    data: HashMap<String, String>,
    binary_data: HashMap<String, Vec<u8>>,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    immutable: Option<bool>,
}

impl ConfigMapConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let namespace = params
            .get_string("namespace")?
            .unwrap_or_else(|| "default".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = ConfigMapState::from_str(&state_str)?;
        let immutable = params.get_bool("immutable")?;

        let data = Self::parse_string_map(params, "data")?;
        let binary_data = Self::parse_binary_data(params)?;
        let labels = Self::parse_string_map(params, "labels")?;
        let annotations = Self::parse_string_map(params, "annotations")?;

        Ok(Self {
            name,
            namespace,
            state,
            data,
            binary_data,
            labels,
            annotations,
            immutable,
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

    fn parse_binary_data(params: &ModuleParams) -> ModuleResult<HashMap<String, Vec<u8>>> {
        match params.get("binary_data") {
            Some(serde_json::Value::Object(map)) => {
                let mut result = HashMap::new();
                for (k, v) in map {
                    let encoded = match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string().trim_matches('"').to_string(),
                    };
                    let decoded = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &encoded,
                    )
                    .map_err(|e| {
                        ModuleError::InvalidParameter(format!(
                            "Invalid base64 in binary_data[{}]: {}",
                            k, e
                        ))
                    })?;
                    result.insert(k.clone(), decoded);
                }
                Ok(result)
            }
            Some(_) => Err(ModuleError::InvalidParameter(
                "binary_data must be an object/map".to_string(),
            )),
            None => Ok(HashMap::new()),
        }
    }
}

/// Module for Kubernetes ConfigMap management
pub struct K8sConfigMapModule;

#[cfg(feature = "kubernetes")]
impl K8sConfigMapModule {
    /// Build a ConfigMap resource from configuration
    fn build_configmap(config: &ConfigMapConfig) -> ConfigMap {
        let mut labels = config.labels.clone();
        if !labels.contains_key("app") {
            labels.insert("app".to_string(), config.name.clone());
        }

        ConfigMap {
            metadata: ObjectMeta {
                name: Some(config.name.clone()),
                namespace: Some(config.namespace.clone()),
                labels: Some(labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
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
            data: if config.data.is_empty() {
                None
            } else {
                Some(
                    config
                        .data
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            },
            binary_data: if config.binary_data.is_empty() {
                None
            } else {
                Some(
                    config
                        .binary_data
                        .iter()
                        .map(|(k, v)| (k.clone(), ByteString(v.clone())))
                        .collect(),
                )
            },
            immutable: config.immutable,
        }
    }

    /// Execute the configmap module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ConfigMapConfig::from_params(params)?;

        // Create Kubernetes client
        let client = Client::try_default().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create Kubernetes client: {}", e))
        })?;

        let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), &config.namespace);

        match config.state {
            ConfigMapState::Absent => {
                // Check if configmap exists
                match configmaps.get(&config.name).await {
                    Ok(_) => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would delete ConfigMap '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Delete the configmap
                        configmaps
                            .delete(&config.name, &DeleteParams::default())
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to delete ConfigMap: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Deleted ConfigMap '{}/{}'",
                            config.namespace, config.name
                        )))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => Ok(ModuleOutput::ok(format!(
                        "ConfigMap '{}/{}' already absent",
                        config.namespace, config.name
                    ))),
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get ConfigMap: {}",
                        e
                    ))),
                }
            }
            ConfigMapState::Present => {
                let configmap = Self::build_configmap(&config);

                // Check if configmap exists
                match configmaps.get(&config.name).await {
                    Ok(existing) => {
                        // Check if it's immutable
                        if existing.immutable == Some(true) {
                            // Cannot update immutable ConfigMap
                            let needs_update = Self::needs_update(&existing, &configmap);
                            if needs_update {
                                return Err(ModuleError::ExecutionFailed(
                                    "Cannot update immutable ConfigMap".to_string(),
                                ));
                            }
                            return Ok(ModuleOutput::ok(format!(
                                "ConfigMap '{}/{}' is immutable and up to date",
                                config.namespace, config.name
                            )));
                        }

                        // Compare and update if needed
                        let needs_update = Self::needs_update(&existing, &configmap);

                        if !needs_update {
                            return Ok(ModuleOutput::ok(format!(
                                "ConfigMap '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data(
                                "keys",
                                serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                            ));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update ConfigMap '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Apply patch to update
                        let patch = Patch::Merge(&configmap);
                        configmaps
                            .patch(&config.name, &PatchParams::apply("rustible"), &patch)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to update ConfigMap: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Updated ConfigMap '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data(
                            "keys",
                            serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                        ))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create ConfigMap '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Create new configmap
                        configmaps
                            .create(&PostParams::default(), &configmap)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to create ConfigMap: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Created ConfigMap '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data(
                            "keys",
                            serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                        ))
                    }
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get ConfigMap: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Check if configmap needs to be updated
    fn needs_update(existing: &ConfigMap, desired: &ConfigMap) -> bool {
        // Compare data
        if existing.data != desired.data {
            return true;
        }

        // Compare binary_data
        if existing.binary_data != desired.binary_data {
            return true;
        }

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
}

#[cfg(not(feature = "kubernetes"))]
impl K8sConfigMapModule {
    fn run_cmd(cmd: &str, context: &ModuleContext) -> ModuleResult<(bool, String, String)> {
        if let Some(conn) = context.connection.as_ref() {
            let rt = tokio::runtime::Handle::try_current()
                .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".into()))?;
            let result = tokio::task::block_in_place(|| rt.block_on(conn.execute(cmd, None)))
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to execute command: {}", e))
                })?;
            Ok((result.success, result.stdout, result.stderr))
        } else {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to run command: {}", e))
                })?;
            Ok((
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    fn execute_cli(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ConfigMapConfig::from_params(params)?;
        let name_escaped = shell_escape(&config.name);
        let ns_escaped = shell_escape(&config.namespace);

        // Check if configmap already exists
        let check_cmd = format!(
            "kubectl get configmap {} -n {} -o json 2>/dev/null",
            name_escaped, ns_escaped
        );
        let (exists, existing_json, _) = Self::run_cmd(&check_cmd, context)?;

        match config.state {
            ConfigMapState::Absent => {
                if !exists {
                    return Ok(ModuleOutput::ok(format!(
                        "ConfigMap '{}/{}' already absent",
                        config.namespace, config.name
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete ConfigMap '{}/{}'",
                        config.namespace, config.name
                    )));
                }

                let delete_cmd = format!(
                    "kubectl delete configmap {} -n {}",
                    name_escaped, ns_escaped
                );
                let (success, _, stderr) = Self::run_cmd(&delete_cmd, context)?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to delete ConfigMap: {}",
                        stderr
                    )));
                }

                Ok(ModuleOutput::changed(format!(
                    "Deleted ConfigMap '{}/{}'",
                    config.namespace, config.name
                )))
            }
            ConfigMapState::Present => {
                // Build the manifest
                let mut labels = config.labels.clone();
                if !labels.contains_key("app") {
                    labels.insert("app".to_string(), config.name.clone());
                }

                let labels_json: serde_json::Value = labels
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                    .collect::<serde_json::Map<String, serde_json::Value>>()
                    .into();

                let annotations_json: serde_json::Value = if config.annotations.is_empty() {
                    serde_json::Value::Null
                } else {
                    config
                        .annotations
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into()
                };

                let data_json: serde_json::Value = if config.data.is_empty() {
                    serde_json::Value::Null
                } else {
                    config
                        .data
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into()
                };

                let binary_data_json: serde_json::Value = if config.binary_data.is_empty() {
                    serde_json::Value::Null
                } else {
                    config
                        .binary_data
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                serde_json::json!(base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    v
                                )),
                            )
                        })
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into()
                };

                let mut manifest = serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "ConfigMap",
                    "metadata": {
                        "name": config.name,
                        "namespace": config.namespace,
                        "labels": labels_json,
                    },
                });

                if !config.annotations.is_empty() {
                    manifest["metadata"]["annotations"] = annotations_json;
                }
                if !config.data.is_empty() {
                    manifest["data"] = data_json;
                }
                if !config.binary_data.is_empty() {
                    manifest["binaryData"] = binary_data_json;
                }
                if let Some(immutable) = config.immutable {
                    manifest["immutable"] = serde_json::json!(immutable);
                }

                if exists {
                    // Check if existing configmap is immutable
                    if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_json)
                    {
                        if existing.get("immutable") == Some(&serde_json::json!(true)) {
                            // Check if update is needed by comparing data
                            let existing_data = existing.get("data");
                            let desired_data = manifest.get("data");
                            let existing_labels = existing.pointer("/metadata/labels");
                            let desired_labels = manifest.pointer("/metadata/labels");

                            if existing_data != desired_data || existing_labels != desired_labels {
                                return Err(ModuleError::ExecutionFailed(
                                    "Cannot update immutable ConfigMap".to_string(),
                                ));
                            }
                            return Ok(ModuleOutput::ok(format!(
                                "ConfigMap '{}/{}' is immutable and up to date",
                                config.namespace, config.name
                            )));
                        }

                        // Check if update is needed
                        let existing_data = existing.get("data");
                        let desired_data = manifest.get("data");
                        let existing_labels = existing.pointer("/metadata/labels");
                        let desired_labels = manifest.pointer("/metadata/labels");
                        let existing_annotations = existing.pointer("/metadata/annotations");
                        let desired_annotations = manifest.pointer("/metadata/annotations");

                        if existing_data == desired_data
                            && existing_labels == desired_labels
                            && existing_annotations == desired_annotations
                        {
                            return Ok(ModuleOutput::ok(format!(
                                "ConfigMap '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data(
                                "keys",
                                serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                            ));
                        }
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update ConfigMap '{}/{}'",
                            config.namespace, config.name
                        )));
                    }

                    let apply_cmd = format!(
                        "echo {} | kubectl apply -f -",
                        shell_escape(&manifest.to_string())
                    );
                    let (success, _, stderr) = Self::run_cmd(&apply_cmd, context)?;
                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to update ConfigMap: {}",
                            stderr
                        )));
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Updated ConfigMap '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data(
                        "keys",
                        serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                    ))
                } else {
                    // Create new configmap
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create ConfigMap '{}/{}'",
                            config.namespace, config.name
                        )));
                    }

                    let apply_cmd = format!(
                        "echo {} | kubectl apply -f -",
                        shell_escape(&manifest.to_string())
                    );
                    let (success, _, stderr) = Self::run_cmd(&apply_cmd, context)?;
                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to create ConfigMap: {}",
                            stderr
                        )));
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Created ConfigMap '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data(
                        "keys",
                        serde_json::json!(config.data.keys().collect::<Vec<_>>()),
                    ))
                }
            }
        }
    }
}

impl Module for K8sConfigMapModule {
    fn name(&self) -> &'static str {
        "k8s_configmap"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes ConfigMaps"
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
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        self.execute_cli(params, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configmap_state_from_str() {
        assert_eq!(
            ConfigMapState::from_str("present").unwrap(),
            ConfigMapState::Present
        );
        assert_eq!(
            ConfigMapState::from_str("absent").unwrap(),
            ConfigMapState::Absent
        );
        assert!(ConfigMapState::from_str("invalid").is_err());
    }

    #[test]
    fn test_configmap_config_from_params() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("app-config"));
        params.insert("namespace".to_string(), serde_json::json!("default"));
        params.insert(
            "data".to_string(),
            serde_json::json!({
                "key1": "value1",
                "key2": "value2"
            }),
        );

        let config = ConfigMapConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "app-config");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.data.len(), 2);
        assert_eq!(config.state, ConfigMapState::Present);
    }

    #[test]
    fn test_configmap_module_metadata() {
        let module = K8sConfigMapModule;
        assert_eq!(module.name(), "k8s_configmap");
        assert_eq!(module.required_params(), &["name"]);
    }
}
