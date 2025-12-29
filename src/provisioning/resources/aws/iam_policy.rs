//! AWS IAM Policy Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS IAM Policies, enabling declarative
//! IAM policy management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_iam_policy:
//!     s3_read_access:
//!       name: s3-read-access-policy
//!       description: "Policy for S3 read access"
//!       policy: |
//!         {
//!           "Version": "2012-10-17",
//!           "Statement": [
//!             {
//!               "Effect": "Allow",
//!               "Action": ["s3:GetObject", "s3:ListBucket"],
//!               "Resource": "*"
//!             }
//!           ]
//!         }
//!       tags:
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "aws")]
use aws_config::BehaviorVersion;
#[cfg(feature = "aws")]
use aws_sdk_iam::types::Tag;
#[cfg(feature = "aws")]
use aws_sdk_iam::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// IAM Policy Resource Configuration
// ============================================================================

/// IAM Policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamPolicyConfig {
    /// Policy name (required, must be unique within account)
    pub name: String,

    /// The policy document (required)
    /// JSON string defining permissions
    pub policy: String,

    /// Description of the policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Path for the policy (default: /)
    #[serde(default = "default_path")]
    pub path: String,

    /// Tags for the policy
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_path() -> String {
    "/".to_string()
}

impl Default for IamPolicyConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            policy: String::new(),
            description: None,
            path: "/".to_string(),
            tags: HashMap::new(),
        }
    }
}

impl IamPolicyConfig {
    /// Validate the configuration
    pub fn validate(&self) -> ProvisioningResult<()> {
        // Validate policy name
        if self.name.is_empty() {
            return Err(ProvisioningError::ValidationError(
                "Policy name cannot be empty".to_string(),
            ));
        }

        if self.name.len() > 128 {
            return Err(ProvisioningError::ValidationError(
                "Policy name cannot exceed 128 characters".to_string(),
            ));
        }

        // Validate policy document is not empty
        if self.policy.is_empty() {
            return Err(ProvisioningError::ValidationError(
                "Policy document cannot be empty".to_string(),
            ));
        }

        // Validate policy is valid JSON
        if serde_json::from_str::<Value>(&self.policy).is_err() {
            return Err(ProvisioningError::ValidationError(
                "Policy must be valid JSON".to_string(),
            ));
        }

        // Validate path
        if !self.path.starts_with('/') {
            return Err(ProvisioningError::ValidationError(
                "Path must start with '/'".to_string(),
            ));
        }

        Ok(())
    }

    /// Parse from serde_json::Value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ConfigError(format!("Failed to parse IAM policy config: {}", e))
        })
    }
}

// ============================================================================
// IAM Policy Attributes (Computed)
// ============================================================================

/// IAM Policy computed attributes
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IamPolicyAttributes {
    /// Policy ARN
    pub arn: String,

    /// Policy ID
    pub policy_id: String,

    /// Policy name
    pub name: String,

    /// Policy path
    pub path: String,

    /// Default version ID
    pub default_version_id: String,

    /// Number of entities attached to this policy
    pub attachment_count: i32,

    /// Creation date
    pub create_date: String,

    /// Last update date
    pub update_date: String,

    /// Tags
    pub tags: HashMap<String, String>,
}

impl IamPolicyAttributes {
    /// Convert to serde_json::Value
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

// ============================================================================
// IAM Policy Resource Implementation
// ============================================================================

/// AWS IAM Policy Resource
#[derive(Debug, Clone)]
pub struct AwsIamPolicyResource {
    schema: ResourceSchema,
}

impl Default for AwsIamPolicyResource {
    fn default() -> Self {
        Self::new()
    }
}

impl AwsIamPolicyResource {
    /// Create a new IAM Policy resource handler
    pub fn new() -> Self {
        Self {
            schema: Self::build_schema(),
        }
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_iam_policy".to_string(),
            description: "AWS IAM Policy resource for managing IAM policies".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the IAM policy (must be unique within account)"
                        .to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 1 },
                        FieldConstraint::MaxLength { max: 128 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "policy".to_string(),
                    field_type: FieldType::String,
                    description: "The policy document (JSON string)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Description of the policy".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MaxLength { max: 1000 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "path".to_string(),
                    field_type: FieldType::String,
                    description: "Path for the IAM policy".to_string(),
                    default: Some(Value::String("/".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the policy".to_string(),
                    default: Some(Value::Object(serde_json::Map::new())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "ARN of the IAM policy".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "policy_id".to_string(),
                    field_type: FieldType::String,
                    description: "The stable unique identifier for the policy".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_version_id".to_string(),
                    field_type: FieldType::String,
                    description: "The identifier for the current policy version".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "attachment_count".to_string(),
                    field_type: FieldType::Integer,
                    description: "Number of entities (users, groups, roles) attached to the policy"
                        .to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "create_date".to_string(),
                    field_type: FieldType::String,
                    description: "Creation date of the policy".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "update_date".to_string(),
                    field_type: FieldType::String,
                    description: "Date when the policy was last updated".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "name".to_string(),
                "path".to_string(),
                "description".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
                delete: 300,
            },
        }
    }

    /// Get IAM client from provider context
    #[cfg(feature = "aws")]
    async fn get_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let region = ctx.config.get("region").and_then(|v| v.as_str());

        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        if let Some(region) = region {
            config_loader = config_loader.region(aws_config::Region::new(region.to_string()));
        }

        let config = config_loader.load().await;
        Ok(Client::new(&config))
    }

    /// Create tags from HashMap
    #[cfg(feature = "aws")]
    fn create_tags(&self, tags: &HashMap<String, String>) -> Vec<Tag> {
        tags.iter()
            .filter_map(|(k, v)| Tag::builder().key(k.clone()).value(v.clone()).build().ok())
            .collect()
    }

    /// Merge default tags with resource tags
    fn merge_tags(
        &self,
        resource_tags: &HashMap<String, String>,
        ctx: &ProviderContext,
    ) -> HashMap<String, String> {
        let mut merged = ctx.default_tags.clone();
        merged.extend(resource_tags.clone());
        merged
    }
}

#[async_trait]
impl Resource for AwsIamPolicyResource {
    fn resource_type(&self) -> &str {
        "aws_iam_policy"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        self.schema.clone()
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let policy_config = IamPolicyConfig::from_value(config)?;
        policy_config.validate()
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        #[cfg(feature = "aws")]
        {
            let client = self.get_client(ctx).await?;

            match client.get_policy().policy_arn(id).send().await {
                Ok(response) => {
                    let policy = response.policy().ok_or_else(|| {
                        ProvisioningError::CloudApiError(format!(
                            "IAM policy '{}' not found",
                            id
                        ))
                    })?;

                    // Extract tags - IAM SDK Tag has &str for key/value
                    let mut tags = HashMap::new();
                    for tag in policy.tags() {
                        tags.insert(tag.key().to_string(), tag.value().to_string());
                    }

                    let attributes = IamPolicyAttributes {
                        arn: policy.arn().unwrap_or_default().to_string(),
                        policy_id: policy.policy_id().unwrap_or_default().to_string(),
                        name: policy.policy_name().unwrap_or_default().to_string(),
                        path: policy.path().unwrap_or_default().to_string(),
                        default_version_id: policy
                            .default_version_id()
                            .unwrap_or_default()
                            .to_string(),
                        attachment_count: policy.attachment_count().unwrap_or(0),
                        create_date: policy
                            .create_date()
                            .map(|d| d.to_string())
                            .unwrap_or_default(),
                        update_date: policy
                            .update_date()
                            .map(|d| d.to_string())
                            .unwrap_or_default(),
                        tags,
                    };

                    Ok(ResourceReadResult {
                        exists: true,
                        cloud_id: Some(id.to_string()),
                        attributes: attributes.to_value(),
                        metadata: HashMap::new(),
                    })
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("NoSuchEntity") {
                        Ok(ResourceReadResult {
                            exists: false,
                            cloud_id: None,
                            attributes: Value::Null,
                            metadata: HashMap::new(),
                        })
                    } else {
                        Err(ProvisioningError::CloudApiError(format!(
                            "Failed to read IAM policy: {}",
                            e
                        )))
                    }
                }
            }
        }

        #[cfg(not(feature = "aws"))]
        {
            let _ = (id, ctx);
            Err(ProvisioningError::ConfigError(
                "AWS feature not enabled".to_string(),
            ))
        }
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let config = IamPolicyConfig::from_value(desired)?;
        config.validate()?;

        // No current state means create
        if current.is_none() {
            return Ok(ResourceDiff::create(desired.clone()));
        }

        let current_val = current.unwrap();
        let force_new_fields = self.forces_replacement();

        let mut modifications = HashMap::new();
        let mut additions = HashMap::new();
        let deletions = Vec::new();
        let mut replacement_fields = Vec::new();

        // Compare fields
        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current_val.as_object())
        {
            // Check for additions and modifications
            for (key, desired_val) in desired_obj {
                // Skip computed fields
                if ["arn", "policy_id", "default_version_id", "attachment_count", "create_date", "update_date"]
                    .contains(&key.as_str())
                {
                    continue;
                }

                if let Some(current_field) = current_obj.get(key) {
                    if desired_val != current_field {
                        modifications.insert(key.clone(), (current_field.clone(), desired_val.clone()));

                        if force_new_fields.iter().any(|f| f == key) {
                            replacement_fields.push(key.clone());
                        }
                    }
                } else {
                    additions.insert(key.clone(), desired_val.clone());
                }
            }
        }

        // Determine change type
        let requires_replacement = !replacement_fields.is_empty();
        let has_changes = !additions.is_empty() || !modifications.is_empty() || !deletions.is_empty();

        let change_type = if requires_replacement {
            ChangeType::Replace
        } else if has_changes {
            ChangeType::Update
        } else {
            ChangeType::NoOp
        };

        Ok(ResourceDiff {
            change_type,
            additions,
            modifications,
            deletions,
            requires_replacement,
            replacement_fields,
        })
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let policy_config = IamPolicyConfig::from_value(config)?;
        policy_config.validate()?;

        #[cfg(feature = "aws")]
        {
            let client = self.get_client(ctx).await?;
            let merged_tags = self.merge_tags(&policy_config.tags, ctx);

            // Create the policy
            let mut create_policy = client
                .create_policy()
                .policy_name(&policy_config.name)
                .policy_document(&policy_config.policy)
                .path(&policy_config.path);

            if let Some(desc) = &policy_config.description {
                create_policy = create_policy.description(desc);
            }

            // Add tags
            for tag in self.create_tags(&merged_tags) {
                create_policy = create_policy.tags(tag);
            }

            let response = create_policy.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to create IAM policy: {}", e))
            })?;

            let policy = response.policy().ok_or_else(|| {
                ProvisioningError::CloudApiError("No policy returned from create".to_string())
            })?;

            let arn = policy.arn().unwrap_or_default().to_string();

            let attributes = IamPolicyAttributes {
                arn: arn.clone(),
                policy_id: policy.policy_id().unwrap_or_default().to_string(),
                name: policy.policy_name().unwrap_or_default().to_string(),
                path: policy.path().unwrap_or_default().to_string(),
                default_version_id: policy
                    .default_version_id()
                    .unwrap_or_default()
                    .to_string(),
                attachment_count: 0,
                create_date: policy
                    .create_date()
                    .map(|d| d.to_string())
                    .unwrap_or_default(),
                update_date: String::new(),
                tags: merged_tags,
            };

            let mut result = ResourceResult::success(arn.clone(), attributes.to_value());
            result.outputs.insert("arn".to_string(), Value::String(arn));
            result.outputs.insert(
                "name".to_string(),
                Value::String(policy_config.name.clone()),
            );
            Ok(result)
        }

        #[cfg(not(feature = "aws"))]
        {
            let _ = ctx;
            Err(ProvisioningError::ConfigError(
                "AWS feature not enabled".to_string(),
            ))
        }
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let policy_config = IamPolicyConfig::from_value(new)?;
        policy_config.validate()?;

        #[cfg(feature = "aws")]
        {
            let client = self.get_client(ctx).await?;

            // Create a new policy version with the updated document
            // First, list existing versions to check if we need to delete old ones
            let versions = client
                .list_policy_versions()
                .policy_arn(id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to list policy versions: {}",
                        e
                    ))
                })?;

            // AWS allows max 5 versions, delete oldest non-default if at limit
            let version_count = versions.versions().len();
            if version_count >= 5 {
                // Find oldest non-default version
                for version in versions.versions() {
                    if !version.is_default_version() {
                        if let Some(version_id) = version.version_id() {
                            client
                                .delete_policy_version()
                                .policy_arn(id)
                                .version_id(version_id)
                                .send()
                                .await
                                .map_err(|e| {
                                    ProvisioningError::CloudApiError(format!(
                                        "Failed to delete old policy version: {}",
                                        e
                                    ))
                                })?;
                            break;
                        }
                    }
                }
            }

            // Create new version and set as default
            client
                .create_policy_version()
                .policy_arn(id)
                .policy_document(&policy_config.policy)
                .set_as_default(true)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to create new policy version: {}",
                        e
                    ))
                })?;

            // Update tags
            let merged_tags = self.merge_tags(&policy_config.tags, ctx);
            if !merged_tags.is_empty() {
                client
                    .tag_policy()
                    .policy_arn(id)
                    .set_tags(Some(self.create_tags(&merged_tags)))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                    })?;
            }

            // Read back the updated policy
            let read_result = self.read(id, ctx).await?;

            let mut result = ResourceResult::success(id, read_result.attributes);
            result.outputs.insert("arn".to_string(), Value::String(id.to_string()));
            Ok(result)
        }

        #[cfg(not(feature = "aws"))]
        {
            let _ = (id, ctx);
            Err(ProvisioningError::ConfigError(
                "AWS feature not enabled".to_string(),
            ))
        }
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        #[cfg(feature = "aws")]
        {
            let client = self.get_client(ctx).await?;

            // Delete all non-default versions first
            if let Ok(versions) = client.list_policy_versions().policy_arn(id).send().await {
                for version in versions.versions() {
                    if !version.is_default_version() {
                        if let Some(version_id) = version.version_id() {
                            let _ = client
                                .delete_policy_version()
                                .policy_arn(id)
                                .version_id(version_id)
                                .send()
                                .await;
                        }
                    }
                }
            }

            // Delete the policy
            client
                .delete_policy()
                .policy_arn(id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to delete IAM policy: {}", e))
                })?;

            Ok(ResourceResult::success(id, Value::Null))
        }

        #[cfg(not(feature = "aws"))]
        {
            let _ = (id, ctx);
            Err(ProvisioningError::ConfigError(
                "AWS feature not enabled".to_string(),
            ))
        }
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;

        if !read_result.exists {
            return Err(ProvisioningError::ResourceNotFound {
                provider: "aws".to_string(),
                resource_type: format!("aws_iam_policy/{}", id),
            });
        }

        let mut result = ResourceResult::success(id, read_result.attributes);
        result.outputs.insert("arn".to_string(), Value::String(id.to_string()));
        Ok(result)
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        // IAM policies don't typically have dependencies on other resources
        vec![]
    }

    fn forces_replacement(&self) -> Vec<String> {
        self.schema.force_new.clone()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::traits::{DebugCredentials, RetryConfig};
    use std::sync::Arc;

    fn test_provider_context() -> ProviderContext {
        ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: serde_json::json!({"region": "us-east-1"}),
            credentials: Arc::new(DebugCredentials::new("test")),
            timeout_seconds: 60,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        }
    }

    #[test]
    fn test_iam_policy_resource_type() {
        let resource = AwsIamPolicyResource::new();
        assert_eq!(resource.resource_type(), "aws_iam_policy");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_iam_policy_schema() {
        let resource = AwsIamPolicyResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_iam_policy");

        // Check required args
        let required_names: Vec<&str> = schema.required_args.iter().map(|f| f.name.as_str()).collect();
        assert!(required_names.contains(&"name"));
        assert!(required_names.contains(&"policy"));

        // Check optional args
        let optional_names: Vec<&str> = schema.optional_args.iter().map(|f| f.name.as_str()).collect();
        assert!(optional_names.contains(&"description"));
        assert!(optional_names.contains(&"path"));
        assert!(optional_names.contains(&"tags"));

        // Check computed attrs
        let computed_names: Vec<&str> = schema.computed_attrs.iter().map(|f| f.name.as_str()).collect();
        assert!(computed_names.contains(&"arn"));
        assert!(computed_names.contains(&"policy_id"));
        assert!(computed_names.contains(&"default_version_id"));
        assert!(computed_names.contains(&"attachment_count"));
        assert!(computed_names.contains(&"create_date"));

        // Check force_new fields
        assert!(schema.force_new.contains(&"name".to_string()));
        assert!(schema.force_new.contains(&"path".to_string()));
        assert!(schema.force_new.contains(&"description".to_string()));
    }

    #[test]
    fn test_iam_policy_config_validation() {
        // Valid config
        let config = IamPolicyConfig {
            name: "test-policy".to_string(),
            policy: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            description: Some("Test policy".to_string()),
            path: "/".to_string(),
            tags: HashMap::new(),
        };
        assert!(config.validate().is_ok());

        // Empty name
        let mut invalid = config.clone();
        invalid.name = "".to_string();
        assert!(invalid.validate().is_err());

        // Name too long
        let mut invalid = config.clone();
        invalid.name = "a".repeat(129);
        assert!(invalid.validate().is_err());

        // Empty policy
        let mut invalid = config.clone();
        invalid.policy = "".to_string();
        assert!(invalid.validate().is_err());

        // Invalid JSON policy
        let mut invalid = config.clone();
        invalid.policy = "not json".to_string();
        assert!(invalid.validate().is_err());

        // Invalid path
        let mut invalid = config.clone();
        invalid.path = "no-slash".to_string();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_iam_policy_config_from_value() {
        let value = serde_json::json!({
            "name": "my-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "description": "My test policy",
            "path": "/",
            "tags": {
                "Environment": "test"
            }
        });

        let config = IamPolicyConfig::from_value(&value).unwrap();
        assert_eq!(config.name, "my-policy");
        assert_eq!(config.tags.get("Environment"), Some(&"test".to_string()));
    }

    #[test]
    fn test_iam_policy_forces_replacement() {
        let resource = AwsIamPolicyResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"name".to_string()));
        assert!(force_new.contains(&"path".to_string()));
        assert!(force_new.contains(&"description".to_string()));
    }

    #[test]
    fn test_iam_policy_attributes_to_value() {
        let attrs = IamPolicyAttributes {
            arn: "arn:aws:iam::123456789012:policy/test-policy".to_string(),
            policy_id: "ANPA1234567890EXAMPLE".to_string(),
            name: "test-policy".to_string(),
            path: "/".to_string(),
            default_version_id: "v1".to_string(),
            attachment_count: 3,
            create_date: "2024-01-01T00:00:00Z".to_string(),
            update_date: "2024-01-02T00:00:00Z".to_string(),
            tags: HashMap::from([("env".to_string(), "prod".to_string())]),
        };

        let value = attrs.to_value();
        assert_eq!(
            value.get("arn").and_then(|v| v.as_str()),
            Some("arn:aws:iam::123456789012:policy/test-policy")
        );
        assert_eq!(
            value.get("name").and_then(|v| v.as_str()),
            Some("test-policy")
        );
        assert_eq!(
            value.get("attachment_count").and_then(|v| v.as_i64()),
            Some(3)
        );
    }

    #[test]
    fn test_iam_policy_validate_trait() {
        let resource = AwsIamPolicyResource::new();

        // Valid config
        let valid = serde_json::json!({
            "name": "valid-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}"
        });
        assert!(resource.validate(&valid).is_ok());

        // Invalid - empty name
        let invalid = serde_json::json!({
            "name": "",
            "policy": "{}"
        });
        assert!(resource.validate(&invalid).is_err());
    }

    #[tokio::test]
    async fn test_iam_policy_plan_create() {
        let resource = AwsIamPolicyResource::new();
        let ctx = test_provider_context();

        let desired = serde_json::json!({
            "name": "new-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "path": "/"
        });

        let diff = resource.plan(&desired, None, &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
        assert!(!diff.requires_replacement);
    }

    #[tokio::test]
    async fn test_iam_policy_plan_no_change() {
        let resource = AwsIamPolicyResource::new();
        let ctx = test_provider_context();

        let config = serde_json::json!({
            "name": "my-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "path": "/"
        });

        let diff = resource.plan(&config, Some(&config), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[tokio::test]
    async fn test_iam_policy_plan_update() {
        let resource = AwsIamPolicyResource::new();
        let ctx = test_provider_context();

        let current = serde_json::json!({
            "name": "my-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "path": "/"
        });

        let desired = serde_json::json!({
            "name": "my-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[{\"Effect\":\"Allow\"}]}",
            "path": "/"
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Update);
        assert!(!diff.requires_replacement);
        assert!(diff.modifications.contains_key("policy"));
    }

    #[tokio::test]
    async fn test_iam_policy_plan_replace() {
        let resource = AwsIamPolicyResource::new();
        let ctx = test_provider_context();

        let current = serde_json::json!({
            "name": "old-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "path": "/"
        });

        let desired = serde_json::json!({
            "name": "new-policy",
            "policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "path": "/"
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
    }

    #[test]
    fn test_iam_policy_no_dependencies() {
        let resource = AwsIamPolicyResource::new();

        let config = serde_json::json!({
            "name": "my-policy",
            "policy": "{}"
        });

        let deps = resource.dependencies(&config);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_default_path() {
        assert_eq!(default_path(), "/");
    }

    #[test]
    fn test_iam_policy_config_default() {
        let config = IamPolicyConfig::default();
        assert_eq!(config.path, "/");
        assert!(config.tags.is_empty());
    }
}
