//! Azure Provider Implementation (Experimental)
//!
//! This module provides the Azure cloud provider stub for Rustible's
//! provisioning system. It defines the provider types and trait implementations
//! without making real Azure API calls.
//!
//! ## Feature Gate
//!
//! This module requires both `azure` and `experimental` features:
//! ```toml
//! [features]
//! azure = []
//! experimental = []
//! ```
//!
//! ## Resource Types
//!
//! - `azurerm_resource_group` - Azure Resource Groups
//! - `azurerm_virtual_network` - Azure Virtual Networks
//! - `azurerm_subnet` - Azure Subnets
//! - `azurerm_network_interface` - Azure Network Interfaces
//! - `azurerm_linux_virtual_machine` - Azure Linux Virtual Machines

pub mod credentials;

pub use credentials::{AzureAuthMethod, AzureCredentials};

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
const PROVIDER_NAME: &str = "azure";

/// Provider version
const PROVIDER_VERSION: &str = "0.1.0-experimental";

/// Default timeout in seconds
const DEFAULT_TIMEOUT: u64 = 300;

/// Supported resource types
const RESOURCE_TYPES: &[&str] = &[
    "azurerm_resource_group",
    "azurerm_virtual_network",
    "azurerm_subnet",
    "azurerm_network_interface",
    "azurerm_linux_virtual_machine",
];

/// Supported Azure regions
const AZURE_REGIONS: &[&str] = &[
    "eastus",
    "eastus2",
    "westus",
    "westus2",
    "westus3",
    "centralus",
    "northcentralus",
    "southcentralus",
    "westcentralus",
    "canadacentral",
    "canadaeast",
    "brazilsouth",
    "northeurope",
    "westeurope",
    "uksouth",
    "ukwest",
    "francecentral",
    "germanywestcentral",
    "switzerlandnorth",
    "norwayeast",
    "eastasia",
    "southeastasia",
    "japaneast",
    "japanwest",
    "australiaeast",
    "australiasoutheast",
    "centralindia",
    "southindia",
    "koreacentral",
    "koreasouth",
];

// ============================================================================
// Azure Provider
// ============================================================================

/// Azure cloud provider implementation (experimental stub)
///
/// The AzureProvider implements the Provider trait for Azure Resource Manager.
/// This is currently a stub that defines the interface without making real
/// API calls.
pub struct AzureProvider {
    /// Provider name
    name: String,

    /// Azure credentials
    credentials: Option<AzureCredentials>,

    /// Azure subscription ID
    subscription_id: Option<String>,

    /// Default resource group
    resource_group: Option<String>,

    /// Azure region / location
    region: Option<String>,

    /// Provider configuration
    config: Value,

    /// Registered resources
    resources: HashMap<String, Arc<dyn Resource>>,

    /// Default tags for all resources
    default_tags: HashMap<String, String>,

    /// Request timeout in seconds
    timeout_seconds: u64,

    /// Maximum retries for API calls
    max_retries: u32,
}

impl Default for AzureProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AzureProvider {
    /// Create a new Azure provider instance
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            credentials: None,
            subscription_id: None,
            resource_group: None,
            region: None,
            config: Value::Null,
            resources: HashMap::new(),
            default_tags: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
            max_retries: 3,
        }
    }

    /// Set the Azure region / location
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the subscription ID
    pub fn with_subscription_id(mut self, subscription_id: impl Into<String>) -> Self {
        self.subscription_id = Some(subscription_id.into());
        self
    }

    /// Set the default resource group
    pub fn with_resource_group(mut self, resource_group: impl Into<String>) -> Self {
        self.resource_group = Some(resource_group.into());
        self
    }

    /// Set credentials
    pub fn with_credentials(mut self, credentials: AzureCredentials) -> Self {
        self.credentials = Some(credentials);
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

    /// Get the resolved region
    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// Get the subscription ID
    pub fn subscription_id(&self) -> Option<&str> {
        self.subscription_id.as_deref()
    }

    /// Get the resource group
    pub fn resource_group(&self) -> Option<&str> {
        self.resource_group.as_deref()
    }

    /// Check if provider is configured
    pub fn is_configured(&self) -> bool {
        self.credentials.is_some()
    }
}

impl std::fmt::Debug for AzureProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureProvider")
            .field("name", &self.name)
            .field("region", &self.region)
            .field("subscription_id", &self.subscription_id)
            .field("resource_group", &self.resource_group)
            .field("is_configured", &self.is_configured())
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
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
impl Provider for AzureProvider {
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
                    name: "subscription_id".to_string(),
                    field_type: FieldType::String,
                    description: "Azure subscription ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tenant_id".to_string(),
                    field_type: FieldType::String,
                    description: "Azure Active Directory tenant ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "client_id".to_string(),
                    field_type: FieldType::String,
                    description: "Azure service principal client ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "client_secret".to_string(),
                    field_type: FieldType::String,
                    description: "Azure service principal client secret".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "resource_group".to_string(),
                    field_type: FieldType::String,
                    description: "Default resource group name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "location".to_string(),
                    field_type: FieldType::String,
                    description: "Azure region / location (e.g., eastus)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Default tags to apply to all resources".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            regions: Some(AZURE_REGIONS.iter().map(|s| s.to_string()).collect()),
        }
    }

    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()> {
        info!("Configuring Azure provider (experimental stub)");

        // Extract region / location
        if let Some(ref region) = config.region {
            self.region = Some(region.clone());
        } else if let Some(location) = config.settings.get("location").and_then(|v| v.as_str()) {
            self.region = Some(location.to_string());
        }

        // Extract subscription ID
        if let Some(sub) = config
            .settings
            .get("subscription_id")
            .and_then(|v| v.as_str())
        {
            self.subscription_id = Some(sub.to_string());
        }

        // Extract resource group
        if let Some(rg) = config
            .settings
            .get("resource_group")
            .and_then(|v| v.as_str())
        {
            self.resource_group = Some(rg.to_string());
        }

        // Extract default tags
        if let Some(tags) = config.settings.get("default_tags").and_then(|v| v.as_object()) {
            for (key, value) in tags {
                if let Some(v) = value.as_str() {
                    self.default_tags.insert(key.clone(), v.to_string());
                }
            }
        }

        // Build credentials from config
        let tenant_id = config
            .settings
            .get("tenant_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let client_id = config
            .settings
            .get("client_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let client_secret = config
            .settings
            .get("client_secret")
            .and_then(|v| v.as_str())
            .map(String::from);

        self.credentials = Some(AzureCredentials::new(tenant_id, client_id, client_secret));
        self.config = config.settings.clone();

        info!(
            "Azure provider configured for region: {:?} (subscription: {:?})",
            self.region, self.subscription_id
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
        if let Some(location) = config.get("location").and_then(|v| v.as_str()) {
            if !AZURE_REGIONS.contains(&location) {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    format!(
                        "Invalid location '{}'. Valid locations: {}",
                        location,
                        AZURE_REGIONS[..5].join(", ") + ", ..."
                    ),
                ));
            }
        }

        // Validate default_tags is an object if provided
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
            credentials: Arc::new(credentials),
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
        let provider = AzureProvider::new();

        assert_eq!(provider.name(), "azure");
        assert!(provider.version().contains("experimental"));
        assert!(!provider.is_configured());
    }

    #[test]
    fn test_provider_default() {
        let provider = AzureProvider::default();
        assert_eq!(provider.name(), "azure");
    }

    #[test]
    fn test_provider_with_region() {
        let provider = AzureProvider::new().with_region("eastus");
        assert_eq!(provider.region(), Some("eastus"));
    }

    #[test]
    fn test_provider_with_subscription_id() {
        let provider = AzureProvider::new().with_subscription_id("sub-123");
        assert_eq!(provider.subscription_id(), Some("sub-123"));
    }

    #[test]
    fn test_provider_with_resource_group() {
        let provider = AzureProvider::new().with_resource_group("my-rg");
        assert_eq!(provider.resource_group(), Some("my-rg"));
    }

    #[test]
    fn test_provider_with_credentials() {
        let creds = AzureCredentials::new(
            Some("tenant".to_string()),
            Some("client".to_string()),
            Some("secret".to_string()),
        );
        let provider = AzureProvider::new().with_credentials(creds);
        assert!(provider.is_configured());
    }

    #[test]
    fn test_provider_with_default_tags() {
        let mut tags = HashMap::new();
        tags.insert("Environment".to_string(), "production".to_string());

        let provider = AzureProvider::new().with_default_tags(tags);
        assert_eq!(provider.default_tags.len(), 1);
        assert_eq!(
            provider.default_tags.get("Environment"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn test_config_schema() {
        let provider = AzureProvider::new();
        let schema = provider.config_schema();

        assert_eq!(schema.name, "azure");
        assert!(schema.required_fields.is_empty());
        assert!(!schema.optional_fields.is_empty());
        assert!(schema.regions.is_some());

        let regions = schema.regions.unwrap();
        assert!(regions.contains(&"eastus".to_string()));
        assert!(regions.contains(&"westeurope".to_string()));

        // Check sensitive fields
        let client_secret = schema
            .optional_fields
            .iter()
            .find(|f| f.name == "client_secret")
            .unwrap();
        assert!(client_secret.sensitive);
    }

    #[test]
    fn test_resource_types() {
        let provider = AzureProvider::new();
        let types = provider.resource_types();

        assert!(types.contains(&"azurerm_resource_group".to_string()));
        assert!(types.contains(&"azurerm_virtual_network".to_string()));
        assert!(types.contains(&"azurerm_subnet".to_string()));
        assert!(types.contains(&"azurerm_network_interface".to_string()));
        assert!(types.contains(&"azurerm_linux_virtual_machine".to_string()));
    }

    #[test]
    fn test_data_source_types_empty() {
        let provider = AzureProvider::new();
        assert!(provider.data_source_types().is_empty());
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = AzureProvider::new();
        let config = serde_json::json!({
            "location": "eastus",
            "subscription_id": "sub-123"
        });
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_invalid_location() {
        let provider = AzureProvider::new();
        let config = serde_json::json!({
            "location": "invalid-region"
        });
        let result = provider.validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid location"));
    }

    #[test]
    fn test_validate_config_invalid_tags() {
        let provider = AzureProvider::new();
        let config = serde_json::json!({
            "default_tags": "not an object"
        });
        let result = provider.validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_empty() {
        let provider = AzureProvider::new();
        let config = serde_json::json!({});
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = AzureProvider::new();
        assert!(provider.context().is_err());
    }

    #[test]
    fn test_context_when_configured() {
        let creds = AzureCredentials::new(
            Some("tenant-123".to_string()),
            Some("client-456".to_string()),
            None,
        );
        let provider = AzureProvider::new()
            .with_region("westeurope")
            .with_credentials(creds)
            .with_subscription_id("sub-789");

        let ctx = provider.context().unwrap();
        assert_eq!(ctx.provider, "azure");
        assert_eq!(ctx.region, Some("westeurope".to_string()));
        assert_eq!(ctx.timeout_seconds, 300);
    }

    #[test]
    fn test_resource_not_found() {
        let provider = AzureProvider::new();
        let result = provider.resource("azurerm_nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_data_source_not_found() {
        let provider = AzureProvider::new();
        let result = provider.data_source("some_ds");
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_debug() {
        let provider = AzureProvider::new().with_region("eastus2");
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("AzureProvider"));
        assert!(debug_str.contains("eastus2"));
    }

    #[tokio::test]
    async fn test_configure() {
        let mut provider = AzureProvider::new();
        let config = ProviderConfig {
            name: "azure".to_string(),
            region: Some("westus2".to_string()),
            settings: serde_json::json!({
                "subscription_id": "sub-123",
                "tenant_id": "tenant-456",
                "client_id": "client-789",
                "client_secret": "secret-abc",
                "resource_group": "my-rg",
                "default_tags": {
                    "Environment": "test"
                }
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());
        assert!(provider.is_configured());
        assert_eq!(provider.region(), Some("westus2"));
        assert_eq!(provider.subscription_id(), Some("sub-123"));
        assert_eq!(provider.resource_group(), Some("my-rg"));
        assert_eq!(
            provider.default_tags.get("Environment"),
            Some(&"test".to_string())
        );
    }

    #[tokio::test]
    async fn test_configure_location_from_settings() {
        let mut provider = AzureProvider::new();
        let config = ProviderConfig {
            name: "azure".to_string(),
            region: None,
            settings: serde_json::json!({
                "location": "northeurope"
            }),
        };

        let _ = provider.configure(config).await;
        assert_eq!(provider.region(), Some("northeurope"));
    }
}
