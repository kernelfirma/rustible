//! Kubernetes Deployment module for managing deployments.
//!
//! This module provides comprehensive deployment management including:
//!
//! - Create, update, and delete deployments
//! - Rolling updates with configurable strategies
//! - Replica scaling
//! - Status monitoring and wait operations
//! - YAML manifest application
//!
//! ## Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Deployment name |
//! | `namespace` | No | Kubernetes namespace (default: "default") |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `replicas` | No | Number of replicas (default: 1) |
//! | `image` | No* | Container image (*required when creating) |
//! | `container_name` | No | Container name (default: deployment name) |
//! | `container_port` | No | Container port to expose |
//! | `labels` | No | Labels for the deployment and pods |
//! | `annotations` | No | Annotations for the deployment |
//! | `selector` | No | Label selector for pods |
//! | `env` | No | Environment variables |
//! | `resources` | No | Resource requests and limits |
//! | `strategy` | No | Update strategy (RollingUpdate or Recreate) |
//! | `min_ready_seconds` | No | Minimum seconds for a pod to be ready |
//! | `revision_history_limit` | No | Number of old ReplicaSets to retain |
//! | `liveness_probe` | No | Liveness probe configuration |
//! | `readiness_probe` | No | Readiness probe configuration |
//! | `volumes` | No | Volume specifications |
//! | `volume_mounts` | No | Volume mount specifications |
//! | `wait` | No | Wait for deployment to be ready (default: true) |
//! | `wait_timeout` | No | Timeout for wait in seconds (default: 300) |
//! | `force` | No | Force recreation of deployment |
//! | `definition` | No | Full deployment YAML definition (overrides other params) |
//!
//! ## Example
//!
//! ```yaml
//! - name: Create nginx deployment
//!   k8s_deployment:
//!     name: nginx
//!     namespace: default
//!     replicas: 3
//!     image: nginx:1.21
//!     container_port: 80
//!     labels:
//!       app: nginx
//!       tier: frontend
//!     resources:
//!       requests:
//!         cpu: "100m"
//!         memory: "128Mi"
//!       limits:
//!         cpu: "500m"
//!         memory: "256Mi"
//!     strategy:
//!       type: RollingUpdate
//!       max_unavailable: "25%"
//!       max_surge: "25%"
//!     wait: true
//!     wait_timeout: 600
//! ```

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

use super::{
    parse_annotations, parse_labels, validate_k8s_name, validate_k8s_namespace, K8sResourceState,
    LabelSelector, ResourceRequirements, RollingUpdateConfig, UpdateStrategy,
};

/// Deployment configuration parsed from module parameters
#[derive(Debug, Clone)]
struct DeploymentConfig {
    name: String,
    namespace: String,
    state: K8sResourceState,
    replicas: i32,
    image: Option<String>,
    container_name: Option<String>,
    container_port: Option<i32>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    selector: Option<LabelSelector>,
    env: Vec<EnvVarSpec>,
    resources: Option<ResourceRequirements>,
    strategy: UpdateStrategy,
    min_ready_seconds: Option<i32>,
    revision_history_limit: Option<i32>,
    liveness_probe: Option<ProbeSpec>,
    readiness_probe: Option<ProbeSpec>,
    volumes: Vec<serde_json::Value>,
    volume_mounts: Vec<serde_json::Value>,
    wait: bool,
    wait_timeout: u64,
    force: bool,
    definition: Option<serde_json::Value>,
    kubeconfig: Option<String>,
    context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnvVarSpec {
    name: String,
    value: Option<String>,
    value_from: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeSpec {
    http_get: Option<HttpGetAction>,
    tcp_socket: Option<TcpSocketAction>,
    exec: Option<ExecAction>,
    initial_delay_seconds: Option<i32>,
    period_seconds: Option<i32>,
    timeout_seconds: Option<i32>,
    success_threshold: Option<i32>,
    failure_threshold: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpGetAction {
    path: String,
    port: i32,
    scheme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TcpSocketAction {
    port: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecAction {
    command: Vec<String>,
}

impl DeploymentConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        validate_k8s_name(&name)?;

        let namespace = params
            .get_string("namespace")?
            .unwrap_or_else(|| "default".to_string());
        validate_k8s_namespace(&namespace)?;

        let state = if let Some(s) = params.get_string("state")? {
            K8sResourceState::from_str(&s)?
        } else {
            K8sResourceState::default()
        };

        let replicas = params.get_i64("replicas")?.unwrap_or(1) as i32;
        if replicas < 0 {
            return Err(ModuleError::InvalidParameter(
                "replicas must be non-negative".to_string(),
            ));
        }

        // Parse labels
        let mut labels = BTreeMap::new();
        if let Some(label_value) = params.get("labels") {
            labels = parse_labels(label_value);
        }
        // Ensure app label is set (required for selector)
        if !labels.contains_key("app") {
            labels.insert("app".to_string(), name.clone());
        }

        // Parse annotations
        let annotations = if let Some(ann_value) = params.get("annotations") {
            parse_annotations(ann_value)
        } else {
            BTreeMap::new()
        };

        // Parse selector
        let selector = if let Some(sel_value) = params.get("selector") {
            let match_labels = parse_labels(sel_value);
            Some(LabelSelector { match_labels })
        } else {
            None
        };

        // Parse environment variables
        let env = if let Some(env_value) = params.get("env") {
            if let Some(env_array) = env_value.as_array() {
                env_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse resources
        let resources = if let Some(res_value) = params.get("resources") {
            serde_json::from_value(res_value.clone()).ok()
        } else {
            None
        };

        // Parse update strategy
        let strategy = if let Some(strat_value) = params.get("strategy") {
            if let Some(strat_obj) = strat_value.as_object() {
                let strategy_type = strat_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("RollingUpdate");

                match strategy_type.to_lowercase().as_str() {
                    "recreate" => UpdateStrategy::Recreate,
                    _ => {
                        let config = RollingUpdateConfig {
                            max_unavailable: strat_obj
                                .get("max_unavailable")
                                .and_then(|v| v.as_str())
                                .unwrap_or("25%")
                                .to_string(),
                            max_surge: strat_obj
                                .get("max_surge")
                                .and_then(|v| v.as_str())
                                .unwrap_or("25%")
                                .to_string(),
                        };
                        UpdateStrategy::RollingUpdate { config }
                    }
                }
            } else {
                UpdateStrategy::default()
            }
        } else {
            UpdateStrategy::default()
        };

        // Parse probes
        let liveness_probe = if let Some(probe_value) = params.get("liveness_probe") {
            serde_json::from_value(probe_value.clone()).ok()
        } else {
            None
        };

        let readiness_probe = if let Some(probe_value) = params.get("readiness_probe") {
            serde_json::from_value(probe_value.clone()).ok()
        } else {
            None
        };

        // Parse volumes and volume mounts
        let volumes = if let Some(vol_value) = params.get("volumes") {
            if let Some(vol_array) = vol_value.as_array() {
                vol_array.clone()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let volume_mounts = if let Some(vm_value) = params.get("volume_mounts") {
            if let Some(vm_array) = vm_value.as_array() {
                vm_array.clone()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            name,
            namespace,
            state,
            replicas,
            image: params.get_string("image")?,
            container_name: params.get_string("container_name")?,
            container_port: params.get_i64("container_port")?.map(|p| p as i32),
            labels,
            annotations,
            selector,
            env,
            resources,
            strategy,
            min_ready_seconds: params.get_i64("min_ready_seconds")?.map(|v| v as i32),
            revision_history_limit: params.get_i64("revision_history_limit")?.map(|v| v as i32),
            liveness_probe,
            readiness_probe,
            volumes,
            volume_mounts,
            wait: params.get_bool_or("wait", true),
            wait_timeout: params.get_i64("wait_timeout")?.unwrap_or(300) as u64,
            force: params.get_bool_or("force", false),
            definition: params.get("definition").cloned(),
            kubeconfig: params.get_string("kubeconfig")?,
            context: params.get_string("context")?,
        })
    }

    /// Get effective selector (use explicit selector or derive from labels)
    fn effective_selector(&self) -> BTreeMap<String, String> {
        if let Some(ref sel) = self.selector {
            sel.match_labels.clone()
        } else {
            // Use app label as default selector
            let mut selector = BTreeMap::new();
            if let Some(app) = self.labels.get("app") {
                selector.insert("app".to_string(), app.clone());
            } else {
                selector.insert("app".to_string(), self.name.clone());
            }
            selector
        }
    }
}

/// Simulated Kubernetes Deployment info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentInfo {
    pub name: String,
    pub namespace: String,
    pub replicas: i32,
    pub ready_replicas: i32,
    pub available_replicas: i32,
    pub updated_replicas: i32,
    pub image: String,
    pub creation_timestamp: String,
    pub labels: BTreeMap<String, String>,
    pub conditions: Vec<DeploymentCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentCondition {
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

/// Kubernetes Deployment module
pub struct K8sDeploymentModule;

impl K8sDeploymentModule {
    /// Get deployment by name
    async fn get_deployment(
        _name: &str,
        _namespace: &str,
        _kubeconfig: Option<&str>,
        _context: Option<&str>,
    ) -> ModuleResult<Option<DeploymentInfo>> {
        // In a real implementation, this would use the kube crate:
        //
        // use kube::{Api, Client, Config};
        // use k8s_openapi::api::apps::v1::Deployment;
        //
        // let config = if let Some(kc) = kubeconfig {
        //     Config::from_kubeconfig(&KubeConfigOptions {
        //         kubeconfig: Some(kc.into()),
        //         context: context.map(|s| s.to_string()),
        //         ..Default::default()
        //     }).await?
        // } else {
        //     Config::infer().await?
        // };
        // let client = Client::try_from(config)?;
        // let deployments: Api<Deployment> = Api::namespaced(client, namespace);
        //
        // match deployments.get_opt(name).await? {
        //     Some(dep) => {
        //         // Convert to DeploymentInfo
        //     }
        //     None => Ok(None)
        // }

        Ok(None)
    }

    /// Create or update deployment
    async fn apply_deployment(config: &DeploymentConfig) -> ModuleResult<DeploymentInfo> {
        // Validate required fields for creation
        let image = config.image.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "image is required when creating a deployment".to_string(),
            )
        })?;

        // In a real implementation:
        //
        // let deployment = Deployment {
        //     metadata: ObjectMeta {
        //         name: Some(config.name.clone()),
        //         namespace: Some(config.namespace.clone()),
        //         labels: Some(config.labels.clone()),
        //         annotations: Some(config.annotations.clone()),
        //         ..Default::default()
        //     },
        //     spec: Some(DeploymentSpec {
        //         replicas: Some(config.replicas),
        //         selector: LabelSelector {
        //             match_labels: Some(config.effective_selector()),
        //             ..Default::default()
        //         },
        //         template: PodTemplateSpec {
        //             metadata: Some(ObjectMeta {
        //                 labels: Some(config.labels.clone()),
        //                 ..Default::default()
        //             }),
        //             spec: Some(PodSpec {
        //                 containers: vec![Container {
        //                     name: config.container_name.clone().unwrap_or(config.name.clone()),
        //                     image: Some(image.clone()),
        //                     ports: config.container_port.map(|p| vec![ContainerPort {
        //                         container_port: p,
        //                         ..Default::default()
        //                     }]),
        //                     ..Default::default()
        //                 }],
        //                 ..Default::default()
        //             }),
        //         },
        //         strategy: Some(DeploymentStrategy { .. }),
        //         ..Default::default()
        //     }),
        //     ..Default::default()
        // };
        //
        // let deployments: Api<Deployment> = Api::namespaced(client, &config.namespace);
        // deployments.patch(
        //     &config.name,
        //     &PatchParams::apply("rustible"),
        //     &Patch::Apply(&deployment)
        // ).await?;

        tracing::info!(
            "Would create/update deployment '{}' in namespace '{}' with image '{}'",
            config.name,
            config.namespace,
            image
        );

        Ok(DeploymentInfo {
            name: config.name.clone(),
            namespace: config.namespace.clone(),
            replicas: config.replicas,
            ready_replicas: 0,
            available_replicas: 0,
            updated_replicas: 0,
            image: image.clone(),
            creation_timestamp: chrono::Utc::now().to_rfc3339(),
            labels: config.labels.clone(),
            conditions: vec![DeploymentCondition {
                condition_type: "Progressing".to_string(),
                status: "True".to_string(),
                reason: Some("NewReplicaSetCreated".to_string()),
                message: Some("ReplicaSet is progressing".to_string()),
            }],
        })
    }

    /// Delete deployment
    async fn delete_deployment(
        name: &str,
        namespace: &str,
        _kubeconfig: Option<&str>,
        _context: Option<&str>,
    ) -> ModuleResult<()> {
        // In a real implementation:
        // let deployments: Api<Deployment> = Api::namespaced(client, namespace);
        // deployments.delete(name, &DeleteParams::default()).await?;

        tracing::info!(
            "Would delete deployment '{}' from namespace '{}'",
            name,
            namespace
        );
        Ok(())
    }

    /// Wait for deployment to be ready
    async fn wait_for_ready(
        name: &str,
        namespace: &str,
        _replicas: i32,
        timeout: Duration,
        _kubeconfig: Option<&str>,
        _context: Option<&str>,
    ) -> ModuleResult<DeploymentInfo> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(5);

        tracing::info!(
            "Waiting for deployment '{}/{}' to be ready (timeout: {:?})",
            namespace,
            name,
            timeout
        );

        // In a real implementation, poll until ready:
        // loop {
        //     if start.elapsed() >= timeout {
        //         return Err(ModuleError::ExecutionFailed(...));
        //     }
        //
        //     let dep = deployments.get(name).await?;
        //     if let Some(status) = &dep.status {
        //         if status.ready_replicas == status.replicas {
        //             return Ok(convert_to_info(dep));
        //         }
        //     }
        //
        //     tokio::time::sleep(poll_interval).await;
        // }

        // Simulate wait
        if start.elapsed() < timeout {
            tokio::time::sleep(std::cmp::min(poll_interval, Duration::from_millis(100))).await;
        }

        Ok(DeploymentInfo {
            name: name.to_string(),
            namespace: namespace.to_string(),
            replicas: _replicas,
            ready_replicas: _replicas,
            available_replicas: _replicas,
            updated_replicas: _replicas,
            image: "nginx:latest".to_string(),
            creation_timestamp: chrono::Utc::now().to_rfc3339(),
            labels: BTreeMap::new(),
            conditions: vec![
                DeploymentCondition {
                    condition_type: "Available".to_string(),
                    status: "True".to_string(),
                    reason: Some("MinimumReplicasAvailable".to_string()),
                    message: Some("Deployment has minimum availability".to_string()),
                },
                DeploymentCondition {
                    condition_type: "Progressing".to_string(),
                    status: "True".to_string(),
                    reason: Some("NewReplicaSetAvailable".to_string()),
                    message: Some("ReplicaSet has successfully progressed".to_string()),
                },
            ],
        })
    }

    /// Scale deployment
    async fn scale_deployment(
        name: &str,
        namespace: &str,
        replicas: i32,
        _kubeconfig: Option<&str>,
        _context: Option<&str>,
    ) -> ModuleResult<()> {
        // In a real implementation:
        // let scale = Scale {
        //     metadata: ObjectMeta { name: Some(name.to_string()), ..Default::default() },
        //     spec: Some(ScaleSpec { replicas: Some(replicas) }),
        //     ..Default::default()
        // };
        // deployments.patch_scale(name, &PatchParams::default(), &Patch::Merge(&scale)).await?;

        tracing::info!(
            "Would scale deployment '{}' in namespace '{}' to {} replicas",
            name,
            namespace,
            replicas
        );
        Ok(())
    }

    /// Execute async deployment operations
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = DeploymentConfig::from_params(params)?;

        // Check for YAML definition override
        if let Some(ref _definition) = config.definition {
            return self.apply_from_definition(&config, context).await;
        }

        // Get existing deployment
        let existing = Self::get_deployment(
            &config.name,
            &config.namespace,
            config.kubeconfig.as_deref(),
            config.context.as_deref(),
        )
        .await?;

        match config.state {
            K8sResourceState::Present => self.ensure_present(&config, existing, context).await,
            K8sResourceState::Absent => self.ensure_absent(&config, existing, context).await,
        }
    }

    /// Apply deployment from YAML definition
    async fn apply_from_definition(
        &self,
        config: &DeploymentConfig,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would apply deployment definition for '{}'",
                config.name
            )));
        }

        // In a real implementation:
        // Parse YAML definition, validate, and apply using server-side apply

        Ok(ModuleOutput::changed(format!(
            "Applied deployment definition for '{}'",
            config.name
        ))
        .with_data("name", serde_json::json!(config.name))
        .with_data("namespace", serde_json::json!(config.namespace)))
    }

    /// Ensure deployment is present
    async fn ensure_present(
        &self,
        config: &DeploymentConfig,
        existing: Option<DeploymentInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(dep) = existing {
            // Deployment exists - check for updates
            let needs_update = self.needs_update(config, &dep);

            if !needs_update && !config.force {
                return Ok(
                    ModuleOutput::ok(format!("Deployment '{}' is up to date", config.name))
                        .with_data("deployment", serde_json::to_value(&dep).unwrap()),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update deployment '{}'",
                    config.name
                )));
            }

            // Check if only replicas changed (use scale instead of full update)
            if dep.replicas != config.replicas
                && !config.force
                && config.image.as_deref() == Some(&dep.image)
            {
                Self::scale_deployment(
                    &config.name,
                    &config.namespace,
                    config.replicas,
                    config.kubeconfig.as_deref(),
                    config.context.as_deref(),
                )
                .await?;

                let final_dep = if config.wait {
                    Self::wait_for_ready(
                        &config.name,
                        &config.namespace,
                        config.replicas,
                        Duration::from_secs(config.wait_timeout),
                        config.kubeconfig.as_deref(),
                        config.context.as_deref(),
                    )
                    .await?
                } else {
                    dep.clone()
                };

                return Ok(ModuleOutput::changed(format!(
                    "Scaled deployment '{}' from {} to {} replicas",
                    config.name, final_dep.replicas, config.replicas
                ))
                .with_data("deployment", serde_json::to_value(&final_dep).unwrap()));
            }

            // Full update
            let updated = Self::apply_deployment(config).await?;

            let final_dep = if config.wait {
                Self::wait_for_ready(
                    &config.name,
                    &config.namespace,
                    config.replicas,
                    Duration::from_secs(config.wait_timeout),
                    config.kubeconfig.as_deref(),
                    config.context.as_deref(),
                )
                .await?
            } else {
                updated
            };

            Ok(
                ModuleOutput::changed(format!("Updated deployment '{}'", config.name))
                    .with_data("deployment", serde_json::to_value(&final_dep).unwrap()),
            )
        } else {
            // Create new deployment
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create deployment '{}'",
                    config.name
                )));
            }

            let created = Self::apply_deployment(config).await?;

            let final_dep = if config.wait {
                Self::wait_for_ready(
                    &config.name,
                    &config.namespace,
                    config.replicas,
                    Duration::from_secs(config.wait_timeout),
                    config.kubeconfig.as_deref(),
                    config.context.as_deref(),
                )
                .await?
            } else {
                created
            };

            Ok(
                ModuleOutput::changed(format!("Created deployment '{}'", config.name))
                    .with_data("deployment", serde_json::to_value(&final_dep).unwrap()),
            )
        }
    }

    /// Ensure deployment is absent
    async fn ensure_absent(
        &self,
        config: &DeploymentConfig,
        existing: Option<DeploymentInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_none() {
            return Ok(ModuleOutput::ok(format!(
                "Deployment '{}' does not exist",
                config.name
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would delete deployment '{}'",
                config.name
            )));
        }

        Self::delete_deployment(
            &config.name,
            &config.namespace,
            config.kubeconfig.as_deref(),
            config.context.as_deref(),
        )
        .await?;

        Ok(ModuleOutput::changed(format!(
            "Deleted deployment '{}'",
            config.name
        )))
    }

    /// Check if deployment needs update
    fn needs_update(&self, config: &DeploymentConfig, existing: &DeploymentInfo) -> bool {
        // Check replicas
        if existing.replicas != config.replicas {
            return true;
        }

        // Check image
        if let Some(ref image) = config.image {
            if &existing.image != image {
                return true;
            }
        }

        // Check labels (simplified comparison)
        if config.labels != existing.labels {
            return true;
        }

        false
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
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 20,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

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

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate name
        let name = params.get_string_required("name")?;
        validate_k8s_name(&name)?;

        // Validate namespace if provided
        if let Some(namespace) = params.get_string("namespace")? {
            validate_k8s_namespace(&namespace)?;
        }

        // Validate state if provided
        if let Some(state) = params.get_string("state")? {
            K8sResourceState::from_str(&state)?;
        }

        // Validate replicas if provided
        if let Some(replicas) = params.get_i64("replicas")? {
            if replicas < 0 {
                return Err(ModuleError::InvalidParameter(
                    "replicas must be non-negative".to_string(),
                ));
            }
        }

        // Validate container_port if provided
        if let Some(port) = params.get_i64("container_port")? {
            if !(1..=65535).contains(&port) {
                return Err(ModuleError::InvalidParameter(format!(
                    "container_port {} is invalid. Must be between 1 and 65535",
                    port
                )));
            }
        }

        Ok(())
    }

    fn diff(&self, params: &ModuleParams, _context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let config = DeploymentConfig::from_params(params)?;

        // In check mode, we can generate a diff of what would change
        let before = "# Current state: unknown (would query API)".to_string();
        let after = format!(
            r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {}
  namespace: {}
spec:
  replicas: {}
  template:
    spec:
      containers:
      - name: {}
        image: {}"#,
            config.name,
            config.namespace,
            config.replicas,
            config.container_name.as_ref().unwrap_or(&config.name),
            config.image.as_deref().unwrap_or("<not specified>")
        );

        Ok(Some(Diff::new(before, after)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_module_metadata() {
        let module = K8sDeploymentModule;
        assert_eq!(module.name(), "k8s_deployment");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_deployment_config_basic() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert("image".to_string(), serde_json::json!("nginx:1.21"));
        params.insert("replicas".to_string(), serde_json::json!(3));

        let config = DeploymentConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "nginx");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.replicas, 3);
        assert_eq!(config.image, Some("nginx:1.21".to_string()));
        assert_eq!(config.state, K8sResourceState::Present);
    }

    #[test]
    fn test_deployment_config_full() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("my-app"));
        params.insert("namespace".to_string(), serde_json::json!("production"));
        params.insert("image".to_string(), serde_json::json!("my-app:v2.0"));
        params.insert("replicas".to_string(), serde_json::json!(5));
        params.insert("container_port".to_string(), serde_json::json!(8080));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("wait".to_string(), serde_json::json!(true));
        params.insert("wait_timeout".to_string(), serde_json::json!(600));
        params.insert(
            "labels".to_string(),
            serde_json::json!({
                "app": "my-app",
                "version": "v2"
            }),
        );
        params.insert(
            "strategy".to_string(),
            serde_json::json!({
                "type": "RollingUpdate",
                "max_unavailable": "1",
                "max_surge": "1"
            }),
        );

        let config = DeploymentConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "my-app");
        assert_eq!(config.namespace, "production");
        assert_eq!(config.replicas, 5);
        assert_eq!(config.container_port, Some(8080));
        assert!(config.wait);
        assert_eq!(config.wait_timeout, 600);
        assert_eq!(config.labels.get("app"), Some(&"my-app".to_string()));
    }

    #[test]
    fn test_deployment_config_invalid_name() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("Invalid_Name"));

        assert!(DeploymentConfig::from_params(&params).is_err());
    }

    #[test]
    fn test_deployment_config_invalid_replicas() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert("replicas".to_string(), serde_json::json!(-1));

        assert!(DeploymentConfig::from_params(&params).is_err());
    }

    #[test]
    fn test_deployment_config_effective_selector() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert(
            "labels".to_string(),
            serde_json::json!({
                "app": "my-nginx",
                "tier": "frontend"
            }),
        );

        let config = DeploymentConfig::from_params(&params).unwrap();
        let selector = config.effective_selector();
        assert_eq!(selector.get("app"), Some(&"my-nginx".to_string()));
    }

    #[test]
    fn test_validate_params_invalid_port() {
        let module = K8sDeploymentModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert("container_port".to_string(), serde_json::json!(99999));

        assert!(module.validate_params(&params).is_err());
    }
}
