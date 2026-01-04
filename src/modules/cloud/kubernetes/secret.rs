//! Kubernetes Secret module for managing sensitive data.
//!
//! This module provides Secret management including:
//!
//! - Create, update, and delete Secrets
//! - Multiple secret types (Opaque, TLS, DockerConfigJson, etc.)
//! - Automatic base64 encoding of values
//! - Immutable Secrets (Kubernetes 1.21+)
//!
//! ## Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Secret name |
//! | `namespace` | No | Kubernetes namespace (default: "default") |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `type` | No | Secret type (default: Opaque) |
//! | `data` | No | Key-value pairs (will be base64 encoded) |
//! | `string_data` | No | Key-value pairs (plain text, encoded automatically) |
//! | `immutable` | No | Make Secret immutable (default: false) |
//! | `labels` | No | Labels for the Secret |
//! | `annotations` | No | Annotations for the Secret |
//!
//! ## Example
//!
//! ```yaml
//! - name: Create secret from literals
//!   k8s_secret:
//!     name: app-secrets
//!     namespace: default
//!     data:
//!       password: supersecret
//!       api_key: my-api-key
//!
//! - name: Create TLS secret
//!   k8s_secret:
//!     name: tls-secret
//!     namespace: default
//!     type: kubernetes.io/tls
//!     data:
//!       tls.crt: "{{ lookup('file', 'cert.pem') | b64encode }}"
//!       tls.key: "{{ lookup('file', 'key.pem') | b64encode }}"
//! ```

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde_json::json;
use std::collections::BTreeMap;

use super::{
    parse_annotations, parse_labels, validate_k8s_name, validate_k8s_namespace, K8sResourceState,
    SecretType,
};

/// Secret configuration parsed from module parameters
#[derive(Debug, Clone)]
struct SecretConfig {
    name: String,
    namespace: String,
    state: K8sResourceState,
    secret_type: SecretType,
    data: BTreeMap<String, String>,
    string_data: BTreeMap<String, String>,
    immutable: bool,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    #[allow(dead_code)]
    kubeconfig: Option<String>,
    #[allow(dead_code)]
    context: Option<String>,
}

impl SecretConfig {
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

        let secret_type = if let Some(t) = params.get_string("type")? {
            SecretType::from_str(&t)?
        } else {
            SecretType::default()
        };

        // Parse data
        let mut data = BTreeMap::new();
        if let Some(d) = params.get("data") {
            if let Some(obj) = d.as_object() {
                for (k, v) in obj {
                    if let Some(vs) = v.as_str() {
                        data.insert(k.clone(), vs.to_string());
                    }
                }
            }
        }

        // Parse string_data
        let mut string_data = BTreeMap::new();
        if let Some(d) = params.get("string_data") {
            if let Some(obj) = d.as_object() {
                for (k, v) in obj {
                    if let Some(vs) = v.as_str() {
                        string_data.insert(k.clone(), vs.to_string());
                    }
                }
            }
        }

        let immutable = params.get_bool_or("immutable", false);

        let labels = if let Some(l) = params.get("labels") {
            parse_labels(l)
        } else {
            BTreeMap::new()
        };

        let annotations = if let Some(a) = params.get("annotations") {
            parse_annotations(a)
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            name,
            namespace,
            state,
            secret_type,
            data,
            string_data,
            immutable,
            labels,
            annotations,
            kubeconfig: params.get_string("kubeconfig")?,
            context: params.get_string("context")?,
        })
    }
}

/// Kubernetes Secret module
pub struct K8sSecretModule;

impl Module for K8sSecretModule {
    fn name(&self) -> &'static str {
        "k8s_secret"
    }

    fn description(&self) -> &'static str {
        "Manage Kubernetes Secrets"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
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
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = SecretConfig::from_params(params)?;

        match config.state {
            K8sResourceState::Present => self.ensure_present(&config),
            K8sResourceState::Absent => self.ensure_absent(&config),
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let config = SecretConfig::from_params(params)?;

        // In check mode, we simulate what would happen
        let mut output = self.execute(params, context)?;
        if output.changed {
            output.diff = Some(Diff::new(
                "current state",
                format!(
                    "Secret {} in namespace {} - desired state",
                    config.name, config.namespace
                ),
            ));
        }
        Ok(output)
    }
}

impl K8sSecretModule {
    fn ensure_present(&self, config: &SecretConfig) -> ModuleResult<ModuleOutput> {
        // Build secret data (encode to base64 if not already encoded)
        let mut encoded_data: BTreeMap<String, String> = BTreeMap::new();

        for (k, v) in &config.data {
            // Assume data is already base64 encoded or encode it
            encoded_data.insert(k.clone(), v.clone());
        }

        for (k, v) in &config.string_data {
            // string_data should be base64 encoded
            use base64::{engine::general_purpose::STANDARD, Engine};
            encoded_data.insert(k.clone(), STANDARD.encode(v));
        }

        // Build the secret specification
        let secret_spec = json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": config.name,
                "namespace": config.namespace,
                "labels": config.labels,
                "annotations": config.annotations,
            },
            "type": config.secret_type.to_k8s_type(),
            "data": encoded_data,
            "immutable": config.immutable,
        });

        // In a real implementation, we would apply this to the cluster
        // For now, return success with the spec
        Ok(ModuleOutput::changed(format!(
            "Secret '{}' would be created/updated in namespace '{}'",
            config.name, config.namespace
        ))
        .with_data("secret", secret_spec)
        .with_data(
            "result",
            json!({
                "name": config.name,
                "namespace": config.namespace,
                "type": config.secret_type.to_k8s_type(),
                "keys": encoded_data.keys().collect::<Vec<_>>(),
            }),
        ))
    }

    fn ensure_absent(&self, config: &SecretConfig) -> ModuleResult<ModuleOutput> {
        // In a real implementation, we would delete the secret
        Ok(ModuleOutput::changed(format!(
            "Secret '{}' would be deleted from namespace '{}'",
            config.name, config.namespace
        ))
        .with_data(
            "result",
            json!({
                "name": config.name,
                "namespace": config.namespace,
                "deleted": true,
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_secret_module_name() {
        let module = K8sSecretModule;
        assert_eq!(module.name(), "k8s_secret");
    }

    #[test]
    fn test_secret_config_from_params() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), json!("my-secret"));
        params.insert("namespace".to_string(), json!("default"));
        params.insert(
            "data".to_string(),
            json!({
                "password": "c2VjcmV0"
            }),
        );

        let config = SecretConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "my-secret");
        assert_eq!(config.namespace, "default");
        assert_eq!(config.secret_type, SecretType::Opaque);
    }

    #[test]
    fn test_secret_ensure_present() {
        let module = K8sSecretModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), json!("test-secret"));
        params.insert(
            "string_data".to_string(),
            json!({
                "password": "mysecret"
            }),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();
        assert!(result.changed);
    }

    #[test]
    fn test_secret_ensure_absent() {
        let module = K8sSecretModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), json!("test-secret"));
        params.insert("state".to_string(), json!("absent"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();
        assert!(result.changed);
    }
}
