//! GCP Provider Implementation (Experimental)
//!
//! This module provides the Google Cloud Platform provider stub for Rustible's
//! provisioning system. It defines the provider types and trait implementations
//! without making real GCP API calls.
//!
//! ## Feature Gate
//!
//! This module requires both `gcp` and `experimental` features:
//! ```toml
//! [features]
//! gcp = []
//! experimental = []
//! ```
//!
//! ## Resource Types
//!
//! - `google_compute_instance` - GCE Virtual Machine Instances
//! - `google_compute_network` - VPC Networks
//! - `google_compute_subnetwork` - VPC Subnetworks
//! - `google_compute_firewall` - VPC Firewall Rules

pub mod credentials;

pub use credentials::{GcpAuthMethod, GcpCredentials};

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::info;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    DataSource, FieldType, Provider, ProviderConfig, ProviderContext, ProviderSchema, Resource,
    RetryConfig, SchemaField,
};

// ============================================================================
// Constants
// ============================================================================

/// Provider name
const PROVIDER_NAME: &str = "gcp";

/// Provider version
const PROVIDER_VERSION: &str = "0.1.0-experimental";

/// Default timeout in seconds
const DEFAULT_TIMEOUT: u64 = 300;

/// Supported resource types
const RESOURCE_TYPES: &[&str] = &[
    "google_compute_instance",
    "google_compute_network",
    "google_compute_subnetwork",
    "google_compute_firewall",
];

/// Supported GCP regions
const GCP_REGIONS: &[&str] = &[
    "us-central1",
    "us-east1",
    "us-east4",
    "us-east5",
    "us-south1",
    "us-west1",
    "us-west2",
    "us-west3",
    "us-west4",
    "northamerica-northeast1",
    "northamerica-northeast2",
    "southamerica-east1",
    "southamerica-west1",
    "europe-central2",
    "europe-north1",
    "europe-southwest1",
    "europe-west1",
    "europe-west2",
    "europe-west3",
    "europe-west4",
    "europe-west6",
    "europe-west8",
    "europe-west9",
    "asia-east1",
    "asia-east2",
    "asia-northeast1",
    "asia-northeast2",
    "asia-northeast3",
    "asia-south1",
    "asia-south2",
    "asia-southeast1",
    "asia-southeast2",
    "australia-southeast1",
    "australia-southeast2",
    "me-central1",
    "me-west1",
];

// ============================================================================
// GCP Provider
// ============================================================================

/// Google Cloud Platform provider implementation (experimental stub)
///
/// The GcpProvider implements the Provider trait for Google Cloud.
/// This is currently a stub that defines the interface without making real
/// API calls.
pub struct GcpProvider {
    /// Provider name
    name: String,

    /// GCP credentials
    credentials: Option<GcpCredentials>,

    /// GCP project ID
    project: Option<String>,

    /// Default GCP region
    region: Option<String>,

    /// Provider configuration
    config: Value,

    /// Registered resources
    resources: HashMap<String, Arc<dyn Resource>>,

    /// Default labels for all resources
    default_labels: HashMap<String, String>,

    /// Request timeout in seconds
    timeout_seconds: u64,

    /// Maximum retries for API calls
    max_retries: u32,
}

impl Default for GcpProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GcpProvider {
    /// Create a new GCP provider instance
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            credentials: None,
            project: None,
            region: None,
            config: Value::Null,
            resources: HashMap::new(),
            default_labels: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
            max_retries: 3,
        }
    }

    /// Set the GCP region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the GCP project
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Set credentials
    pub fn with_credentials(mut self, credentials: GcpCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Set default labels for all resources
    pub fn with_default_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.default_labels = labels;
        self
    }

    /// Set the request timeout
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Get the resolved region
    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// Get the project ID
    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    /// Check if provider is configured
    pub fn is_configured(&self) -> bool {
        self.credentials.is_some()
    }
}

impl std::fmt::Debug for GcpProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpProvider")
            .field("name", &self.name)
            .field("project", &self.project)
            .field("region", &self.region)
            .field("is_configured", &self.is_configured())
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
            .field("default_labels", &self.default_labels)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

// ============================================================================
// Provider Trait Implementation
// ============================================================================

#[async_trait]
impl Provider for GcpProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        PROVIDER_VERSION
    }

    fn config_schema(&self) -> ProviderSchema {
        ProviderSchema {
            name: PROVIDER_NAME.to_string(),
            version: PROVIDER_VERSION.to_string(),
            required_fields: vec![],
            optional_fields: vec![
                SchemaField {
                    name: "project".to_string(),
                    field_type: FieldType::String,
                    description: "GCP project ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "region".to_string(),
                    field_type: FieldType::String,
                    description: "Default GCP region (e.g., us-central1)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "zone".to_string(),
                    field_type: FieldType::String,
                    description: "Default GCP zone (e.g., us-central1-a)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "credentials".to_string(),
                    field_type: FieldType::String,
                    description: "Path to service account JSON key file".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "default_labels".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Default labels to apply to all resources".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            regions: Some(GCP_REGIONS.iter().map(|s| s.to_string()).collect()),
        }
    }

    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()> {
        info!("Configuring GCP provider (experimental stub)");

        // Extract region
        if let Some(ref region) = config.region {
            self.region = Some(region.clone());
        } else if let Some(region) = config.settings.get("region").and_then(|v| v.as_str()) {
            self.region = Some(region.to_string());
        }

        // Extract project
        if let Some(project) = config.settings.get("project").and_then(|v| v.as_str()) {
            self.project = Some(project.to_string());
        }

        // Extract default labels
        if let Some(labels) = config
            .settings
            .get("default_labels")
            .and_then(|v| v.as_object())
        {
            for (key, value) in labels {
                if let Some(v) = value.as_str() {
                    self.default_labels.insert(key.clone(), v.to_string());
                }
            }
        }

        // Build credentials from config
        let project_id = self.project.clone();
        let service_account_key = config
            .settings
            .get("credentials")
            .and_then(|v| v.as_str())
            .map(String::from);

        self.credentials = Some(GcpCredentials::new(project_id, service_account_key));
        self.config = config.settings.clone();

        info!(
            "GCP provider configured for project: {:?} region: {:?}",
            self.project, self.region
        );

        Ok(())
    }

    fn resource(&self, resource_type: &str) -> ProvisioningResult<Arc<dyn Resource>> {
        self.resources
            .get(resource_type)
            .cloned()
            .ok_or_else(|| ProvisioningError::resource_not_found(PROVIDER_NAME, resource_type))
    }

    fn data_source(&self, ds_type: &str) -> ProvisioningResult<Arc<dyn DataSource>> {
        Err(ProvisioningError::resource_not_found(
            PROVIDER_NAME,
            format!("data.{}", ds_type),
        ))
    }

    fn resource_types(&self) -> Vec<String> {
        RESOURCE_TYPES.iter().map(|s| s.to_string()).collect()
    }

    fn data_source_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_config(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate region if provided
        if let Some(region) = config.get("region").and_then(|v| v.as_str()) {
            if !GCP_REGIONS.contains(&region) {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    format!(
                        "Invalid region '{}'. Valid regions: {}",
                        region,
                        GCP_REGIONS[..5].join(", ") + ", ..."
                    ),
                ));
            }
        }

        // Validate default_labels is an object if provided
        if let Some(labels) = config.get("default_labels") {
            if !labels.is_null() && !labels.is_object() {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "default_labels must be an object with string values",
                ));
            }
        }

        Ok(())
    }

    fn context(&self) -> ProvisioningResult<ProviderContext> {
        let credentials = self.credentials.clone().ok_or_else(|| {
            ProvisioningError::provider_config(
                PROVIDER_NAME,
                "Provider not configured. Call configure() first.",
            )
        })?;

        Ok(ProviderContext {
            provider: PROVIDER_NAME.to_string(),
            region: self.region.clone(),
            config: self.config.clone(),
            credentials: Arc::new(credentials),
            timeout_seconds: self.timeout_seconds,
            retry_config: RetryConfig {
                max_retries: self.max_retries,
                initial_backoff_ms: 1000,
                max_backoff_ms: 30000,
                backoff_multiplier: 2.0,
            },
            default_tags: self.default_labels.clone(),
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_metadata() {
        let provider = GcpProvider::new();

        assert_eq!(provider.name(), "gcp");
        assert!(provider.version().contains("experimental"));
        assert!(!provider.is_configured());
    }

    #[test]
    fn test_provider_default() {
        let provider = GcpProvider::default();
        assert_eq!(provider.name(), "gcp");
    }

    #[test]
    fn test_provider_with_region() {
        let provider = GcpProvider::new().with_region("us-central1");
        assert_eq!(provider.region(), Some("us-central1"));
    }

    #[test]
    fn test_provider_with_project() {
        let provider = GcpProvider::new().with_project("my-project-123");
        assert_eq!(provider.project(), Some("my-project-123"));
    }

    #[test]
    fn test_provider_with_credentials() {
        let creds =
            GcpCredentials::new(Some("my-project".to_string()), Some("key-data".to_string()));
        let provider = GcpProvider::new().with_credentials(creds);
        assert!(provider.is_configured());
    }

    #[test]
    fn test_provider_with_default_labels() {
        let mut labels = HashMap::new();
        labels.insert("environment".to_string(), "production".to_string());

        let provider = GcpProvider::new().with_default_labels(labels);
        assert_eq!(provider.default_labels.len(), 1);
        assert_eq!(
            provider.default_labels.get("environment"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn test_config_schema() {
        let provider = GcpProvider::new();
        let schema = provider.config_schema();

        assert_eq!(schema.name, "gcp");
        assert!(schema.required_fields.is_empty());
        assert!(!schema.optional_fields.is_empty());
        assert!(schema.regions.is_some());

        let regions = schema.regions.unwrap();
        assert!(regions.contains(&"us-central1".to_string()));
        assert!(regions.contains(&"europe-west1".to_string()));

        // Check sensitive fields
        let cred_field = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "credentials")
            .unwrap();
        assert!(cred_field.sensitive);
    }

    #[test]
    fn test_resource_types() {
        let provider = GcpProvider::new();
        let types = provider.resource_types();

        assert!(types.contains(&"google_compute_instance".to_string()));
        assert!(types.contains(&"google_compute_network".to_string()));
        assert!(types.contains(&"google_compute_subnetwork".to_string()));
        assert!(types.contains(&"google_compute_firewall".to_string()));
    }

    #[test]
    fn test_data_source_types_empty() {
        let provider = GcpProvider::new();
        assert!(provider.data_source_types().is_empty());
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = GcpProvider::new();
        let config = serde_json::json!({
            "project": "my-project",
            "region": "us-central1"
        });
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_invalid_region() {
        let provider = GcpProvider::new();
        let config = serde_json::json!({
            "region": "invalid-region"
        });
        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid region"));
    }

    #[test]
    fn test_validate_config_invalid_labels() {
        let provider = GcpProvider::new();
        let config = serde_json::json!({
            "default_labels": "not an object"
        });
        let result = provider.validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_empty() {
        let provider = GcpProvider::new();
        let config = serde_json::json!({});
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = GcpProvider::new();
        assert!(provider.context().is_err());
    }

    #[test]
    fn test_context_when_configured() {
        let creds = GcpCredentials::new(Some("my-project".to_string()), None);
        let provider = GcpProvider::new()
            .with_region("europe-west1")
            .with_project("my-project")
            .with_credentials(creds);

        let ctx = provider.context().unwrap();
        assert_eq!(ctx.provider, "gcp");
        assert_eq!(ctx.region, Some("europe-west1".to_string()));
        assert_eq!(ctx.timeout_seconds, 300);
    }

    #[test]
    fn test_resource_not_found() {
        let provider = GcpProvider::new();
        let result = provider.resource("google_nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_data_source_not_found() {
        let provider = GcpProvider::new();
        let result = provider.data_source("some_ds");
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_debug() {
        let provider = GcpProvider::new()
            .with_region("us-west1")
            .with_project("my-project");
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("GcpProvider"));
        assert!(debug_str.contains("us-west1"));
        assert!(debug_str.contains("my-project"));
    }

    #[tokio::test]
    async fn test_configure() {
        let mut provider = GcpProvider::new();
        let config = ProviderConfig {
            name: "gcp".to_string(),
            region: Some("us-east1".to_string()),
            settings: serde_json::json!({
                "project": "my-project-123",
                "credentials": "/path/to/key.json",
                "default_labels": {
                    "environment": "test"
                }
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());
        assert!(provider.is_configured());
        assert_eq!(provider.region(), Some("us-east1"));
        assert_eq!(provider.project(), Some("my-project-123"));
        assert_eq!(
            provider.default_labels.get("environment"),
            Some(&"test".to_string())
        );
    }

    #[tokio::test]
    async fn test_configure_region_from_settings() {
        let mut provider = GcpProvider::new();
        let config = ProviderConfig {
            name: "gcp".to_string(),
            region: None,
            settings: serde_json::json!({
                "region": "asia-east1"
            }),
        };

        let _ = provider.configure(config).await;
        assert_eq!(provider.region(), Some("asia-east1"));
    }
}
