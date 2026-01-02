//! Kubernetes resource management modules.
//!
//! This module provides comprehensive Kubernetes resource management including:
//!
//! - **Deployment**: Create, update, delete deployments with rolling updates
//! - **Service**: Manage ClusterIP, NodePort, and LoadBalancer services
//! - **ConfigMap**: Configuration data management
//! - **Secret**: Sensitive data management with base64 encoding
//!
//! ## Feature Requirements
//!
//! These modules require the `kubernetes` feature flag:
//!
//! ```toml
//! [dependencies]
//! rustible = { version = "*", features = ["kubernetes"] }
//! ```
//!
//! ## Configuration
//!
//! Kubernetes modules use the following configuration sources (in order):
//!
//! 1. `kubeconfig` parameter - explicit path to kubeconfig file
//! 2. `KUBECONFIG` environment variable
//! 3. `~/.kube/config` default location
//! 4. In-cluster configuration (when running inside a pod)
//!
//! ## Example Usage
//!
//! ```yaml
//! - name: Create deployment
//!   k8s_deployment:
//!     name: nginx-deployment
//!     namespace: default
//!     replicas: 3
//!     image: nginx:1.21
//!     container_port: 80
//!     state: present
//!
//! - name: Create service
//!   k8s_service:
//!     name: nginx-service
//!     namespace: default
//!     selector:
//!       app: nginx
//!     ports:
//!       - port: 80
//!         target_port: 80
//!     type: ClusterIP
//!     state: present
//!
//! - name: Create configmap
//!   k8s_configmap:
//!     name: app-config
//!     namespace: default
//!     data:
//!       config.yaml: |
//!         key: value
//!     state: present
//!
//! - name: Create secret
//!   k8s_secret:
//!     name: app-secrets
//!     namespace: default
//!     type: Opaque
//!     data:
//!       password: supersecret
//!     state: present
//! ```

pub mod configmap;
pub mod deployment;
pub mod secret;
pub mod service;

pub use configmap::K8sConfigMapModule;
pub use deployment::K8sDeploymentModule;
pub use secret::K8sSecretModule;
pub use service::K8sServiceModule;

use crate::modules::{ModuleError, ModuleResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Represents the desired state of a Kubernetes resource
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum K8sResourceState {
    /// Resource should exist
    #[default]
    Present,
    /// Resource should not exist
    Absent,
}

impl K8sResourceState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(K8sResourceState::Present),
            "absent" => Ok(K8sResourceState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Common Kubernetes label selector
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LabelSelector {
    /// Match labels (exact match)
    #[serde(default)]
    pub match_labels: BTreeMap<String, String>,
}

impl LabelSelector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_labels.insert(key.into(), value.into());
        self
    }
}

/// Container port specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerPort {
    /// Port number
    pub container_port: i32,
    /// Protocol (TCP, UDP, SCTP)
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// Optional port name
    pub name: Option<String>,
}

fn default_protocol() -> String {
    "TCP".to_string()
}

/// Environment variable specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvVar {
    /// Simple key-value pair
    Value { name: String, value: String },
    /// Reference to a ConfigMap key
    ConfigMapRef {
        name: String,
        config_map_key_ref: ConfigMapKeyRef,
    },
    /// Reference to a Secret key
    SecretRef {
        name: String,
        secret_key_ref: SecretKeyRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMapKeyRef {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretKeyRef {
    pub name: String,
    pub key: String,
}

/// Resource requirements for containers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceRequirements {
    /// Resource limits
    #[serde(default)]
    pub limits: BTreeMap<String, String>,
    /// Resource requests
    #[serde(default)]
    pub requests: BTreeMap<String, String>,
}

impl ResourceRequirements {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cpu_limit(mut self, cpu: impl Into<String>) -> Self {
        self.limits.insert("cpu".to_string(), cpu.into());
        self
    }

    pub fn with_memory_limit(mut self, memory: impl Into<String>) -> Self {
        self.limits.insert("memory".to_string(), memory.into());
        self
    }

    pub fn with_cpu_request(mut self, cpu: impl Into<String>) -> Self {
        self.requests.insert("cpu".to_string(), cpu.into());
        self
    }

    pub fn with_memory_request(mut self, memory: impl Into<String>) -> Self {
        self.requests.insert("memory".to_string(), memory.into());
        self
    }
}

/// Volume mount specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Volume name (must match a volume in the pod spec)
    pub name: String,
    /// Path within the container
    pub mount_path: String,
    /// Path within the volume to mount
    pub sub_path: Option<String>,
    /// Mount read-only
    #[serde(default)]
    pub read_only: bool,
}

/// Volume specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Volume {
    /// EmptyDir volume
    EmptyDir {
        name: String,
        #[serde(default)]
        medium: Option<String>,
        size_limit: Option<String>,
    },
    /// ConfigMap volume
    ConfigMap {
        name: String,
        config_map_name: String,
        #[serde(default)]
        items: Vec<KeyToPath>,
        #[serde(default)]
        optional: bool,
    },
    /// Secret volume
    Secret {
        name: String,
        secret_name: String,
        #[serde(default)]
        items: Vec<KeyToPath>,
        #[serde(default)]
        optional: bool,
    },
    /// PersistentVolumeClaim volume
    PersistentVolumeClaim {
        name: String,
        claim_name: String,
        #[serde(default)]
        read_only: bool,
    },
    /// HostPath volume (use with caution)
    HostPath {
        name: String,
        path: String,
        #[serde(default)]
        host_path_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyToPath {
    pub key: String,
    pub path: String,
    pub mode: Option<i32>,
}

/// Container probe configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Probe {
    /// HTTP GET probe
    HttpGet {
        path: String,
        port: i32,
        #[serde(default)]
        scheme: Option<String>,
        #[serde(default)]
        http_headers: Vec<HttpHeader>,
    },
    /// TCP socket probe
    TcpSocket { port: i32 },
    /// Exec probe (runs a command)
    Exec { command: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// Probe timing configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProbeConfig {
    /// Probe specification
    pub probe: Option<Probe>,
    /// Initial delay in seconds
    #[serde(default = "default_initial_delay")]
    pub initial_delay_seconds: i32,
    /// Period between probes in seconds
    #[serde(default = "default_period")]
    pub period_seconds: i32,
    /// Timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i32,
    /// Success threshold
    #[serde(default = "default_threshold")]
    pub success_threshold: i32,
    /// Failure threshold
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: i32,
}

fn default_initial_delay() -> i32 {
    0
}
fn default_period() -> i32 {
    10
}
fn default_timeout() -> i32 {
    1
}
fn default_threshold() -> i32 {
    1
}
fn default_failure_threshold() -> i32 {
    3
}

/// Rolling update strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingUpdateConfig {
    /// Maximum number of pods that can be unavailable during update
    #[serde(default = "default_max_unavailable")]
    pub max_unavailable: String,
    /// Maximum number of pods that can be created above desired count
    #[serde(default = "default_max_surge")]
    pub max_surge: String,
}

fn default_max_unavailable() -> String {
    "25%".to_string()
}

fn default_max_surge() -> String {
    "25%".to_string()
}

impl Default for RollingUpdateConfig {
    fn default() -> Self {
        Self {
            max_unavailable: default_max_unavailable(),
            max_surge: default_max_surge(),
        }
    }
}

/// Deployment update strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum UpdateStrategy {
    /// Rolling update (default)
    RollingUpdate {
        #[serde(flatten)]
        config: RollingUpdateConfig,
    },
    /// Recreate (kill all pods before creating new ones)
    Recreate,
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        UpdateStrategy::RollingUpdate {
            config: RollingUpdateConfig::default(),
        }
    }
}

/// Service port specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePort {
    /// Port exposed by the service
    pub port: i32,
    /// Target port on the pods
    pub target_port: Option<i32>,
    /// Node port (for NodePort/LoadBalancer services)
    pub node_port: Option<i32>,
    /// Protocol (TCP, UDP, SCTP)
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// Port name
    pub name: Option<String>,
}

/// Service type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ServiceType {
    /// ClusterIP service (default)
    #[default]
    ClusterIP,
    /// NodePort service
    NodePort,
    /// LoadBalancer service
    LoadBalancer,
    /// ExternalName service
    ExternalName,
    /// Headless service (ClusterIP: None)
    Headless,
}

impl ServiceType {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().replace('-', "").replace('_', "").as_str() {
            "clusterip" => Ok(ServiceType::ClusterIP),
            "nodeport" => Ok(ServiceType::NodePort),
            "loadbalancer" => Ok(ServiceType::LoadBalancer),
            "externalname" => Ok(ServiceType::ExternalName),
            "headless" | "none" => Ok(ServiceType::Headless),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid service type '{}'. Valid types: ClusterIP, NodePort, LoadBalancer, ExternalName, Headless",
                s
            ))),
        }
    }

    pub fn to_k8s_type(&self) -> &'static str {
        match self {
            ServiceType::ClusterIP => "ClusterIP",
            ServiceType::NodePort => "NodePort",
            ServiceType::LoadBalancer => "LoadBalancer",
            ServiceType::ExternalName => "ExternalName",
            ServiceType::Headless => "ClusterIP", // Headless uses ClusterIP: None
        }
    }
}

/// Secret type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SecretType {
    /// Opaque secret (default)
    #[default]
    Opaque,
    /// Docker registry credentials
    DockerConfigJson,
    /// Basic authentication
    BasicAuth,
    /// SSH authentication
    SshAuth,
    /// TLS certificate
    Tls,
    /// Bootstrap token
    BootstrapToken,
    /// Service account token
    ServiceAccountToken,
}

impl SecretType {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().replace(['-', '_', '/', '.'], "").as_str() {
            "opaque" => Ok(SecretType::Opaque),
            "dockerconfigjson" | "kubernetesiodockerconfigjson" => Ok(SecretType::DockerConfigJson),
            "basicauth" | "kubernetesiobasicauth" => Ok(SecretType::BasicAuth),
            "sshauth" | "kubernetesiosshauth" => Ok(SecretType::SshAuth),
            "tls" | "kubernetesiotls" => Ok(SecretType::Tls),
            "bootstraptoken" | "bootstrapkubernetesiotoken" => Ok(SecretType::BootstrapToken),
            "serviceaccounttoken" | "kubernetesioserviceaccounttoken" => {
                Ok(SecretType::ServiceAccountToken)
            }
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid secret type '{}'. Valid types: Opaque, DockerConfigJson, BasicAuth, SshAuth, Tls, BootstrapToken, ServiceAccountToken",
                s
            ))),
        }
    }

    pub fn to_k8s_type(&self) -> &'static str {
        match self {
            SecretType::Opaque => "Opaque",
            SecretType::DockerConfigJson => "kubernetes.io/dockerconfigjson",
            SecretType::BasicAuth => "kubernetes.io/basic-auth",
            SecretType::SshAuth => "kubernetes.io/ssh-auth",
            SecretType::Tls => "kubernetes.io/tls",
            SecretType::BootstrapToken => "bootstrap.kubernetes.io/token",
            SecretType::ServiceAccountToken => "kubernetes.io/service-account-token",
        }
    }
}

/// Parse labels from module parameters
pub fn parse_labels(value: &serde_json::Value) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if let Some(vs) = v.as_str() {
                labels.insert(k.clone(), vs.to_string());
            } else {
                labels.insert(k.clone(), v.to_string().trim_matches('"').to_string());
            }
        }
    }
    labels
}

/// Parse annotations from module parameters
pub fn parse_annotations(value: &serde_json::Value) -> BTreeMap<String, String> {
    parse_labels(value) // Same format as labels
}

/// Validate Kubernetes resource name
pub fn validate_k8s_name(name: &str) -> ModuleResult<()> {
    // Kubernetes names must:
    // - be 253 characters or less
    // - begin and end with lowercase alphanumeric
    // - contain only lowercase alphanumeric, '-', or '.'
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Resource name cannot be empty".to_string(),
        ));
    }

    if name.len() > 253 {
        return Err(ModuleError::InvalidParameter(format!(
            "Resource name '{}' exceeds 253 character limit",
            name
        )));
    }

    let chars: Vec<char> = name.chars().collect();
    if !chars[0].is_ascii_lowercase() && !chars[0].is_ascii_digit() {
        return Err(ModuleError::InvalidParameter(format!(
            "Resource name '{}' must start with lowercase alphanumeric",
            name
        )));
    }

    if !chars.last().unwrap().is_ascii_lowercase() && !chars.last().unwrap().is_ascii_digit() {
        return Err(ModuleError::InvalidParameter(format!(
            "Resource name '{}' must end with lowercase alphanumeric",
            name
        )));
    }

    for c in &chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && *c != '-' && *c != '.' {
            return Err(ModuleError::InvalidParameter(format!(
                "Resource name '{}' contains invalid character '{}'. Only lowercase alphanumeric, '-', and '.' are allowed",
                name, c
            )));
        }
    }

    Ok(())
}

/// Validate Kubernetes namespace name
pub fn validate_k8s_namespace(namespace: &str) -> ModuleResult<()> {
    // Namespace names have stricter rules - no dots allowed
    if namespace.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Namespace cannot be empty".to_string(),
        ));
    }

    if namespace.len() > 63 {
        return Err(ModuleError::InvalidParameter(format!(
            "Namespace '{}' exceeds 63 character limit",
            namespace
        )));
    }

    let chars: Vec<char> = namespace.chars().collect();
    if !chars[0].is_ascii_lowercase() && !chars[0].is_ascii_digit() {
        return Err(ModuleError::InvalidParameter(format!(
            "Namespace '{}' must start with lowercase alphanumeric",
            namespace
        )));
    }

    if !chars.last().unwrap().is_ascii_lowercase() && !chars.last().unwrap().is_ascii_digit() {
        return Err(ModuleError::InvalidParameter(format!(
            "Namespace '{}' must end with lowercase alphanumeric",
            namespace
        )));
    }

    for c in &chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && *c != '-' {
            return Err(ModuleError::InvalidParameter(format!(
                "Namespace '{}' contains invalid character '{}'. Only lowercase alphanumeric and '-' are allowed",
                namespace, c
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_state_from_str() {
        assert_eq!(
            K8sResourceState::from_str("present").unwrap(),
            K8sResourceState::Present
        );
        assert_eq!(
            K8sResourceState::from_str("absent").unwrap(),
            K8sResourceState::Absent
        );
        assert!(K8sResourceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_service_type_from_str() {
        assert_eq!(
            ServiceType::from_str("ClusterIP").unwrap(),
            ServiceType::ClusterIP
        );
        assert_eq!(
            ServiceType::from_str("nodeport").unwrap(),
            ServiceType::NodePort
        );
        assert_eq!(
            ServiceType::from_str("LoadBalancer").unwrap(),
            ServiceType::LoadBalancer
        );
        assert_eq!(
            ServiceType::from_str("headless").unwrap(),
            ServiceType::Headless
        );
        assert!(ServiceType::from_str("invalid").is_err());
    }

    #[test]
    fn test_secret_type_from_str() {
        assert_eq!(SecretType::from_str("Opaque").unwrap(), SecretType::Opaque);
        assert_eq!(SecretType::from_str("tls").unwrap(), SecretType::Tls);
        assert_eq!(
            SecretType::from_str("kubernetes.io/dockerconfigjson").unwrap(),
            SecretType::DockerConfigJson
        );
        assert!(SecretType::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_k8s_name_valid() {
        assert!(validate_k8s_name("nginx").is_ok());
        assert!(validate_k8s_name("nginx-deployment").is_ok());
        assert!(validate_k8s_name("app.example.com").is_ok());
        assert!(validate_k8s_name("my-app-123").is_ok());
        assert!(validate_k8s_name("a").is_ok());
    }

    #[test]
    fn test_validate_k8s_name_invalid() {
        assert!(validate_k8s_name("").is_err());
        assert!(validate_k8s_name("Nginx").is_err()); // uppercase
        assert!(validate_k8s_name("-nginx").is_err()); // starts with dash
        assert!(validate_k8s_name("nginx-").is_err()); // ends with dash
        assert!(validate_k8s_name("nginx_app").is_err()); // underscore
    }

    #[test]
    fn test_validate_k8s_namespace_valid() {
        assert!(validate_k8s_namespace("default").is_ok());
        assert!(validate_k8s_namespace("kube-system").is_ok());
        assert!(validate_k8s_namespace("my-namespace").is_ok());
    }

    #[test]
    fn test_validate_k8s_namespace_invalid() {
        assert!(validate_k8s_namespace("").is_err());
        assert!(validate_k8s_namespace("my.namespace").is_err()); // dots not allowed
        assert!(validate_k8s_namespace("MyNamespace").is_err()); // uppercase
    }

    #[test]
    fn test_label_selector() {
        let selector = LabelSelector::new()
            .with_label("app", "nginx")
            .with_label("version", "v1");
        assert_eq!(selector.match_labels.get("app"), Some(&"nginx".to_string()));
        assert_eq!(
            selector.match_labels.get("version"),
            Some(&"v1".to_string())
        );
    }

    #[test]
    fn test_resource_requirements() {
        let resources = ResourceRequirements::new()
            .with_cpu_limit("1")
            .with_memory_limit("512Mi")
            .with_cpu_request("500m")
            .with_memory_request("256Mi");

        assert_eq!(resources.limits.get("cpu"), Some(&"1".to_string()));
        assert_eq!(resources.limits.get("memory"), Some(&"512Mi".to_string()));
        assert_eq!(resources.requests.get("cpu"), Some(&"500m".to_string()));
        assert_eq!(resources.requests.get("memory"), Some(&"256Mi".to_string()));
    }
}
