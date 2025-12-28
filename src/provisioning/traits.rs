//! Core traits for infrastructure provisioning
//!
//! This module defines the fundamental traits that all providers and resources
//! must implement. These traits are distinct from the Module trait used for
//! SSH-based configuration management - Resource operates via cloud APIs.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::ProvisioningResult;

// ============================================================================
// Provider Traits
// ============================================================================

/// Context passed to resources during operations
#[derive(Debug, Clone)]
pub struct ProviderContext {
    /// Provider name (e.g., "aws", "azure", "gcp")
    pub provider: String,

    /// Region or location
    pub region: Option<String>,

    /// Provider-specific configuration
    pub config: Value,

    /// Credentials (opaque to resources)
    pub credentials: Arc<dyn ProviderCredentials>,

    /// Request timeout in seconds
    pub timeout_seconds: u64,

    /// Retry configuration
    pub retry_config: RetryConfig,

    /// Tags to apply to all resources
    pub default_tags: HashMap<String, String>,
}

/// Provider credentials trait
pub trait ProviderCredentials: Send + Sync + Debug {
    /// Get credential type name
    fn credential_type(&self) -> &str;

    /// Check if credentials are expired
    fn is_expired(&self) -> bool;

    /// Get credentials as a generic value (for serialization)
    fn as_value(&self) -> Value;
}

/// Retry configuration for cloud API calls
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,

    /// Initial backoff in milliseconds
    pub initial_backoff_ms: u64,

    /// Maximum backoff in milliseconds
    pub max_backoff_ms: u64,

    /// Backoff multiplier
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 1000,
            max_backoff_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Provider configuration schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSchema {
    /// Provider name
    pub name: String,

    /// Provider version
    pub version: String,

    /// Required configuration fields
    pub required_fields: Vec<SchemaField>,

    /// Optional configuration fields
    pub optional_fields: Vec<SchemaField>,

    /// Supported regions (if applicable)
    pub regions: Option<Vec<String>>,
}

/// Configuration for initializing a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name
    pub name: String,

    /// Region or location
    pub region: Option<String>,

    /// Provider-specific settings
    #[serde(flatten)]
    pub settings: Value,
}

/// A cloud provider implementation
#[async_trait]
pub trait Provider: Send + Sync + Debug {
    /// Get provider name (e.g., "aws", "azure", "gcp")
    fn name(&self) -> &str;

    /// Get provider version
    fn version(&self) -> &str;

    /// Get configuration schema
    fn config_schema(&self) -> ProviderSchema;

    /// Configure the provider with credentials and settings
    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()>;

    /// Get a resource implementation by type
    fn resource(&self, resource_type: &str) -> ProvisioningResult<Arc<dyn Resource>>;

    /// Get a data source implementation by type
    fn data_source(&self, ds_type: &str) -> ProvisioningResult<Arc<dyn DataSource>>;

    /// List all supported resource types
    fn resource_types(&self) -> Vec<String>;

    /// List all supported data source types
    fn data_source_types(&self) -> Vec<String>;

    /// Validate provider configuration
    fn validate_config(&self, config: &Value) -> ProvisioningResult<()>;

    /// Get the provider context for resource operations
    fn context(&self) -> ProvisioningResult<ProviderContext>;
}

// ============================================================================
// Resource Traits
// ============================================================================

/// Schema for a resource type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSchema {
    /// Resource type name
    pub resource_type: String,

    /// Human-readable description
    pub description: String,

    /// Required arguments
    pub required_args: Vec<SchemaField>,

    /// Optional arguments
    pub optional_args: Vec<SchemaField>,

    /// Computed attributes (read-only, set by cloud)
    pub computed_attrs: Vec<SchemaField>,

    /// Fields that force resource replacement when changed
    pub force_new: Vec<String>,

    /// Timeout settings
    pub timeouts: ResourceTimeouts,
}

/// A field in a schema
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaField {
    /// Field name
    pub name: String,

    /// Field type
    pub field_type: FieldType,

    /// Human-readable description
    pub description: String,

    /// Default value (if any)
    pub default: Option<Value>,

    /// Validation constraints
    pub constraints: Vec<FieldConstraint>,

    /// Whether this field is sensitive (should be hidden in logs)
    pub sensitive: bool,
}

/// Type of a schema field
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    List(Box<FieldType>),
    Map(Box<FieldType>),
    Object(Vec<SchemaField>),
    Any,
}

/// Constraint on a field value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldConstraint {
    MinLength {
        min: usize,
    },
    MaxLength {
        max: usize,
    },
    Pattern {
        regex: String,
    },
    Enum {
        values: Vec<String>,
    },
    #[serde(rename = "min")]
    MinValue {
        value: i64,
    },
    #[serde(rename = "max")]
    MaxValue {
        value: i64,
    },
    CidrBlock,
    Arn,
    Custom {
        name: String,
        message: String,
    },
}

/// Timeout settings for resource operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTimeouts {
    /// Create timeout in seconds
    pub create: u64,
    /// Read timeout in seconds
    pub read: u64,
    /// Update timeout in seconds
    pub update: u64,
    /// Delete timeout in seconds
    pub delete: u64,
}

impl Default for ResourceTimeouts {
    fn default() -> Self {
        Self {
            create: 300,
            read: 60,
            update: 300,
            delete: 300,
        }
    }
}

/// Result of reading a resource from the cloud
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReadResult {
    /// Whether the resource exists
    pub exists: bool,

    /// Cloud resource ID
    pub cloud_id: Option<String>,

    /// Current state/attributes from the cloud
    pub attributes: Value,

    /// Any metadata
    pub metadata: HashMap<String, String>,
}

impl ResourceReadResult {
    /// Create a result for a non-existent resource
    pub fn not_found() -> Self {
        Self {
            exists: false,
            cloud_id: None,
            attributes: Value::Null,
            metadata: HashMap::new(),
        }
    }

    /// Create a result for an existing resource
    pub fn found(cloud_id: impl Into<String>, attributes: Value) -> Self {
        Self {
            exists: true,
            cloud_id: Some(cloud_id.into()),
            attributes,
            metadata: HashMap::new(),
        }
    }
}

/// Difference between desired and current state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDiff {
    /// Type of change required
    pub change_type: ChangeType,

    /// Fields that will be added
    pub additions: HashMap<String, Value>,

    /// Fields that will be modified (old_value, new_value)
    pub modifications: HashMap<String, (Value, Value)>,

    /// Fields that will be removed
    pub deletions: Vec<String>,

    /// Whether the resource must be replaced
    pub requires_replacement: bool,

    /// Fields causing replacement
    pub replacement_fields: Vec<String>,
}

impl ResourceDiff {
    /// Create a no-change diff
    pub fn no_change() -> Self {
        Self {
            change_type: ChangeType::NoOp,
            additions: HashMap::new(),
            modifications: HashMap::new(),
            deletions: Vec::new(),
            requires_replacement: false,
            replacement_fields: Vec::new(),
        }
    }

    /// Create a create diff
    pub fn create(config: Value) -> Self {
        let additions = if let Value::Object(map) = config {
            map.into_iter().collect()
        } else {
            HashMap::new()
        };

        Self {
            change_type: ChangeType::Create,
            additions,
            modifications: HashMap::new(),
            deletions: Vec::new(),
            requires_replacement: false,
            replacement_fields: Vec::new(),
        }
    }

    /// Create a destroy diff
    pub fn destroy() -> Self {
        Self {
            change_type: ChangeType::Destroy,
            additions: HashMap::new(),
            modifications: HashMap::new(),
            deletions: Vec::new(),
            requires_replacement: false,
            replacement_fields: Vec::new(),
        }
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !matches!(self.change_type, ChangeType::NoOp)
    }
}

/// Type of change to a resource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// No changes needed
    NoOp,
    /// Create new resource
    Create,
    /// Update existing resource in-place
    Update,
    /// Replace resource (destroy + create)
    Replace,
    /// Destroy resource
    Destroy,
    /// Read/refresh only
    Read,
}

/// Result of a resource operation (create/update/destroy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Cloud resource ID
    pub cloud_id: Option<String>,

    /// Final attributes after operation
    pub attributes: Value,

    /// Any outputs to expose
    pub outputs: HashMap<String, Value>,

    /// Warning messages
    pub warnings: Vec<String>,

    /// Error message (if failed)
    pub error: Option<String>,
}

impl ResourceResult {
    /// Create a successful result
    pub fn success(cloud_id: impl Into<String>, attributes: Value) -> Self {
        Self {
            success: true,
            cloud_id: Some(cloud_id.into()),
            attributes,
            outputs: HashMap::new(),
            warnings: Vec::new(),
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            cloud_id: None,
            attributes: Value::Null,
            outputs: HashMap::new(),
            warnings: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// Add an output value
    pub fn with_output(mut self, key: impl Into<String>, value: Value) -> Self {
        self.outputs.insert(key.into(), value);
        self
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Dependency on another resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDependency {
    /// Resource type (e.g., "aws_vpc")
    pub resource_type: String,

    /// Resource name
    pub resource_name: String,

    /// Attribute being referenced (e.g., "id")
    pub attribute: String,

    /// Whether this is a hard dependency (blocks parallel execution)
    pub hard: bool,
}

impl ResourceDependency {
    /// Create a new resource dependency
    pub fn new(
        resource_type: impl Into<String>,
        resource_name: impl Into<String>,
        attribute: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: resource_type.into(),
            resource_name: resource_name.into(),
            attribute: attribute.into(),
            hard: true,
        }
    }

    /// Create a soft dependency
    pub fn soft(mut self) -> Self {
        self.hard = false;
        self
    }

    /// Get the full resource address
    pub fn address(&self) -> String {
        format!("{}.{}", self.resource_type, self.resource_name)
    }
}

/// An infrastructure resource that can be provisioned via cloud API
#[async_trait]
pub trait Resource: Send + Sync + Debug {
    /// Get the resource type (e.g., "aws_vpc", "aws_instance")
    fn resource_type(&self) -> &str;

    /// Get the provider name (e.g., "aws")
    fn provider(&self) -> &str;

    /// Get the resource schema
    fn schema(&self) -> ResourceSchema;

    /// Read current state of a resource from the cloud
    async fn read(&self, id: &str, ctx: &ProviderContext)
        -> ProvisioningResult<ResourceReadResult>;

    /// Plan changes between desired and current state
    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff>;

    /// Create a new resource
    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult>;

    /// Update an existing resource
    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult>;

    /// Destroy a resource
    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult>;

    /// Import an existing resource into state
    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult>;

    /// Extract dependencies from configuration
    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency>;

    /// Get fields that force replacement when changed
    fn forces_replacement(&self) -> Vec<String>;

    /// Validate resource configuration
    fn validate(&self, config: &Value) -> ProvisioningResult<()>;
}

// ============================================================================
// Data Source Traits
// ============================================================================

/// Result of a data source query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceResult {
    /// Whether the query succeeded
    pub success: bool,

    /// Query results
    pub data: Value,

    /// Any warnings
    pub warnings: Vec<String>,

    /// Error message (if failed)
    pub error: Option<String>,
}

impl DataSourceResult {
    /// Create a successful result
    pub fn success(data: Value) -> Self {
        Self {
            success: true,
            data,
            warnings: Vec::new(),
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: Value::Null,
            warnings: Vec::new(),
            error: Some(error.into()),
        }
    }
}

/// A data source for querying existing infrastructure (read-only)
#[async_trait]
pub trait DataSource: Send + Sync + Debug {
    /// Get the data source type
    fn data_source_type(&self) -> &str;

    /// Get the provider name
    fn provider(&self) -> &str;

    /// Get the data source schema
    fn schema(&self) -> ResourceSchema;

    /// Query the data source
    async fn read(
        &self,
        query: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<DataSourceResult>;

    /// Validate query configuration
    fn validate(&self, query: &Value) -> ProvisioningResult<()>;
}

// ============================================================================
// Null/Debug Credentials Implementation
// ============================================================================

/// Debug credentials for testing
#[derive(Debug, Clone)]
pub struct DebugCredentials {
    pub credential_type: String,
}

impl DebugCredentials {
    pub fn new(credential_type: impl Into<String>) -> Self {
        Self {
            credential_type: credential_type.into(),
        }
    }
}

impl ProviderCredentials for DebugCredentials {
    fn credential_type(&self) -> &str {
        &self.credential_type
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": self.credential_type,
            "debug": true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_diff_no_change() {
        let diff = ResourceDiff::no_change();
        assert!(!diff.has_changes());
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_resource_diff_create() {
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns": true
        });
        let diff = ResourceDiff::create(config);
        assert!(diff.has_changes());
        assert_eq!(diff.change_type, ChangeType::Create);
        assert_eq!(diff.additions.len(), 2);
    }

    #[test]
    fn test_resource_result_success() {
        let result = ResourceResult::success("vpc-123", serde_json::json!({"id": "vpc-123"}))
            .with_output("vpc_id", serde_json::json!("vpc-123"))
            .with_warning("Consider enabling flow logs");

        assert!(result.success);
        assert_eq!(result.cloud_id, Some("vpc-123".to_string()));
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_resource_dependency() {
        let dep = ResourceDependency::new("aws_vpc", "main", "id");
        assert_eq!(dep.address(), "aws_vpc.main");
        assert!(dep.hard);

        let soft_dep = dep.clone().soft();
        assert!(!soft_dep.hard);
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff_ms, 1000);
    }
}
