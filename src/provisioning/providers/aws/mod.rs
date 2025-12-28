//! AWS Provider Implementation
//!
//! This module provides the AWS cloud provider implementation for Rustible's
//! provisioning system. It supports common AWS resources like VPCs, subnets,
//! security groups, and EC2 instances.
//!
//! ## Configuration
//!
//! The AWS provider can be configured with:
//! - `region`: AWS region (e.g., "us-east-1")
//! - `access_key`: AWS access key ID (optional, uses credential chain)
//! - `secret_key`: AWS secret access key (optional, uses credential chain)
//! - `profile`: AWS profile name (optional)
//! - `session_token`: Session token for temporary credentials (optional)
//!
//! ## Credential Resolution
//!
//! Credentials are resolved in this order:
//! 1. Explicit credentials in configuration
//! 2. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
//! 3. AWS profile (~/.aws/credentials)
//! 4. Instance metadata (EC2/ECS)
//!
//! ## Example
//!
//! ```rust,ignore
//! use rustible::provisioning::providers::aws::AwsProvider;
//! use rustible::provisioning::traits::{Provider, ProviderConfig};
//!
//! let mut provider = AwsProvider::new();
//!
//! let config = ProviderConfig {
//!     name: "aws".to_string(),
//!     region: Some("us-east-1".to_string()),
//!     settings: serde_json::json!({
//!         "profile": "production"
//!     }),
//! };
//!
//! provider.configure(config).await?;
//! let ctx = provider.context()?;
//! ```

mod credentials;

pub use credentials::{resolve_credentials, AwsCredentialChain, AwsCredentials, CredentialSource};

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    DataSource, FieldConstraint, FieldType, Provider, ProviderConfig, ProviderContext,
    ProviderSchema, Resource, RetryConfig, SchemaField,
};

// ============================================================================
// Constants
// ============================================================================

/// Provider name
const PROVIDER_NAME: &str = "aws";

/// Provider version
const PROVIDER_VERSION: &str = "1.0.0";

/// Default AWS region
const DEFAULT_REGION: &str = "us-east-1";

/// Default timeout in seconds
const DEFAULT_TIMEOUT: u64 = 300;

/// Supported AWS regions
const AWS_REGIONS: &[&str] = &[
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "af-south-1",
    "ap-east-1",
    "ap-south-1",
    "ap-south-2",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-3",
    "ap-southeast-4",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "ca-central-1",
    "ca-west-1",
    "eu-central-1",
    "eu-central-2",
    "eu-west-1",
    "eu-west-2",
    "eu-west-3",
    "eu-north-1",
    "eu-south-1",
    "eu-south-2",
    "il-central-1",
    "me-central-1",
    "me-south-1",
    "sa-east-1",
];

/// Supported resource types
const RESOURCE_TYPES: &[&str] = &[
    "aws_vpc",
    "aws_subnet",
    "aws_security_group",
    "aws_instance",
];

// ============================================================================
// AWS Provider
// ============================================================================

/// AWS cloud provider implementation
///
/// The AwsProvider implements the Provider trait to enable infrastructure
/// provisioning on AWS. It handles credential resolution, resource management,
/// and API interactions with AWS services.
///
/// # Example
///
/// ```rust,ignore
/// use rustible::provisioning::providers::aws::AwsProvider;
///
/// let provider = AwsProvider::new()
///     .with_region("us-west-2")
///     .with_default_tags(tags);
/// ```
pub struct AwsProvider {
    /// Provider name
    name: String,

    /// AWS region
    region: Option<String>,

    /// Resolved credentials
    credentials: Option<Arc<AwsCredentials>>,

    /// Provider configuration
    config: Value,

    /// Registered resources
    resources: HashMap<String, Arc<dyn Resource>>,

    /// Registered data sources
    data_sources: HashMap<String, Arc<dyn DataSource>>,

    /// Default tags for all resources
    default_tags: HashMap<String, String>,

    /// Request timeout in seconds
    timeout_seconds: u64,

    /// Maximum retries for API calls
    max_retries: u32,
}

impl Default for AwsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AwsProvider {
    /// Create a new AWS provider instance
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            region: None,
            credentials: None,
            config: Value::Null,
            resources: HashMap::new(),
            data_sources: HashMap::new(),
            default_tags: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
            max_retries: 3,
        }
    }

    /// Create a provider with a specific region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Create a provider with pre-resolved credentials
    pub fn with_credentials(mut self, credentials: AwsCredentials) -> Self {
        self.credentials = Some(Arc::new(credentials));
        self
    }

    /// Set default tags for all resources
    pub fn with_default_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.default_tags = tags;
        self
    }

    /// Set the request timeout
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Set the maximum retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Get the resolved region (defaults to us-east-1)
    pub fn region(&self) -> &str {
        self.region.as_deref().unwrap_or(DEFAULT_REGION)
    }

    /// Get the credentials (if resolved)
    pub fn credentials(&self) -> Option<&Arc<AwsCredentials>> {
        self.credentials.as_ref()
    }

    /// Check if provider is configured
    pub fn is_configured(&self) -> bool {
        self.credentials.is_some()
    }

    /// Register a resource implementation
    pub fn register_resource(&mut self, resource: Arc<dyn Resource>) {
        let resource_type = resource.resource_type().to_string();
        debug!("Registering AWS resource: {}", resource_type);
        self.resources.insert(resource_type, resource);
    }

    /// Register a data source implementation
    pub fn register_data_source(&mut self, data_source: Arc<dyn DataSource>) {
        let ds_type = data_source.data_source_type().to_string();
        debug!("Registering AWS data source: {}", ds_type);
        self.data_sources.insert(ds_type, data_source);
    }

    /// Extract region from configuration
    fn extract_region(&mut self, config: &ProviderConfig) {
        // Priority: config.region > config.settings.region > existing
        if let Some(ref region) = config.region {
            self.region = Some(region.clone());
        } else if let Some(region) = config.settings.get("region").and_then(|v| v.as_str()) {
            self.region = Some(region.to_string());
        }
    }

    /// Extract default tags from configuration
    fn extract_default_tags(&mut self, config: &Value) {
        if let Some(tags) = config.get("default_tags").and_then(|v| v.as_object()) {
            for (key, value) in tags {
                if let Some(v) = value.as_str() {
                    self.default_tags.insert(key.clone(), v.to_string());
                }
            }
        }
    }

    /// Extract timeout from configuration
    fn extract_timeout(&mut self, config: &Value) {
        if let Some(timeout) = config.get("timeout").and_then(|v| v.as_u64()) {
            self.timeout_seconds = timeout;
        }
    }

    /// Extract max_retries from configuration
    fn extract_max_retries(&mut self, config: &Value) {
        if let Some(retries) = config.get("max_retries").and_then(|v| v.as_u64()) {
            self.max_retries = retries as u32;
        }
    }
}

impl std::fmt::Debug for AwsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsProvider")
            .field("name", &self.name)
            .field("region", &self.region)
            .field("is_configured", &self.is_configured())
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
            .field(
                "data_sources",
                &self.data_sources.keys().collect::<Vec<_>>(),
            )
            .field("default_tags", &self.default_tags)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

// ============================================================================
// Provider Trait Implementation
// ============================================================================

#[async_trait]
impl Provider for AwsProvider {
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
                    name: "region".to_string(),
                    field_type: FieldType::String,
                    description: "AWS region (e.g., us-east-1)".to_string(),
                    default: Some(Value::String(DEFAULT_REGION.to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: AWS_REGIONS.iter().map(|s| s.to_string()).collect(),
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "access_key".to_string(),
                    field_type: FieldType::String,
                    description: "AWS access key ID".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 16 },
                        FieldConstraint::MaxLength { max: 128 },
                    ],
                    sensitive: true,
                },
                SchemaField {
                    name: "secret_key".to_string(),
                    field_type: FieldType::String,
                    description: "AWS secret access key".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "profile".to_string(),
                    field_type: FieldType::String,
                    description: "AWS profile name from ~/.aws/credentials".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "session_token".to_string(),
                    field_type: FieldType::String,
                    description: "AWS session token for temporary credentials".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "default_tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Default tags to apply to all resources".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "timeout".to_string(),
                    field_type: FieldType::Integer,
                    description: "Default timeout for API operations in seconds".to_string(),
                    default: Some(Value::Number(DEFAULT_TIMEOUT.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 30 },
                        FieldConstraint::MaxValue { value: 3600 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "max_retries".to_string(),
                    field_type: FieldType::Integer,
                    description: "Maximum number of retries for failed API calls".to_string(),
                    default: Some(Value::Number(3.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 0 },
                        FieldConstraint::MaxValue { value: 10 },
                    ],
                    sensitive: false,
                },
            ],
            regions: Some(AWS_REGIONS.iter().map(|s| s.to_string()).collect()),
        }
    }

    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()> {
        info!("Configuring AWS provider");

        // Extract region
        self.extract_region(&config);

        // Store configuration
        self.config = config.settings.clone();

        // Extract other settings
        self.extract_default_tags(&config.settings);
        self.extract_timeout(&config.settings);
        self.extract_max_retries(&config.settings);

        // Resolve credentials using the credential chain
        let creds = resolve_credentials(&config.settings).await?;
        self.credentials = Some(Arc::new(creds));

        info!(
            "AWS provider configured for region: {} (credentials: {:?})",
            self.region(),
            self.credentials.as_ref().map(|c| &c.source)
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
        self.data_sources
            .get(ds_type)
            .cloned()
            .ok_or_else(|| ProvisioningError::ResourceNotFound {
                provider: PROVIDER_NAME.to_string(),
                resource_type: format!("data.{}", ds_type),
            })
    }

    fn resource_types(&self) -> Vec<String> {
        RESOURCE_TYPES.iter().map(|s| s.to_string()).collect()
    }

    fn data_source_types(&self) -> Vec<String> {
        self.data_sources.keys().cloned().collect()
    }

    fn validate_config(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate region if provided
        if let Some(region) = config.get("region").and_then(|v| v.as_str()) {
            if !AWS_REGIONS.contains(&region) {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    format!(
                        "Invalid region '{}'. Valid regions: {}",
                        region,
                        AWS_REGIONS[..5].join(", ") + ", ..."
                    ),
                ));
            }
        }

        // Validate access_key format if provided
        if let Some(access_key) = config.get("access_key").and_then(|v| v.as_str()) {
            if access_key.len() < 16 || access_key.len() > 128 {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "access_key must be between 16 and 128 characters",
                ));
            }
        }

        // Validate that secret_key is provided with access_key
        let has_access = config.get("access_key").and_then(|v| v.as_str()).is_some();
        let has_secret = config.get("secret_key").and_then(|v| v.as_str()).is_some();

        if has_access != has_secret {
            return Err(ProvisioningError::provider_config(
                PROVIDER_NAME,
                "Both access_key and secret_key must be provided together",
            ));
        }

        // Validate timeout if provided
        if let Some(timeout) = config.get("timeout") {
            if let Some(t) = timeout.as_u64() {
                if t < 30 || t > 3600 {
                    return Err(ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "timeout must be between 30 and 3600 seconds",
                    ));
                }
            } else if !timeout.is_null() {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "timeout must be a positive integer",
                ));
            }
        }

        // Validate max_retries if provided
        if let Some(retries) = config.get("max_retries") {
            if let Some(r) = retries.as_i64() {
                if r < 0 || r > 10 {
                    return Err(ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "max_retries must be between 0 and 10",
                    ));
                }
            } else if !retries.is_null() {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "max_retries must be a positive integer",
                ));
            }
        }

        // Validate default_tags if provided
        if let Some(tags) = config.get("default_tags") {
            if !tags.is_null() && !tags.is_object() {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "default_tags must be an object with string values",
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
            credentials,
            timeout_seconds: self.timeout_seconds,
            retry_config: RetryConfig {
                max_retries: self.max_retries,
                initial_backoff_ms: 1000,
                max_backoff_ms: 30000,
                backoff_multiplier: 2.0,
            },
            default_tags: self.default_tags.clone(),
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
        let provider = AwsProvider::new();

        assert_eq!(provider.name(), "aws");
        assert_eq!(provider.version(), "1.0.0");
        assert!(!provider.is_configured());
    }

    #[test]
    fn test_provider_default() {
        let provider = AwsProvider::default();
        assert_eq!(provider.name(), "aws");
    }

    #[test]
    fn test_provider_with_region() {
        let provider = AwsProvider::new().with_region("eu-west-1");
        assert_eq!(provider.region(), "eu-west-1");
    }

    #[test]
    fn test_provider_default_region() {
        let provider = AwsProvider::new();
        assert_eq!(provider.region(), "us-east-1");
    }

    #[test]
    fn test_provider_with_credentials() {
        let creds = AwsCredentials::new("AKID", "SECRET", CredentialSource::Explicit);
        let provider = AwsProvider::new().with_credentials(creds);

        assert!(provider.credentials().is_some());
        assert!(provider.is_configured());
    }

    #[test]
    fn test_provider_with_default_tags() {
        let mut tags = HashMap::new();
        tags.insert("Environment".to_string(), "production".to_string());
        tags.insert("Team".to_string(), "platform".to_string());

        let provider = AwsProvider::new().with_default_tags(tags);

        assert_eq!(provider.default_tags.len(), 2);
        assert_eq!(
            provider.default_tags.get("Environment"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn test_provider_with_timeout() {
        let provider = AwsProvider::new().with_timeout(600);
        assert_eq!(provider.timeout_seconds, 600);
    }

    #[test]
    fn test_provider_with_max_retries() {
        let provider = AwsProvider::new().with_max_retries(5);
        assert_eq!(provider.max_retries, 5);
    }

    #[test]
    fn test_config_schema() {
        let provider = AwsProvider::new();
        let schema = provider.config_schema();

        assert_eq!(schema.name, "aws");
        assert_eq!(schema.version, "1.0.0");
        assert!(schema.required_fields.is_empty());
        assert!(!schema.optional_fields.is_empty());

        // Check that region field exists
        let region_field = schema.optional_fields.iter().find(|f| f.name == "region");
        assert!(region_field.is_some());

        // Check that regions list is present
        assert!(schema.regions.is_some());
        let regions = schema.regions.unwrap();
        assert!(regions.contains(&"us-east-1".to_string()));
        assert!(regions.contains(&"eu-west-1".to_string()));
    }

    #[test]
    fn test_resource_types() {
        let provider = AwsProvider::new();
        let types = provider.resource_types();

        assert!(types.contains(&"aws_vpc".to_string()));
        assert!(types.contains(&"aws_subnet".to_string()));
        assert!(types.contains(&"aws_security_group".to_string()));
        assert!(types.contains(&"aws_instance".to_string()));
    }

    #[test]
    fn test_data_source_types_empty() {
        let provider = AwsProvider::new();
        let types = provider.data_source_types();
        assert!(types.is_empty());
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "region": "us-east-1",
            "timeout": 300,
            "max_retries": 3
        });

        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_valid_with_credentials() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "region": "us-west-2",
            "access_key": "AKIAIOSFODNN7EXAMPLE",
            "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
        });

        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_invalid_region() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "region": "invalid-region"
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid region"));
    }

    #[test]
    fn test_validate_config_missing_secret() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "access_key": "AKIAIOSFODNN7EXAMPLE"
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Both access_key and secret_key"));
    }

    #[test]
    fn test_validate_config_missing_access_key() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_invalid_access_key_length() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "access_key": "short",
            "secret_key": "secret"
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("access_key must be"));
    }

    #[test]
    fn test_validate_config_invalid_timeout_too_low() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "timeout": 10
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timeout must be"));
    }

    #[test]
    fn test_validate_config_invalid_timeout_too_high() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "timeout": 5000
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_invalid_max_retries() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "max_retries": 15
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_retries must be"));
    }

    #[test]
    fn test_validate_config_invalid_default_tags() {
        let provider = AwsProvider::new();

        let config = serde_json::json!({
            "default_tags": "not an object"
        });

        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("default_tags must be"));
    }

    #[test]
    fn test_validate_config_empty() {
        let provider = AwsProvider::new();
        let config = serde_json::json!({});
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = AwsProvider::new();
        let result = provider.context();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[test]
    fn test_context_when_configured() {
        let creds = AwsCredentials::new("AKID", "SECRET", CredentialSource::Explicit);
        let mut tags = HashMap::new();
        tags.insert("Env".to_string(), "test".to_string());

        let provider = AwsProvider::new()
            .with_region("us-west-2")
            .with_credentials(creds)
            .with_default_tags(tags)
            .with_timeout(600)
            .with_max_retries(5);

        let ctx = provider.context().unwrap();

        assert_eq!(ctx.provider, "aws");
        assert_eq!(ctx.region, Some("us-west-2".to_string()));
        assert_eq!(ctx.timeout_seconds, 600);
        assert_eq!(ctx.retry_config.max_retries, 5);
        assert_eq!(ctx.default_tags.get("Env"), Some(&"test".to_string()));
    }

    #[test]
    fn test_resource_not_found() {
        let provider = AwsProvider::new();
        let result = provider.resource("aws_nonexistent");

        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisioningError::ResourceNotFound {
                provider,
                resource_type,
            } => {
                assert_eq!(provider, "aws");
                assert_eq!(resource_type, "aws_nonexistent");
            }
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_data_source_not_found() {
        let provider = AwsProvider::new();
        let result = provider.data_source("aws_ami");

        assert!(result.is_err());
    }

    #[test]
    fn test_provider_debug() {
        let provider = AwsProvider::new().with_region("us-west-2");
        let debug_str = format!("{:?}", provider);

        assert!(debug_str.contains("AwsProvider"));
        assert!(debug_str.contains("us-west-2"));
        assert!(debug_str.contains("is_configured"));
    }

    #[test]
    fn test_sensitive_fields_marked() {
        let provider = AwsProvider::new();
        let schema = provider.config_schema();

        let secret_field = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "secret_key")
            .unwrap();
        assert!(secret_field.sensitive);

        let access_field = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "access_key")
            .unwrap();
        assert!(access_field.sensitive);

        let session_token_field = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "session_token")
            .unwrap();
        assert!(session_token_field.sensitive);

        let region_field = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "region")
            .unwrap();
        assert!(!region_field.sensitive);
    }

    #[tokio::test]
    async fn test_configure_with_static_credentials() {
        let mut provider = AwsProvider::new();

        let config = ProviderConfig {
            name: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            settings: serde_json::json!({
                "access_key": "AKIAIOSFODNN7EXAMPLE",
                "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                "default_tags": {
                    "Environment": "test"
                },
                "timeout": 600,
                "max_retries": 5
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());

        assert!(provider.is_configured());
        assert_eq!(provider.region(), "us-east-1");
        assert_eq!(provider.timeout_seconds, 600);
        assert_eq!(provider.max_retries, 5);
        assert_eq!(
            provider.default_tags.get("Environment"),
            Some(&"test".to_string())
        );

        // Test context creation
        let context = provider.context().unwrap();
        assert_eq!(context.provider, "aws");
        assert_eq!(context.region, Some("us-east-1".to_string()));
        assert_eq!(context.timeout_seconds, 600);
        assert_eq!(context.retry_config.max_retries, 5);
    }

    #[tokio::test]
    async fn test_configure_region_priority() {
        let mut provider = AwsProvider::new();

        // config.region takes priority over settings.region
        let config = ProviderConfig {
            name: "aws".to_string(),
            region: Some("eu-west-1".to_string()),
            settings: serde_json::json!({
                "region": "us-west-2",
                "access_key": "AKIAIOSFODNN7EXAMPLE",
                "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
            }),
        };

        let _ = provider.configure(config).await;
        assert_eq!(provider.region(), "eu-west-1");
    }

    #[tokio::test]
    async fn test_configure_region_from_settings() {
        let mut provider = AwsProvider::new();

        let config = ProviderConfig {
            name: "aws".to_string(),
            region: None,
            settings: serde_json::json!({
                "region": "ap-northeast-1",
                "access_key": "AKIAIOSFODNN7EXAMPLE",
                "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
            }),
        };

        let _ = provider.configure(config).await;
        assert_eq!(provider.region(), "ap-northeast-1");
    }

    #[test]
    fn test_extract_default_tags() {
        let mut provider = AwsProvider::new();
        let config = serde_json::json!({
            "default_tags": {
                "Project": "rustible",
                "Owner": "team-infra",
                "Cost-Center": "1234"
            }
        });

        provider.extract_default_tags(&config);

        assert_eq!(provider.default_tags.len(), 3);
        assert_eq!(
            provider.default_tags.get("Project"),
            Some(&"rustible".to_string())
        );
        assert_eq!(
            provider.default_tags.get("Owner"),
            Some(&"team-infra".to_string())
        );
    }

    #[test]
    fn test_extract_timeout() {
        let mut provider = AwsProvider::new();
        let config = serde_json::json!({
            "timeout": 900
        });

        provider.extract_timeout(&config);
        assert_eq!(provider.timeout_seconds, 900);
    }

    #[test]
    fn test_extract_max_retries() {
        let mut provider = AwsProvider::new();
        let config = serde_json::json!({
            "max_retries": 7
        });

        provider.extract_max_retries(&config);
        assert_eq!(provider.max_retries, 7);
    }

    #[test]
    fn test_retry_config_defaults() {
        let creds = AwsCredentials::new("AKID", "SECRET", CredentialSource::Explicit);
        let provider = AwsProvider::new().with_credentials(creds);

        let ctx = provider.context().unwrap();

        assert_eq!(ctx.retry_config.max_retries, 3);
        assert_eq!(ctx.retry_config.initial_backoff_ms, 1000);
        assert_eq!(ctx.retry_config.max_backoff_ms, 30000);
        assert!((ctx.retry_config.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }
}
