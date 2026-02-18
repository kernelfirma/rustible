//! AWS IAM Role Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS IAM Roles, enabling declarative
//! IAM role management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_iam_role:
//!     lambda_execution:
//!       name: "lambda-execution-role"
//!       assume_role_policy: |
//!         {
//!           "Version": "2012-10-17",
//!           "Statement": [{
//!             "Effect": "Allow",
//!             "Principal": {"Service": "lambda.amazonaws.com"},
//!             "Action": "sts:AssumeRole"
//!           }]
//!         }
//!       description: "IAM role for Lambda function execution"
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
// IAM Role Configuration
// ============================================================================

/// IAM Role configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamRoleConfig {
    /// Role name
    pub name: String,
    /// Trust policy (assume role policy document)
    pub assume_role_policy: String,
    /// Role description
    pub description: Option<String>,
    /// Path for the role
    pub path: String,
    /// Maximum session duration in seconds (3600-43200)
    pub max_session_duration: i32,
    /// Permissions boundary ARN
    pub permissions_boundary: Option<String>,
    /// Managed policy ARNs to attach
    pub managed_policy_arns: Vec<String>,
    /// Inline policies (name -> policy document)
    pub inline_policies: HashMap<String, String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

impl Default for IamRoleConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            assume_role_policy: String::new(),
            description: None,
            path: "/".to_string(),
            max_session_duration: 3600,
            permissions_boundary: None,
            managed_policy_arns: Vec::new(),
            inline_policies: HashMap::new(),
            tags: HashMap::new(),
        }
    }
}

/// IAM Role attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamRoleAttributes {
    /// Role name
    pub name: String,
    /// Role ARN
    pub arn: String,
    /// Role ID
    pub role_id: String,
    /// Creation date
    pub create_date: String,
    /// Trust policy
    pub assume_role_policy: String,
    /// Description
    pub description: Option<String>,
    /// Path
    pub path: String,
    /// Maximum session duration
    pub max_session_duration: i32,
    /// Permissions boundary ARN
    pub permissions_boundary: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

impl IamRoleAttributes {
    /// Convert to serde_json::Value
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

// ============================================================================
// AWS IAM Role Resource Implementation
// ============================================================================

/// AWS IAM Role resource for infrastructure provisioning
#[derive(Debug, Clone, Default)]
pub struct AwsIamRoleResource;

impl AwsIamRoleResource {
    /// Create a new IAM Role resource
    pub fn new() -> Self {
        Self
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_iam_role".to_string(),
            description: "AWS IAM Role resource".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the IAM role".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 1 },
                        FieldConstraint::MaxLength { max: 64 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "assume_role_policy".to_string(),
                    field_type: FieldType::String,
                    description: "The trust relationship policy document (JSON)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Description of the IAM role".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "path".to_string(),
                    field_type: FieldType::String,
                    description: "Path for the role (default: /)".to_string(),
                    default: Some(Value::String("/".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "max_session_duration".to_string(),
                    field_type: FieldType::Number,
                    description: "Maximum session duration in seconds (3600-43200)".to_string(),
                    default: Some(Value::Number(3600.into())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "permissions_boundary".to_string(),
                    field_type: FieldType::String,
                    description: "ARN of the policy used as permissions boundary".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "managed_policy_arns".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of managed policy ARNs to attach".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "inline_policies".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Map of inline policy names to policy documents".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the role".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "Role ARN".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "role_id".to_string(),
                    field_type: FieldType::String,
                    description: "Role unique ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "create_date".to_string(),
                    field_type: FieldType::String,
                    description: "Creation timestamp".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "path".to_string()],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
                delete: 300,
            },
        }
    }

    /// Extract configuration values from JSON
    fn extract_config(config: &Value) -> ProvisioningResult<IamRoleConfig> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("name is required".to_string()))?
            .to_string();

        let assume_role_policy = config
            .get("assume_role_policy")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("assume_role_policy is required".to_string())
            })?
            .to_string();

        let description = config
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let path = config
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/")
            .to_string();

        let max_session_duration = config
            .get("max_session_duration")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .unwrap_or(3600);

        let permissions_boundary = config
            .get("permissions_boundary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let managed_policy_arns = config
            .get("managed_policy_arns")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let inline_policies = if let Some(policies) = config.get("inline_policies") {
            if let Some(obj) = policies.as_object() {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        let tags = if let Some(tags_value) = config.get("tags") {
            if let Some(obj) = tags_value.as_object() {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        Ok(IamRoleConfig {
            name,
            assume_role_policy,
            description,
            path,
            max_session_duration,
            permissions_boundary,
            managed_policy_arns,
            inline_policies,
            tags,
        })
    }

    /// Create AWS IAM client
    #[cfg(feature = "aws")]
    async fn create_client(ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_iam::config::Region::new(region.clone()))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Read IAM Role by name from AWS
    #[cfg(feature = "aws")]
    async fn read_role_by_name(
        client: &Client,
        name: &str,
    ) -> ProvisioningResult<Option<IamRoleAttributes>> {
        let resp = client.get_role().role_name(name).send().await;

        match resp {
            Ok(output) => {
                if let Some(role) = output.role() {
                    let mut tags = HashMap::new();
                    for tag in role.tags() {
                        tags.insert(tag.key().to_string(), tag.value().to_string());
                    }

                    Ok(Some(IamRoleAttributes {
                        name: role.role_name().to_string(),
                        arn: role.arn().to_string(),
                        role_id: role.role_id().to_string(),
                        create_date: role.create_date().to_string(),
                        assume_role_policy: role
                            .assume_role_policy_document()
                            .unwrap_or_default()
                            .to_string(),
                        description: role.description().map(|s| s.to_string()),
                        path: role.path().to_string(),
                        max_session_duration: role.max_session_duration().unwrap_or(3600),
                        permissions_boundary: role
                            .permissions_boundary()
                            .and_then(|pb| pb.permissions_boundary_arn())
                            .map(|s| s.to_string()),
                        tags,
                    }))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("NoSuchEntity") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to get role: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Create IAM Role in AWS
    #[cfg(feature = "aws")]
    async fn create_role(
        client: &Client,
        config: &IamRoleConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<IamRoleAttributes> {
        // Build tags including default tags from context
        let mut all_tags: Vec<Tag> = ctx
            .default_tags
            .iter()
            .filter_map(|(k, v)| Tag::builder().key(k).value(v).build().ok())
            .collect();

        for (k, v) in &config.tags {
            if let Ok(tag) = Tag::builder().key(k).value(v).build() {
                all_tags.push(tag);
            }
        }

        let mut create_req = client
            .create_role()
            .role_name(&config.name)
            .assume_role_policy_document(&config.assume_role_policy)
            .path(&config.path)
            .max_session_duration(config.max_session_duration);

        if let Some(ref desc) = config.description {
            create_req = create_req.description(desc);
        }

        if let Some(ref boundary) = config.permissions_boundary {
            create_req = create_req.permissions_boundary(boundary);
        }

        if !all_tags.is_empty() {
            create_req = create_req.set_tags(Some(all_tags));
        }

        create_req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create role: {}", e))
        })?;

        // Attach managed policies
        for policy_arn in &config.managed_policy_arns {
            client
                .attach_role_policy()
                .role_name(&config.name)
                .policy_arn(policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to attach policy {}: {}",
                        policy_arn, e
                    ))
                })?;
        }

        // Add inline policies
        for (policy_name, policy_doc) in &config.inline_policies {
            client
                .put_role_policy()
                .role_name(&config.name)
                .policy_name(policy_name)
                .policy_document(policy_doc)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to put inline policy {}: {}",
                        policy_name, e
                    ))
                })?;
        }

        // Read the created role
        Self::read_role_by_name(client, &config.name)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read created role".to_string())
            })
    }

    /// Update IAM Role in AWS
    #[cfg(feature = "aws")]
    async fn update_role(
        client: &Client,
        name: &str,
        old_config: &IamRoleConfig,
        new_config: &IamRoleConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<IamRoleAttributes> {
        // Update assume role policy if changed
        if old_config.assume_role_policy != new_config.assume_role_policy {
            client
                .update_assume_role_policy()
                .role_name(name)
                .policy_document(&new_config.assume_role_policy)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to update assume role policy: {}",
                        e
                    ))
                })?;
        }

        // Update description if changed
        if old_config.description != new_config.description {
            client
                .update_role()
                .role_name(name)
                .set_description(new_config.description.clone())
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to update role description: {}",
                        e
                    ))
                })?;
        }

        // Update max session duration if changed
        if old_config.max_session_duration != new_config.max_session_duration {
            client
                .update_role()
                .role_name(name)
                .max_session_duration(new_config.max_session_duration)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to update max session duration: {}",
                        e
                    ))
                })?;
        }

        // Update managed policies
        let old_policies: std::collections::HashSet<_> =
            old_config.managed_policy_arns.iter().collect();
        let new_policies: std::collections::HashSet<_> =
            new_config.managed_policy_arns.iter().collect();

        // Detach removed policies
        for policy_arn in old_policies.difference(&new_policies) {
            client
                .detach_role_policy()
                .role_name(name)
                .policy_arn(*policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to detach policy {}: {}",
                        policy_arn, e
                    ))
                })?;
        }

        // Attach new policies
        for policy_arn in new_policies.difference(&old_policies) {
            client
                .attach_role_policy()
                .role_name(name)
                .policy_arn(*policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to attach policy {}: {}",
                        policy_arn, e
                    ))
                })?;
        }

        // Update tags if changed
        if old_config.tags != new_config.tags {
            // Remove old tags
            let old_tag_keys: Vec<_> = old_config.tags.keys().cloned().collect();
            if !old_tag_keys.is_empty() {
                client
                    .untag_role()
                    .role_name(name)
                    .set_tag_keys(Some(old_tag_keys))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to untag role: {}", e))
                    })?;
            }

            // Add new tags including context defaults
            let mut all_tags: Vec<Tag> = ctx
                .default_tags
                .iter()
                .filter_map(|(k, v)| Tag::builder().key(k).value(v).build().ok())
                .collect();

            for (k, v) in &new_config.tags {
                if let Ok(tag) = Tag::builder().key(k).value(v).build() {
                    all_tags.push(tag);
                }
            }

            if !all_tags.is_empty() {
                client
                    .tag_role()
                    .role_name(name)
                    .set_tags(Some(all_tags))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to tag role: {}", e))
                    })?;
            }
        }

        // Read updated role
        Self::read_role_by_name(client, name).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("Failed to read updated role".to_string())
        })
    }

    /// Delete IAM Role in AWS
    #[cfg(feature = "aws")]
    async fn delete_role(client: &Client, name: &str) -> ProvisioningResult<()> {
        // First, detach all managed policies
        let attached = client
            .list_attached_role_policies()
            .role_name(name)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to list attached policies: {}", e))
            })?;

        for policy in attached.attached_policies() {
            if let Some(arn) = policy.policy_arn() {
                client
                    .detach_role_policy()
                    .role_name(name)
                    .policy_arn(arn)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to detach policy {}: {}",
                            arn, e
                        ))
                    })?;
            }
        }

        // Delete all inline policies
        let inline = client
            .list_role_policies()
            .role_name(name)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to list inline policies: {}", e))
            })?;

        for policy_name in inline.policy_names() {
            client
                .delete_role_policy()
                .role_name(name)
                .policy_name(policy_name)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to delete inline policy {}: {}",
                        policy_name, e
                    ))
                })?;
        }

        // Delete the role
        client
            .delete_role()
            .role_name(name)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete role: {}", e))
            })?;

        Ok(())
    }

    /// Compute diff between desired and current state
    fn compute_diff(
        desired: &Value,
        current: Option<&Value>,
        force_new_fields: &[String],
    ) -> ProvisioningResult<ResourceDiff> {
        if current.is_none() {
            return Ok(ResourceDiff::create(desired.clone()));
        }

        let current = current.unwrap();

        let mut modifications = HashMap::new();
        let mut replacement_fields = Vec::new();

        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current.as_object()) {
            for (key, desired_val) in desired_obj {
                // Skip computed fields
                if ["arn", "role_id", "create_date"].contains(&key.as_str()) {
                    continue;
                }

                if let Some(current_val) = current_obj.get(key) {
                    if desired_val != current_val {
                        modifications
                            .insert(key.clone(), (current_val.clone(), desired_val.clone()));

                        if force_new_fields.contains(key) {
                            replacement_fields.push(key.clone());
                        }
                    }
                }
            }
        }

        let requires_replacement = !replacement_fields.is_empty();
        let has_changes = !modifications.is_empty();

        let change_type = if requires_replacement {
            ChangeType::Replace
        } else if has_changes {
            ChangeType::Update
        } else {
            ChangeType::NoOp
        };

        Ok(ResourceDiff {
            change_type,
            additions: HashMap::new(),
            modifications,
            deletions: Vec::new(),
            requires_replacement,
            replacement_fields,
        })
    }
}

#[async_trait]
impl Resource for AwsIamRoleResource {
    fn resource_type(&self) -> &str {
        "aws_iam_role"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        Self::build_schema()
    }

    #[cfg(feature = "aws")]
    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        let client = Self::create_client(ctx).await?;

        match Self::read_role_by_name(&client, id).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceReadResult::found(id, attributes))
            }
            None => Ok(ResourceReadResult::not_found()),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let force_new = self.forces_replacement();
        Self::compute_diff(desired, current, &force_new)
    }

    #[cfg(feature = "aws")]
    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let role_config = Self::extract_config(config)?;
        let client = Self::create_client(ctx).await?;

        match Self::create_role(&client, &role_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.name, attributes);
                result
                    .outputs
                    .insert("name".to_string(), Value::String(attrs.name.clone()));
                result
                    .outputs
                    .insert("arn".to_string(), Value::String(attrs.arn));
                Ok(result)
            }
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let old_config = Self::extract_config(old)?;
        let new_config = Self::extract_config(new)?;
        let client = Self::create_client(ctx).await?;

        match Self::update_role(&client, id, &old_config, &new_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.name, attributes);
                result
                    .outputs
                    .insert("name".to_string(), Value::String(attrs.name.clone()));
                result
                    .outputs
                    .insert("arn".to_string(), Value::String(attrs.arn));
                Ok(result)
            }
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;

        match Self::delete_role(&client, id).await {
            Ok(()) => Ok(ResourceResult::success(id, Value::Null)),
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;

        match Self::read_role_by_name(&client, id).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.name, attributes);
                result
                    .outputs
                    .insert("name".to_string(), Value::String(attrs.name.clone()));
                result
                    .outputs
                    .insert("arn".to_string(), Value::String(attrs.arn));
                Ok(result)
            }
            None => Ok(ResourceResult::failure(format!("Role '{}' not found", id))),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Check permissions_boundary for dependency
        if let Some(boundary) = config.get("permissions_boundary").and_then(|v| v.as_str()) {
            if boundary.starts_with("{{") && boundary.contains("resources.") {
                deps.push(ResourceDependency {
                    resource_type: "aws_iam_policy".to_string(),
                    resource_name: boundary.to_string(),
                    attribute: "arn".to_string(),
                    hard: true,
                });
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "path".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate name
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("name is required".to_string()))?;

        if name.is_empty() {
            return Err(ProvisioningError::ValidationError(
                "name cannot be empty".to_string(),
            ));
        }

        if name.len() > 64 {
            return Err(ProvisioningError::ValidationError(
                "name cannot exceed 64 characters".to_string(),
            ));
        }

        // Validate assume_role_policy
        let policy = config
            .get("assume_role_policy")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("assume_role_policy is required".to_string())
            })?;

        // Validate it's valid JSON
        serde_json::from_str::<Value>(policy).map_err(|_| {
            ProvisioningError::ValidationError("assume_role_policy must be valid JSON".to_string())
        })?;

        // Validate max_session_duration if provided
        if let Some(duration) = config.get("max_session_duration").and_then(|v| v.as_i64()) {
            if !(3600..=43200).contains(&duration) {
                return Err(ProvisioningError::ValidationError(
                    "max_session_duration must be between 3600 and 43200 seconds".to_string(),
                ));
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iam_role_resource_type() {
        let resource = AwsIamRoleResource::new();
        assert_eq!(resource.resource_type(), "aws_iam_role");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_iam_role_schema() {
        let resource = AwsIamRoleResource::new();
        let schema = resource.schema();
        assert_eq!(schema.resource_type, "aws_iam_role");
        assert!(!schema.required_args.is_empty());
        assert!(!schema.optional_args.is_empty());
        assert!(!schema.computed_attrs.is_empty());
        assert!(schema.force_new.contains(&"name".to_string()));
    }

    #[test]
    fn test_iam_role_config_extraction() {
        let config = serde_json::json!({
            "name": "test-role",
            "assume_role_policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "description": "Test role",
            "tags": {"Environment": "test"}
        });

        let result = AwsIamRoleResource::extract_config(&config);
        assert!(result.is_ok());
        let cfg = result.unwrap();
        assert_eq!(cfg.name, "test-role");
        assert_eq!(cfg.path, "/");
        assert_eq!(cfg.max_session_duration, 3600);
    }

    #[test]
    fn test_iam_role_validation_valid() {
        let resource = AwsIamRoleResource::new();
        let config = serde_json::json!({
            "name": "test-role",
            "assume_role_policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_iam_role_validation_missing_name() {
        let resource = AwsIamRoleResource::new();
        let config = serde_json::json!({
            "assume_role_policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_iam_role_validation_invalid_policy() {
        let resource = AwsIamRoleResource::new();
        let config = serde_json::json!({
            "name": "test-role",
            "assume_role_policy": "not valid json",
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_iam_role_validation_invalid_duration() {
        let resource = AwsIamRoleResource::new();
        let config = serde_json::json!({
            "name": "test-role",
            "assume_role_policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}",
            "max_session_duration": 1000,
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsIamRoleResource::new();
        let forces = resource.forces_replacement();
        assert!(forces.contains(&"name".to_string()));
        assert!(forces.contains(&"path".to_string()));
    }
}
