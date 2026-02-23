//! Kubernetes Secret module - Secret resource management
//!
//! This module manages Kubernetes Secret resources using the kube-rs crate.
//! Secrets allow you to store and manage sensitive information, such as
//! passwords, OAuth tokens, and ssh keys.
//!
//! ## Parameters
//!
//! - `name`: Secret name (required)
//! - `namespace`: Kubernetes namespace (default: "default")
//! - `state`: Desired state (present, absent) (default: "present")
//! - `type`: Secret type (Opaque, kubernetes.io/tls, kubernetes.io/dockerconfigjson, etc.)
//! - `data`: Key-value pairs for the Secret data (values will be base64 encoded)
//! - `string_data`: Key-value pairs as plain strings (will be encoded by Kubernetes)
//! - `labels`: Labels to apply to the Secret
//! - `annotations`: Annotations to apply to the Secret
//! - `immutable`: If true, ensures the Secret cannot be updated after creation
//!
//! ## Secret Types
//!
//! - `Opaque`: arbitrary user-defined data (default)
//! - `kubernetes.io/service-account-token`: service account token
//! - `kubernetes.io/dockerconfigjson`: serialized ~/.docker/config.json
//! - `kubernetes.io/dockercfg`: serialized ~/.dockercfg
//! - `kubernetes.io/basic-auth`: credentials for basic authentication
//! - `kubernetes.io/ssh-auth`: credentials for SSH authentication
//! - `kubernetes.io/tls`: TLS certificate and key
//! - `bootstrap.kubernetes.io/token`: bootstrap token data
//!
//! ## Example
//!
//! ```yaml
//! - name: Create database credentials
//!   k8s_secret:
//!     name: db-credentials
//!     namespace: default
//!     type: Opaque
//!     string_data:
//!       username: admin
//!       password: "{{ db_password }}"
//!     labels:
//!       app: myapp
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
use k8s_openapi::api::core::v1::Secret;
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
#[cfg(feature = "kubernetes")]
use k8s_openapi::ByteString;
#[cfg(feature = "kubernetes")]
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client,
};

/// Desired state for a Kubernetes Secret
#[derive(Debug, Clone, PartialEq)]
pub enum SecretState {
    /// Secret should exist
    Present,
    /// Secret should not exist
    Absent,
}

impl SecretState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(SecretState::Present),
            "absent" => Ok(SecretState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Secret module configuration
#[derive(Debug, Clone)]
struct SecretConfig {
    name: String,
    namespace: String,
    state: SecretState,
    secret_type: String,
    data: HashMap<String, Vec<u8>>,
    string_data: HashMap<String, String>,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    immutable: Option<bool>,
}

impl SecretConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let namespace = params
            .get_string("namespace")?
            .unwrap_or_else(|| "default".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = SecretState::from_str(&state_str)?;
        let secret_type = params
            .get_string("type")?
            .unwrap_or_else(|| "Opaque".to_string());
        let immutable = params.get_bool("immutable")?;

        let data = Self::parse_data(params)?;
        let string_data = Self::parse_string_map(params, "string_data")?;
        let labels = Self::parse_string_map(params, "labels")?;
        let annotations = Self::parse_string_map(params, "annotations")?;

        Ok(Self {
            name,
            namespace,
            state,
            secret_type,
            data,
            string_data,
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

    fn parse_data(params: &ModuleParams) -> ModuleResult<HashMap<String, Vec<u8>>> {
        match params.get("data") {
            Some(serde_json::Value::Object(map)) => {
                let mut result = HashMap::new();
                for (k, v) in map {
                    let encoded = match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string().trim_matches('"').to_string(),
                    };
                    // Data values should already be base64 encoded
                    let decoded = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &encoded,
                    )
                    .map_err(|e| {
                        ModuleError::InvalidParameter(format!(
                            "Invalid base64 in data[{}]: {}. Use 'string_data' for plain text values.",
                            k, e
                        ))
                    })?;
                    result.insert(k.clone(), decoded);
                }
                Ok(result)
            }
            Some(_) => Err(ModuleError::InvalidParameter(
                "data must be an object/map".to_string(),
            )),
            None => Ok(HashMap::new()),
        }
    }
}

/// Module for Kubernetes Secret management
pub struct K8sSecretModule;

#[cfg(feature = "kubernetes")]
impl K8sSecretModule {
    /// Build a Secret resource from configuration
    fn build_secret(config: &SecretConfig) -> Secret {
        let mut labels = config.labels.clone();
        if !labels.contains_key("app") {
            labels.insert("app".to_string(), config.name.clone());
        }

        Secret {
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
            type_: Some(config.secret_type.clone()),
            data: if config.data.is_empty() {
                None
            } else {
                Some(
                    config
                        .data
                        .iter()
                        .map(|(k, v)| (k.clone(), ByteString(v.clone())))
                        .collect(),
                )
            },
            string_data: if config.string_data.is_empty() {
                None
            } else {
                Some(
                    config
                        .string_data
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            },
            immutable: config.immutable,
        }
    }

    /// Execute the secret module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = SecretConfig::from_params(params)?;

        // Create Kubernetes client
        let client = Client::try_default().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create Kubernetes client: {}", e))
        })?;

        let secrets: Api<Secret> = Api::namespaced(client.clone(), &config.namespace);

        match config.state {
            SecretState::Absent => {
                // Check if secret exists
                match secrets.get(&config.name).await {
                    Ok(_) => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would delete Secret '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Delete the secret
                        secrets
                            .delete(&config.name, &DeleteParams::default())
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to delete Secret: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Deleted Secret '{}/{}'",
                            config.namespace, config.name
                        )))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => Ok(ModuleOutput::ok(format!(
                        "Secret '{}/{}' already absent",
                        config.namespace, config.name
                    ))),
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Secret: {}",
                        e
                    ))),
                }
            }
            SecretState::Present => {
                let secret = Self::build_secret(&config);

                // Count total keys for output
                let total_keys = config.data.len() + config.string_data.len();

                // Check if secret exists
                match secrets.get(&config.name).await {
                    Ok(existing) => {
                        // Check if it's immutable
                        if existing.immutable == Some(true) {
                            // Cannot update immutable Secret
                            let needs_update = Self::needs_update(&existing, &secret);
                            if needs_update {
                                return Err(ModuleError::ExecutionFailed(
                                    "Cannot update immutable Secret".to_string(),
                                ));
                            }
                            return Ok(ModuleOutput::ok(format!(
                                "Secret '{}/{}' is immutable and up to date",
                                config.namespace, config.name
                            )));
                        }

                        // Compare and update if needed
                        let needs_update = Self::needs_update(&existing, &secret);

                        if !needs_update {
                            return Ok(ModuleOutput::ok(format!(
                                "Secret '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data("type", serde_json::json!(config.secret_type)));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update Secret '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Apply patch to update
                        let patch = Patch::Merge(&secret);
                        secrets
                            .patch(&config.name, &PatchParams::apply("rustible"), &patch)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to update Secret: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Updated Secret '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data("type", serde_json::json!(config.secret_type))
                        .with_data("keys_count", serde_json::json!(total_keys)))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create Secret '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Create new secret
                        secrets
                            .create(&PostParams::default(), &secret)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to create Secret: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Created Secret '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data("type", serde_json::json!(config.secret_type))
                        .with_data("keys_count", serde_json::json!(total_keys)))
                    }
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Secret: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Check if secret needs to be updated
    fn needs_update(existing: &Secret, desired: &Secret) -> bool {
        // Compare type
        if existing.type_ != desired.type_ {
            return true;
        }

        // Compare data (note: string_data is converted to data by Kubernetes)
        // We need to compare the actual binary data
        if existing.data != desired.data {
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
impl K8sSecretModule {
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
        let config = SecretConfig::from_params(params)?;
        let name_escaped = shell_escape(&config.name);
        let ns_escaped = shell_escape(&config.namespace);

        // Check if secret already exists
        let check_cmd = format!(
            "kubectl get secret {} -n {} -o json 2>/dev/null",
            name_escaped, ns_escaped
        );
        let (exists, existing_json, _) = Self::run_cmd(&check_cmd, context)?;

        let total_keys = config.data.len() + config.string_data.len();

        match config.state {
            SecretState::Absent => {
                if !exists {
                    return Ok(ModuleOutput::ok(format!(
                        "Secret '{}/{}' already absent",
                        config.namespace, config.name
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete Secret '{}/{}'",
                        config.namespace, config.name
                    )));
                }

                let delete_cmd = format!(
                    "kubectl delete secret {} -n {}",
                    name_escaped, ns_escaped
                );
                let (success, _, stderr) = Self::run_cmd(&delete_cmd, context)?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to delete Secret: {}",
                        stderr
                    )));
                }

                Ok(ModuleOutput::changed(format!(
                    "Deleted Secret '{}/{}'",
                    config.namespace, config.name
                )))
            }
            SecretState::Present => {
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

                // For secret data, values must be base64-encoded in the manifest
                let data_json: serde_json::Value = if config.data.is_empty() {
                    serde_json::Value::Null
                } else {
                    config
                        .data
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

                let string_data_json: serde_json::Value = if config.string_data.is_empty() {
                    serde_json::Value::Null
                } else {
                    config
                        .string_data
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into()
                };

                let mut manifest = serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Secret",
                    "metadata": {
                        "name": config.name,
                        "namespace": config.namespace,
                        "labels": labels_json,
                    },
                    "type": config.secret_type,
                });

                if !config.annotations.is_empty() {
                    manifest["metadata"]["annotations"] = annotations_json;
                }
                if !config.data.is_empty() {
                    manifest["data"] = data_json;
                }
                if !config.string_data.is_empty() {
                    manifest["stringData"] = string_data_json;
                }
                if let Some(immutable) = config.immutable {
                    manifest["immutable"] = serde_json::json!(immutable);
                }

                if exists {
                    // Check if existing secret is immutable
                    if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_json)
                    {
                        if existing.get("immutable") == Some(&serde_json::json!(true)) {
                            let existing_data = existing.get("data");
                            let existing_type = existing.get("type");
                            let desired_type = manifest.get("type");
                            let existing_labels = existing.pointer("/metadata/labels");
                            let desired_labels = manifest.pointer("/metadata/labels");

                            // For immutable secrets, any difference in data/type/labels means error
                            if existing_data != manifest.get("data")
                                || existing_type != desired_type
                                || existing_labels != desired_labels
                            {
                                return Err(ModuleError::ExecutionFailed(
                                    "Cannot update immutable Secret".to_string(),
                                ));
                            }
                            return Ok(ModuleOutput::ok(format!(
                                "Secret '{}/{}' is immutable and up to date",
                                config.namespace, config.name
                            )));
                        }

                        // Check if update is needed
                        let existing_type = existing.get("type");
                        let desired_type = manifest.get("type");
                        let existing_data = existing.get("data");
                        let existing_labels = existing.pointer("/metadata/labels");
                        let desired_labels = manifest.pointer("/metadata/labels");
                        let existing_annotations = existing.pointer("/metadata/annotations");
                        let desired_annotations = manifest.pointer("/metadata/annotations");

                        if existing_type == desired_type
                            && existing_data == manifest.get("data")
                            && existing_labels == desired_labels
                            && existing_annotations == desired_annotations
                        {
                            return Ok(ModuleOutput::ok(format!(
                                "Secret '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data("type", serde_json::json!(config.secret_type)));
                        }
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update Secret '{}/{}'",
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
                            "Failed to update Secret: {}",
                            stderr
                        )));
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Updated Secret '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("type", serde_json::json!(config.secret_type))
                    .with_data("keys_count", serde_json::json!(total_keys)))
                } else {
                    // Create new secret
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create Secret '{}/{}'",
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
                            "Failed to create Secret: {}",
                            stderr
                        )));
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Created Secret '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("type", serde_json::json!(config.secret_type))
                    .with_data("keys_count", serde_json::json!(total_keys)))
                }
            }
        }
    }
}

impl Module for K8sSecretModule {
    fn name(&self) -> &'static str {
        "k8s_secret"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes Secrets"
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
    fn test_secret_state_from_str() {
        assert_eq!(
            SecretState::from_str("present").unwrap(),
            SecretState::Present
        );
        assert_eq!(
            SecretState::from_str("absent").unwrap(),
            SecretState::Absent
        );
        assert!(SecretState::from_str("invalid").is_err());
    }

    #[test]
    fn test_secret_config_from_params() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("db-credentials"));
        params.insert("namespace".to_string(), serde_json::json!("default"));
        params.insert("type".to_string(), serde_json::json!("Opaque"));
        params.insert(
            "string_data".to_string(),
            serde_json::json!({
                "username": "admin",
                "password": "secret123"
            }),
        );

        let config = SecretConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "db-credentials");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.secret_type, "Opaque");
        assert_eq!(config.string_data.len(), 2);
        assert_eq!(config.state, SecretState::Present);
    }

    #[test]
    fn test_secret_module_metadata() {
        let module = K8sSecretModule;
        assert_eq!(module.name(), "k8s_secret");
        assert_eq!(module.required_params(), &["name"]);
    }
}
