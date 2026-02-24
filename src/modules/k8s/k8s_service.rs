//! Kubernetes Service module - Service resource management
//!
//! This module manages Kubernetes Service resources using the kube-rs crate.
//! It supports creating, updating, and deleting Services with various types
//! including ClusterIP, NodePort, LoadBalancer, and ExternalName.
//!
//! ## Parameters
//!
//! - `name`: Service name (required)
//! - `namespace`: Kubernetes namespace (default: "default")
//! - `state`: Desired state (present, absent) (default: "present")
//! - `type`: Service type (ClusterIP, NodePort, LoadBalancer, ExternalName)
//! - `selector`: Label selector for pods (required for ClusterIP/NodePort/LoadBalancer)
//! - `ports`: List of port mappings
//! - `cluster_ip`: Cluster IP address (optional, use "None" for headless)
//! - `external_ips`: List of external IP addresses
//! - `external_name`: External DNS name (for ExternalName type)
//! - `load_balancer_ip`: Requested load balancer IP
//! - `session_affinity`: Session affinity (None, ClientIP)
//! - `labels`: Labels to apply to the Service
//! - `annotations`: Annotations to apply to the Service
//!
//! ## Example
//!
//! ```yaml
//! - name: Create nginx service
//!   k8s_service:
//!     name: nginx-service
//!     namespace: default
//!     type: ClusterIP
//!     selector:
//!       app: nginx
//!     ports:
//!       - port: 80
//!         target_port: 80
//!         protocol: TCP
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
use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
#[cfg(feature = "kubernetes")]
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
#[cfg(feature = "kubernetes")]
use kube::{
    api::{Api, DeleteParams, Patch, PatchParams, PostParams},
    Client,
};

/// Desired state for a Kubernetes Service
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceState {
    /// Service should exist
    Present,
    /// Service should not exist
    Absent,
}

impl ServiceState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(ServiceState::Present),
            "absent" => Ok(ServiceState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Service type
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
    ExternalName,
}

impl ServiceType {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "clusterip" | "cluster_ip" => Ok(ServiceType::ClusterIP),
            "nodeport" | "node_port" => Ok(ServiceType::NodePort),
            "loadbalancer" | "load_balancer" => Ok(ServiceType::LoadBalancer),
            "externalname" | "external_name" => Ok(ServiceType::ExternalName),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid service type '{}'. Valid types: ClusterIP, NodePort, LoadBalancer, ExternalName",
                s
            ))),
        }
    }

    fn to_k8s_string(&self) -> String {
        match self {
            ServiceType::ClusterIP => "ClusterIP".to_string(),
            ServiceType::NodePort => "NodePort".to_string(),
            ServiceType::LoadBalancer => "LoadBalancer".to_string(),
            ServiceType::ExternalName => "ExternalName".to_string(),
        }
    }
}

/// Port configuration for a Service
#[derive(Debug, Clone)]
struct PortConfig {
    name: Option<String>,
    port: i32,
    target_port: Option<i32>,
    node_port: Option<i32>,
    protocol: String,
}

/// Service module configuration
#[derive(Debug, Clone)]
struct ServiceConfig {
    name: String,
    namespace: String,
    state: ServiceState,
    service_type: ServiceType,
    selector: HashMap<String, String>,
    ports: Vec<PortConfig>,
    cluster_ip: Option<String>,
    external_ips: Vec<String>,
    external_name: Option<String>,
    load_balancer_ip: Option<String>,
    session_affinity: Option<String>,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
}

impl ServiceConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let namespace = params
            .get_string("namespace")?
            .unwrap_or_else(|| "default".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = ServiceState::from_str(&state_str)?;
        let type_str = params
            .get_string("type")?
            .unwrap_or_else(|| "ClusterIP".to_string());
        let service_type = ServiceType::from_str(&type_str)?;

        let selector = Self::parse_string_map(params, "selector")?;
        let ports = Self::parse_ports(params)?;
        let cluster_ip = params.get_string("cluster_ip")?;
        let external_ips = params.get_vec_string("external_ips")?.unwrap_or_default();
        let external_name = params.get_string("external_name")?;
        let load_balancer_ip = params.get_string("load_balancer_ip")?;
        let session_affinity = params.get_string("session_affinity")?;
        let labels = Self::parse_string_map(params, "labels")?;
        let annotations = Self::parse_string_map(params, "annotations")?;

        Ok(Self {
            name,
            namespace,
            state,
            service_type,
            selector,
            ports,
            cluster_ip,
            external_ips,
            external_name,
            load_balancer_ip,
            session_affinity,
            labels,
            annotations,
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

    fn parse_ports(params: &ModuleParams) -> ModuleResult<Vec<PortConfig>> {
        let mut ports = Vec::new();

        match params.get("ports") {
            Some(serde_json::Value::Array(arr)) => {
                for item in arr {
                    let port = match item.get("port") {
                        Some(serde_json::Value::Number(n)) => n.as_i64().ok_or_else(|| {
                            ModuleError::InvalidParameter("port must be an integer".to_string())
                        })? as i32,
                        _ => {
                            return Err(ModuleError::InvalidParameter(
                                "Each port entry must have a 'port' field".to_string(),
                            ))
                        }
                    };

                    let target_port = item
                        .get("target_port")
                        .and_then(|v| v.as_i64().map(|n| n as i32));

                    let node_port = item
                        .get("node_port")
                        .and_then(|v| v.as_i64().map(|n| n as i32));

                    let name = item.get("name").and_then(|v| v.as_str()).map(String::from);

                    let protocol = item
                        .get("protocol")
                        .and_then(|v| v.as_str())
                        .unwrap_or("TCP")
                        .to_uppercase();

                    ports.push(PortConfig {
                        name,
                        port,
                        target_port,
                        node_port,
                        protocol,
                    });
                }
            }
            Some(_) => {
                return Err(ModuleError::InvalidParameter(
                    "ports must be an array".to_string(),
                ))
            }
            None => {}
        }

        Ok(ports)
    }
}

/// Module for Kubernetes Service management
pub struct K8sServiceModule;

#[cfg(feature = "kubernetes")]
impl K8sServiceModule {
    /// Build a Service resource from configuration
    fn build_service(config: &ServiceConfig) -> ModuleResult<Service> {
        // Build service ports
        let ports: Vec<ServicePort> = config
            .ports
            .iter()
            .map(|p| ServicePort {
                name: p.name.clone(),
                port: p.port,
                target_port: p.target_port.map(IntOrString::Int),
                node_port: p.node_port,
                protocol: Some(p.protocol.clone()),
                ..Default::default()
            })
            .collect();

        // Build the Service spec
        let mut spec = ServiceSpec {
            type_: Some(config.service_type.to_k8s_string()),
            ports: if ports.is_empty() { None } else { Some(ports) },
            cluster_ip: config.cluster_ip.clone(),
            external_ips: if config.external_ips.is_empty() {
                None
            } else {
                Some(config.external_ips.clone())
            },
            external_name: config.external_name.clone(),
            load_balancer_ip: config.load_balancer_ip.clone(),
            session_affinity: config.session_affinity.clone(),
            ..Default::default()
        };

        // Set selector (not for ExternalName services)
        if config.service_type != ServiceType::ExternalName && !config.selector.is_empty() {
            spec.selector = Some(
                config
                    .selector
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            );
        }

        // Build labels
        let mut labels = config.labels.clone();
        if !labels.contains_key("app") {
            labels.insert("app".to_string(), config.name.clone());
        }

        let service = Service {
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
            spec: Some(spec),
            ..Default::default()
        };

        Ok(service)
    }

    /// Execute the service module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ServiceConfig::from_params(params)?;

        // Create Kubernetes client
        let client = Client::try_default().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create Kubernetes client: {}", e))
        })?;

        let services: Api<Service> = Api::namespaced(client.clone(), &config.namespace);

        match config.state {
            ServiceState::Absent => {
                // Check if service exists
                match services.get(&config.name).await {
                    Ok(_) => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would delete Service '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Delete the service
                        services
                            .delete(&config.name, &DeleteParams::default())
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to delete Service: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Deleted Service '{}/{}'",
                            config.namespace, config.name
                        )))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => Ok(ModuleOutput::ok(format!(
                        "Service '{}/{}' already absent",
                        config.namespace, config.name
                    ))),
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Service: {}",
                        e
                    ))),
                }
            }
            ServiceState::Present => {
                let service = Self::build_service(&config)?;

                // Check if service exists
                match services.get(&config.name).await {
                    Ok(existing) => {
                        // Compare and update if needed
                        let needs_update = Self::needs_update(&existing, &service);

                        if !needs_update {
                            return Ok(ModuleOutput::ok(format!(
                                "Service '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data(
                                "cluster_ip",
                                serde_json::json!(existing
                                    .spec
                                    .as_ref()
                                    .and_then(|s| s.cluster_ip.clone())),
                            ));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update Service '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Apply patch to update (preserving clusterIP)
                        let mut patched_service = service.clone();
                        if let Some(ref mut spec) = patched_service.spec {
                            // Preserve existing clusterIP if not explicitly set
                            if config.cluster_ip.is_none() {
                                spec.cluster_ip =
                                    existing.spec.as_ref().and_then(|s| s.cluster_ip.clone());
                            }
                        }

                        let patch = Patch::Merge(&patched_service);
                        let result = services
                            .patch(&config.name, &PatchParams::apply("rustible"), &patch)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to update Service: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Updated Service '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data(
                            "cluster_ip",
                            serde_json::json!(result
                                .spec
                                .as_ref()
                                .and_then(|s| s.cluster_ip.clone())),
                        ))
                    }
                    Err(kube::Error::Api(e)) if e.code == 404 => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create Service '{}/{}'",
                                config.namespace, config.name
                            )));
                        }

                        // Create new service
                        let result = services
                            .create(&PostParams::default(), &service)
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to create Service: {}",
                                    e
                                ))
                            })?;

                        Ok(ModuleOutput::changed(format!(
                            "Created Service '{}/{}'",
                            config.namespace, config.name
                        ))
                        .with_data(
                            "cluster_ip",
                            serde_json::json!(result
                                .spec
                                .as_ref()
                                .and_then(|s| s.cluster_ip.clone())),
                        ))
                    }
                    Err(e) => Err(ModuleError::ExecutionFailed(format!(
                        "Failed to get Service: {}",
                        e
                    ))),
                }
            }
        }
    }

    /// Check if service needs to be updated
    fn needs_update(existing: &Service, desired: &Service) -> bool {
        // Compare service type
        let existing_type = existing.spec.as_ref().and_then(|s| s.type_.as_ref());
        let desired_type = desired.spec.as_ref().and_then(|s| s.type_.as_ref());
        if existing_type != desired_type {
            return true;
        }

        // Compare ports
        let existing_ports = existing.spec.as_ref().and_then(|s| s.ports.as_ref());
        let desired_ports = desired.spec.as_ref().and_then(|s| s.ports.as_ref());
        if let (Some(ep), Some(dp)) = (existing_ports, desired_ports) {
            if ep.len() != dp.len() {
                return true;
            }
            // Simple comparison - could be more sophisticated
            for (e, d) in ep.iter().zip(dp.iter()) {
                if e.port != d.port || e.target_port != d.target_port {
                    return true;
                }
            }
        } else if existing_ports.is_some() != desired_ports.is_some() {
            return true;
        }

        // Compare selector
        let existing_selector = existing.spec.as_ref().and_then(|s| s.selector.as_ref());
        let desired_selector = desired.spec.as_ref().and_then(|s| s.selector.as_ref());
        if existing_selector != desired_selector {
            return true;
        }

        false
    }
}

#[cfg(not(feature = "kubernetes"))]
impl K8sServiceModule {
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
        let config = ServiceConfig::from_params(params)?;
        let name_escaped = shell_escape(&config.name);
        let ns_escaped = shell_escape(&config.namespace);

        // Check if service already exists
        let check_cmd = format!(
            "kubectl get service {} -n {} -o json 2>/dev/null",
            name_escaped, ns_escaped
        );
        let (exists, existing_json, _) = Self::run_cmd(&check_cmd, context)?;

        match config.state {
            ServiceState::Absent => {
                if !exists {
                    return Ok(ModuleOutput::ok(format!(
                        "Service '{}/{}' already absent",
                        config.namespace, config.name
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete Service '{}/{}'",
                        config.namespace, config.name
                    )));
                }

                let delete_cmd =
                    format!("kubectl delete service {} -n {}", name_escaped, ns_escaped);
                let (success, _, stderr) = Self::run_cmd(&delete_cmd, context)?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to delete Service: {}",
                        stderr
                    )));
                }

                Ok(ModuleOutput::changed(format!(
                    "Deleted Service '{}/{}'",
                    config.namespace, config.name
                )))
            }
            ServiceState::Present => {
                // Build labels
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

                // Build ports
                let ports_json: serde_json::Value = if config.ports.is_empty() {
                    serde_json::Value::Null
                } else {
                    let ports: Vec<serde_json::Value> = config
                        .ports
                        .iter()
                        .map(|p| {
                            let mut port_obj = serde_json::json!({
                                "port": p.port,
                                "protocol": p.protocol,
                            });
                            if let Some(tp) = p.target_port {
                                port_obj["targetPort"] = serde_json::json!(tp);
                            }
                            if let Some(np) = p.node_port {
                                port_obj["nodePort"] = serde_json::json!(np);
                            }
                            if let Some(ref name) = p.name {
                                port_obj["name"] = serde_json::json!(name);
                            }
                            port_obj
                        })
                        .collect();
                    serde_json::json!(ports)
                };

                // Build selector
                let selector_json: serde_json::Value = if config.service_type
                    != ServiceType::ExternalName
                    && !config.selector.is_empty()
                {
                    config
                        .selector
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into()
                } else {
                    serde_json::Value::Null
                };

                // Build spec
                let mut spec = serde_json::json!({
                    "type": config.service_type.to_k8s_string(),
                });

                if !ports_json.is_null() {
                    spec["ports"] = ports_json;
                }
                if !selector_json.is_null() {
                    spec["selector"] = selector_json;
                }
                if let Some(ref cip) = config.cluster_ip {
                    spec["clusterIP"] = serde_json::json!(cip);
                }
                if !config.external_ips.is_empty() {
                    spec["externalIPs"] = serde_json::json!(config.external_ips);
                }
                if let Some(ref en) = config.external_name {
                    spec["externalName"] = serde_json::json!(en);
                }
                if let Some(ref lbip) = config.load_balancer_ip {
                    spec["loadBalancerIP"] = serde_json::json!(lbip);
                }
                if let Some(ref sa) = config.session_affinity {
                    spec["sessionAffinity"] = serde_json::json!(sa);
                }

                let mut manifest = serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Service",
                    "metadata": {
                        "name": config.name,
                        "namespace": config.namespace,
                        "labels": labels_json,
                    },
                    "spec": spec,
                });

                if !config.annotations.is_empty() {
                    manifest["metadata"]["annotations"] = annotations_json;
                }

                if exists {
                    // Parse existing for comparison
                    if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_json)
                    {
                        let existing_type = existing.pointer("/spec/type").and_then(|v| v.as_str());
                        let desired_type = Some(config.service_type.to_k8s_string());

                        let existing_selector = existing.pointer("/spec/selector");
                        let desired_selector = manifest.pointer("/spec/selector");

                        // Compare ports (simplified)
                        let existing_ports = existing.pointer("/spec/ports");
                        let desired_ports = manifest.pointer("/spec/ports");

                        let ports_match = match (existing_ports, desired_ports) {
                            (Some(ep), Some(dp)) => {
                                if let (Some(ea), Some(da)) = (ep.as_array(), dp.as_array()) {
                                    if ea.len() != da.len() {
                                        false
                                    } else {
                                        ea.iter().zip(da.iter()).all(|(e, d)| {
                                            e.get("port") == d.get("port")
                                                && e.get("targetPort") == d.get("targetPort")
                                        })
                                    }
                                } else {
                                    false
                                }
                            }
                            (None, None) => true,
                            _ => false,
                        };

                        if existing_type == desired_type.as_deref()
                            && existing_selector == desired_selector
                            && ports_match
                        {
                            let cluster_ip = existing
                                .pointer("/spec/clusterIP")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            return Ok(ModuleOutput::ok(format!(
                                "Service '{}/{}' is up to date",
                                config.namespace, config.name
                            ))
                            .with_data("cluster_ip", serde_json::json!(cluster_ip)));
                        }
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update Service '{}/{}'",
                            config.namespace, config.name
                        )));
                    }

                    // Preserve existing clusterIP if not explicitly set
                    if config.cluster_ip.is_none() {
                        if let Ok(existing) =
                            serde_json::from_str::<serde_json::Value>(&existing_json)
                        {
                            if let Some(cip) =
                                existing.pointer("/spec/clusterIP").and_then(|v| v.as_str())
                            {
                                manifest["spec"]["clusterIP"] = serde_json::json!(cip);
                            }
                        }
                    }

                    let apply_cmd = format!(
                        "echo {} | kubectl apply -f -",
                        shell_escape(&manifest.to_string())
                    );
                    let (success, _, stderr) = Self::run_cmd(&apply_cmd, context)?;
                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to update Service: {}",
                            stderr
                        )));
                    }

                    // Get the updated service info for cluster_ip
                    let get_cmd = format!(
                        "kubectl get service {} -n {} -o json 2>/dev/null",
                        name_escaped, ns_escaped
                    );
                    let (_, updated_json, _) = Self::run_cmd(&get_cmd, context)?;
                    let cluster_ip = serde_json::from_str::<serde_json::Value>(&updated_json)
                        .ok()
                        .and_then(|v| {
                            v.pointer("/spec/clusterIP")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        });

                    Ok(ModuleOutput::changed(format!(
                        "Updated Service '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("cluster_ip", serde_json::json!(cluster_ip)))
                } else {
                    // Create new service
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create Service '{}/{}'",
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
                            "Failed to create Service: {}",
                            stderr
                        )));
                    }

                    // Get the created service info for cluster_ip
                    let get_cmd = format!(
                        "kubectl get service {} -n {} -o json 2>/dev/null",
                        name_escaped, ns_escaped
                    );
                    let (_, created_json, _) = Self::run_cmd(&get_cmd, context)?;
                    let cluster_ip = serde_json::from_str::<serde_json::Value>(&created_json)
                        .ok()
                        .and_then(|v| {
                            v.pointer("/spec/clusterIP")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        });

                    Ok(ModuleOutput::changed(format!(
                        "Created Service '{}/{}'",
                        config.namespace, config.name
                    ))
                    .with_data("cluster_ip", serde_json::json!(cluster_ip)))
                }
            }
        }
    }
}

impl Module for K8sServiceModule {
    fn name(&self) -> &'static str {
        "k8s_service"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes Services"
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
    fn test_service_state_from_str() {
        assert_eq!(
            ServiceState::from_str("present").unwrap(),
            ServiceState::Present
        );
        assert_eq!(
            ServiceState::from_str("absent").unwrap(),
            ServiceState::Absent
        );
        assert!(ServiceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_service_type_from_str() {
        assert_eq!(
            ServiceType::from_str("ClusterIP").unwrap(),
            ServiceType::ClusterIP
        );
        assert_eq!(
            ServiceType::from_str("NodePort").unwrap(),
            ServiceType::NodePort
        );
        assert_eq!(
            ServiceType::from_str("LoadBalancer").unwrap(),
            ServiceType::LoadBalancer
        );
        assert_eq!(
            ServiceType::from_str("ExternalName").unwrap(),
            ServiceType::ExternalName
        );
        assert!(ServiceType::from_str("invalid").is_err());
    }

    #[test]
    fn test_service_config_from_params() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("nginx-svc"));
        params.insert("namespace".to_string(), serde_json::json!("default"));
        params.insert("type".to_string(), serde_json::json!("ClusterIP"));

        let config = ServiceConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "nginx-svc");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.service_type, ServiceType::ClusterIP);
        assert_eq!(config.state, ServiceState::Present);
    }

    #[test]
    fn test_service_module_metadata() {
        let module = K8sServiceModule;
        assert_eq!(module.name(), "k8s_service");
        assert_eq!(module.required_params(), &["name"]);
    }
}
