//! Kubernetes Deployment module - Deployment resource management
//!
//! This module manages Kubernetes Deployment resources using the kube-rs crate.
//! It supports creating, updating, and deleting Deployments with full control over
//! pod specifications, replicas, and update strategies.
//!
//! ## Parameters
//!
//! - `name`: Deployment name (required)
//! - `namespace`: Kubernetes namespace (default: "default")
//! - `state`: Desired state (present, absent) (default: "present")
//! - `replicas`: Number of desired pod replicas (default: 1)
//! - `image`: Container image to deploy (required for state=present)
//! - `container_name`: Name of the container (default: same as deployment name)
//! - `container_port`: Container port to expose
//! - `labels`: Labels to apply to the Deployment and pods
//! - `annotations`: Annotations to apply to the Deployment
//! - `env`: Environment variables as key-value pairs
//! - `resources`: Resource requests and limits
//! - `strategy`: Update strategy (RollingUpdate, Recreate)
//! - `max_surge`: Maximum number of pods above desired count during update
//! - `max_unavailable`: Maximum number of unavailable pods during update
//! - `wait`: Wait for deployment to be ready (default: false)
//! - `wait_timeout`: Timeout in seconds for wait (default: 300)
//!
//! ## Example
//!
//! ```yaml
//! - name: Deploy nginx
//!   k8s_deployment:
//!     name: nginx-deployment
//!     namespace: default
//!     replicas: 3
//!     image: nginx:1.21
//!     container_port: 80
//!     labels:
//!       app: nginx
//!       tier: frontend
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
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy};
#[cfg(feature = "kubernetes")]
use k8s_openapi::api::core::v1::{Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec};
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
#[cfg(feature = "kubernetes")]
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client,
};

/// Desired state for a Kubernetes Deployment
#[derive(Debug, Clone, PartialEq)]
pub enum DeploymentState {
    /// Deployment should exist
    Present,
    /// Deployment should not exist
    Absent,
}

impl DeploymentState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(DeploymentState::Present),
            "absent" => Ok(DeploymentState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Update strategy for Deployments
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStrategy {
    RollingUpdate,
    Recreate,
}

impl UpdateStrategy {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "rollingupdate" | "rolling_update" | "rolling" => Ok(UpdateStrategy::RollingUpdate),
            "recreate" => Ok(UpdateStrategy::Recreate),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid strategy '{}'. Valid strategies: RollingUpdate, Recreate",
                s
            ))),
        }
    }
}

/// Deployment module configuration
#[derive(Debug, Clone)]
struct DeploymentConfig {
    name: String,
    namespace: String,
    state: DeploymentState,
    replicas: i32,
    image: Option<String>,
    container_name: Option<String>,
    container_port: Option<i32>,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    env: HashMap<String, String>,
    strategy: UpdateStrategy,
    max_surge: Option<String>,
    max_unavailable: Option<String>,
    wait: bool,
    wait_timeout: u64,
}

impl DeploymentConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let namespace = params
            .get_string("namespace")?
            .unwrap_or_else(|| "default".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = DeploymentState::from_str(&state_str)?;
        let replicas = params.get_i64("replicas")?.unwrap_or(1) as i32;
        let image = params.get_string("image")?;
        let container_name = params.get_string("container_name")?;
        let container_port = params.get_i64("container_port")?.map(|p| p as i32);
        let strategy_str = params
            .get_string("strategy")?
            .unwrap_or_else(|| "RollingUpdate".to_string());
        let strategy = UpdateStrategy::from_str(&strategy_str)?;
        let max_surge = params.get_string("max_surge")?;
        let max_unavailable = params.get_string("max_unavailable")?;
        let wait = params.get_bool_or("wait", false);
        let wait_timeout = params.get_i64("wait_timeout")?.unwrap_or(300) as u64;

        // Parse labels
        let labels = Self::parse_string_map(params, "labels")?;
        let annotations = Self::parse_string_map(params, "annotations")?;
        let env = Self::parse_string_map(params, "env")?;

        Ok(Self {
            name,
            namespace,
            state,
            replicas,
            image,
            container_name,
            container_port,
            labels,
            annotations,
            env,
            strategy,
            max_surge,
            max_unavailable,
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

/// Module for Kubernetes Deployment management
pub struct K8sDeploymentModule;

#[cfg(feature = "kubernetes")]
impl K8sDeploymentModule {
    /// Build a Deployment resource from configuration
    fn build_deployment(config: &DeploymentConfig) -> ModuleResult<Deployment> {
        let image = config.image.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("image is required for state=present".to_string())
        })?;

        let container_name = config
            .container_name
            .clone()
            .unwrap_or_else(|| config.name.clone());

        // Build container ports
        let ports = config.container_port.map(|port| {
            vec![ContainerPort {
                container_port: port,
                ..Default::default()
            }]
        });

        // Build environment variables
        let env_vars: Vec<EnvVar> = config
            .env
            .iter()
            .map(|(name, value)| EnvVar {
                name: name.clone(),
                value: Some(value.clone()),
                ..Default::default()
            })
            .collect();

        // Build container
        let container = Container {
            name: container_name,
            image: Some(image.clone()),
            ports,
            env: if env_vars.is_empty() {
                None
            } else {
                Some(env_vars)
            },
            ..Default::default()
        };

        // Build labels with app label for selector
        let mut labels = config.labels.clone();
        if !labels.contains_key("app") {
            labels.insert("app".to_string(), config.name.clone());
        }

        // Build deployment strategy
        let strategy = match config.strategy {
            UpdateStrategy::Recreate => DeploymentStrategy {
                type_: Some("Recreate".to_string()),
                rolling_update: None,
            },
            UpdateStrategy::RollingUpdate => {
                use k8s_openapi::api::apps::v1::RollingUpdateDeployment;
                DeploymentStrategy {
                    type_: Some("RollingUpdate".to_string()),
                    rolling_update: Some(RollingUpdateDeployment {
                        max_surge: config
                            .max_surge
                            .as_ref()
                            .map(|s| IntOrString::String(s.clone())),
                        max_unavailable: config
                            .max_unavailable
                            .as_ref()
                            .map(|s| IntOrString::String(s.clone())),
                    }),
                }
            }
        };

        // Build the Deployment
        let deployment = Deployment {
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
            spec: Some(DeploymentSpec {
                replicas: Some(config.replicas),
                selector: LabelSelector {
                    match_labels: Some(
                        labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    ),
                    ..Default::default()
                },
                strategy: Some(strategy),
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        containers: vec![container],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        Ok(deployment)
    }

    /// Execute the deployment module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = DeploymentConfig::from_params(params)?;

        // Create Kubernetes client
        let client = Client::try_default().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create Kubernetes client: {}", e))
        })?;

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), &config.namespace);

        match config.state {
            DeploymentState::Absent => {
                // Check if deployment exists
                match deployments.get(&config.name).await {
                    Ok(_) => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would delete Deployment '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Delete the deployment
                        deployments
                            .delete(&config.name, &DeleteParams::default())
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to delete Deployment: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Deleted Deployment '{}/{}'",
                            config.namespace, config.name
                        )))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => Ok(ModuleOutput::ok(format!(
                        "Deployment '{}/{}' already absent",
                        config.namespace, config.name
                    ))),
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Deployment: {}",
                        e
                    ))),
                }
            }
            DeploymentState::Present => {
                let deployment = Self::build_deployment(&config)?;

                // Check if deployment exists
                match deployments.get(&config.name).await {
                    Ok(existing) => {
                        // Compare and update if needed
                        let needs_update = Self::needs_update(&existing, &deployment);

                        if !needs_update {
                            return Ok(ModuleOutput::ok(format!(
                                "Deployment '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data("replicas", serde_json::json!(config.replicas)));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update Deployment '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Apply patch to update
                        let patch = Patch::Merge(&deployment);
                        deployments
                            .patch(&config.name, &PatchParams::apply("rustible"), &patch)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to update Deployment: {}",
                                    e
                                ))
                            })?;

                        // Wait for deployment if requested
                        if config.wait {
                            Self::wait_for_deployment(
                                &deployments,
                                &config.name,
                                config.wait_timeout,
                            )
                            .await?;
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Updated Deployment '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data("replicas", serde_json::json!(config.replicas)))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create Deployment '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Create new deployment
                        deployments
                            .create(&PostParams::default(), &deployment)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to create Deployment: {}",
                                    e
                                ))
                            })?;

                        // Wait for deployment if requested
                        if config.wait {
                            Self::wait_for_deployment(
                                &deployments,
                                &config.name,
                                config.wait_timeout,
                            )
                            .await?;
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Created Deployment '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data("replicas", serde_json::json!(config.replicas)))
                    }
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Deployment: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Check if deployment needs to be updated
    fn needs_update(existing: &Deployment, desired: &Deployment) -> bool {
        // Compare replicas
        let existing_replicas = existing.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
        let desired_replicas = desired.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
        if existing_replicas != desired_replicas {
            return true;
        }

        // Compare container images
        let existing_image = existing
            .spec
            .as_ref()
            .and_then(|s| s.template.spec.as_ref())
            .and_then(|ps| ps.containers.first())
            .and_then(|c| c.image.as_ref());

        let desired_image = desired
            .spec
            .as_ref()
            .and_then(|s| s.template.spec.as_ref())
            .and_then(|ps| ps.containers.first())
            .and_then(|c| c.image.as_ref());

        if existing_image != desired_image {
            return true;
        }

        // Compare labels
        let existing_labels = existing.metadata.labels.as_ref();
        let desired_labels = desired.metadata.labels.as_ref();
        if existing_labels != desired_labels {
            return true;
        }

        false
    }

    /// Wait for deployment to be ready
    async fn wait_for_deployment(
        deployments: &Api<Deployment>,
        name: &str,
        timeout_secs: u64,
    ) -> ModuleResult<()> {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Timeout waiting for Deployment '{}' to be ready",
                    name
                )));
            }

            match deployments.get(name).await {
                Ok(deployment) => {
                    if let Some(status) = deployment.status {
                        let ready = status.ready_replicas.unwrap_or(0);
                        let desired = deployment
                            .spec
                            .as_ref()
                            .and_then(|s| s.replicas)
                            .unwrap_or(1);

                        if ready >= desired {
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Error checking Deployment status: {}",
                        e
                    )));
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

#[cfg(not(feature = "kubernetes"))]
impl K8sDeploymentModule {
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
        let config = DeploymentConfig::from_params(params)?;
        let name_escaped = shell_escape(&config.name);
        let ns_escaped = shell_escape(&config.namespace);

        // Check if deployment already exists
        let check_cmd = format!(
            "kubectl get deployment {} -n {} -o json 2>/dev/null",
            name_escaped, ns_escaped
        );
        let (exists, existing_json, _) = Self::run_cmd(&check_cmd, context)?;

        match config.state {
            DeploymentState::Absent => {
                if !exists {
                    return Ok(ModuleOutput::ok(format!(
                        "Deployment '{}/{}' already absent",
                        config.namespace, config.name
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete Deployment '{}/{}'",
                        config.namespace, config.name
                    )));
                }

                let delete_cmd = format!(
                    "kubectl delete deployment {} -n {}",
                    name_escaped, ns_escaped
                );
                let (success, _, stderr) = Self::run_cmd(&delete_cmd, context)?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to delete Deployment: {}",
                        stderr
                    )));
                }

                Ok(ModuleOutput::changed(format!(
                    "Deleted Deployment '{}/{}'",
                    config.namespace, config.name
                )))
            }
            DeploymentState::Present => {
                let image = config.image.as_ref().ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "image is required for state=present".to_string(),
                    )
                })?;

                let container_name = config
                    .container_name
                    .clone()
                    .unwrap_or_else(|| config.name.clone());

                // Build labels with app label for selector
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

                // Build container ports
                let ports_json: serde_json::Value = if let Some(port) = config.container_port {
                    serde_json::json!([{"containerPort": port}])
                } else {
                    serde_json::Value::Null
                };

                // Build environment variables
                let env_json: serde_json::Value = if config.env.is_empty() {
                    serde_json::Value::Null
                } else {
                    let env_vars: Vec<serde_json::Value> = config
                        .env
                        .iter()
                        .map(|(k, v)| serde_json::json!({"name": k, "value": v}))
                        .collect();
                    serde_json::json!(env_vars)
                };

                // Build strategy
                let strategy_json = match config.strategy {
                    UpdateStrategy::Recreate => serde_json::json!({"type": "Recreate"}),
                    UpdateStrategy::RollingUpdate => {
                        let mut rolling = serde_json::Map::new();
                        if let Some(ref ms) = config.max_surge {
                            rolling.insert(
                                "maxSurge".to_string(),
                                serde_json::json!(ms),
                            );
                        }
                        if let Some(ref mu) = config.max_unavailable {
                            rolling.insert(
                                "maxUnavailable".to_string(),
                                serde_json::json!(mu),
                            );
                        }
                        serde_json::json!({
                            "type": "RollingUpdate",
                            "rollingUpdate": rolling,
                        })
                    }
                };

                // Build container spec
                let mut container = serde_json::json!({
                    "name": container_name,
                    "image": image,
                });
                if !ports_json.is_null() {
                    container["ports"] = ports_json;
                }
                if !env_json.is_null() {
                    container["env"] = env_json;
                }

                // Build the full manifest
                let mut manifest = serde_json::json!({
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "metadata": {
                        "name": config.name,
                        "namespace": config.namespace,
                        "labels": labels_json,
                    },
                    "spec": {
                        "replicas": config.replicas,
                        "selector": {
                            "matchLabels": labels_json,
                        },
                        "strategy": strategy_json,
                        "template": {
                            "metadata": {
                                "labels": labels_json,
                            },
                            "spec": {
                                "containers": [container],
                            },
                        },
                    },
                });

                if !config.annotations.is_empty() {
                    manifest["metadata"]["annotations"] = annotations_json;
                }

                if exists {
                    // Check if update is needed
                    if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_json)
                    {
                        let existing_replicas = existing
                            .pointer("/spec/replicas")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(1);
                        let existing_image = existing
                            .pointer("/spec/template/spec/containers/0/image")
                            .and_then(|v| v.as_str());
                        let existing_labels = existing.pointer("/metadata/labels");
                        let desired_labels = manifest.pointer("/metadata/labels");

                        if existing_replicas == config.replicas as i64
                            && existing_image == Some(image.as_str())
                            && existing_labels == desired_labels
                        {
                            return Ok(ModuleOutput::ok(format!(
                                "Deployment '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data("replicas", serde_json::json!(config.replicas)));
                        }
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update Deployment '{}/{}'",
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
                            "Failed to update Deployment: {}",
                            stderr
                        )));
                    }

                    // Wait for deployment if requested
                    if config.wait {
                        let wait_cmd = format!(
                            "kubectl rollout status deployment/{} -n {} --timeout={}s",
                            name_escaped, ns_escaped, config.wait_timeout
                        );
                        let (ok, _, stderr) = Self::run_cmd(&wait_cmd, context)?;
                        if !ok {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Timeout waiting for Deployment '{}' to be ready: {}",
                                config.name, stderr
                            )));
                        }
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Updated Deployment '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("replicas", serde_json::json!(config.replicas)))
                } else {
                    // Create new deployment
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create Deployment '{}/{}'",
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
                            "Failed to create Deployment: {}",
                            stderr
                        )));
                    }

                    // Wait for deployment if requested
                    if config.wait {
                        let wait_cmd = format!(
                            "kubectl rollout status deployment/{} -n {} --timeout={}s",
                            name_escaped, ns_escaped, config.wait_timeout
                        );
                        let (ok, _, stderr) = Self::run_cmd(&wait_cmd, context)?;
                        if !ok {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Timeout waiting for Deployment '{}' to be ready: {}",
                                config.name, stderr
                            )));
                        }
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Created Deployment '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("replicas", serde_json::json!(config.replicas)))
                }
            }
        }
    }
}

impl Module for K8sDeploymentModule {
    fn name(&self) -> &'static str {
        "k8s_deployment"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes Deployments"
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
    fn test_deployment_state_from_str() {
        assert_eq!(
            DeploymentState::from_str("present").unwrap(),
            DeploymentState::Present
        );
        assert_eq!(
            DeploymentState::from_str("absent").unwrap(),
            DeploymentState::Absent
        );
        assert!(DeploymentState::from_str("invalid").is_err());
    }

    #[test]
    fn test_update_strategy_from_str() {
        assert_eq!(
            UpdateStrategy::from_str("RollingUpdate").unwrap(),
            UpdateStrategy::RollingUpdate
        );
        assert_eq!(
            UpdateStrategy::from_str("rolling_update").unwrap(),
            UpdateStrategy::RollingUpdate
        );
        assert_eq!(
            UpdateStrategy::from_str("Recreate").unwrap(),
            UpdateStrategy::Recreate
        );
        assert!(UpdateStrategy::from_str("invalid").is_err());
    }

    #[test]
    fn test_deployment_config_from_params() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert("namespace".to_string(), serde_json::json!("default"));
        params.insert("replicas".to_string(), serde_json::json!(3));
        params.insert("image".to_string(), serde_json::json!("nginx:1.21"));

        let config = DeploymentConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "nginx");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.replicas, 3);
        assert_eq!(config.image, Some("nginx:1.21".to_string()));
        assert_eq!(config.state, DeploymentState::Present);
    }

    #[test]
    fn test_deployment_module_metadata() {
        let module = K8sDeploymentModule;
        assert_eq!(module.name(), "k8s_deployment");
        assert_eq!(module.required_params(), &["name"]);
    }
}
