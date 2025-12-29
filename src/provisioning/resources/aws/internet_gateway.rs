//! AWS Internet Gateway Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS Internet Gateways, enabling declarative
//! Internet Gateway management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_internet_gateway:
//!     main:
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//!       tags:
//!         Name: main-igw
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
use aws_sdk_ec2::types::{Filter, ResourceType, Tag, TagSpecification};
#[cfg(feature = "aws")]
use aws_sdk_ec2::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Internet Gateway Resource Configuration
// ============================================================================

/// Internet Gateway resource attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetGatewayAttributes {
    /// Internet Gateway ID (e.g., igw-12345678)
    pub id: String,
    /// Internet Gateway ARN
    pub arn: String,
    /// Owner ID (AWS account ID)
    pub owner_id: String,
    /// Attached VPC ID (if attached)
    pub vpc_id: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

/// Internet Gateway resource configuration (from user)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetGatewayConfig {
    /// VPC ID to attach the Internet Gateway to
    pub vpc_id: Option<String>,
    /// Tags to apply to the Internet Gateway
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl InternetGatewayConfig {
    /// Parse configuration from serde_json::Value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        let vpc_id = value
            .get("vpc_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let tags = value
            .get("tags")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self { vpc_id, tags })
    }

    /// Convert to serde_json::Value
    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();

        if let Some(ref vpc_id) = self.vpc_id {
            map.insert("vpc_id".to_string(), Value::String(vpc_id.clone()));
        }

        if !self.tags.is_empty() {
            let tags: serde_json::Map<String, Value> = self
                .tags
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            map.insert("tags".to_string(), Value::Object(tags));
        }

        Value::Object(map)
    }
}

// ============================================================================
// Internet Gateway Resource Implementation
// ============================================================================

/// AWS Internet Gateway resource implementation
#[derive(Debug, Clone)]
pub struct AwsInternetGatewayResource;

impl Default for AwsInternetGatewayResource {
    fn default() -> Self {
        Self::new()
    }
}

impl AwsInternetGatewayResource {
    /// Create a new Internet Gateway resource handler
    pub fn new() -> Self {
        Self
    }

    /// Build schema for Internet Gateway resource
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            provider: "aws".to_string(),
            resource_type: "aws_internet_gateway".to_string(),
            description: "AWS Internet Gateway for VPC internet access".to_string(),
            fields: vec![
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ID to attach the Internet Gateway to".to_string(),
                    required: false,
                    computed: false,
                    force_new: false,
                    sensitive: false,
                    default: None,
                    constraints: vec![],
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map,
                    description: "Tags to apply to the Internet Gateway".to_string(),
                    required: false,
                    computed: false,
                    force_new: false,
                    sensitive: false,
                    default: None,
                    constraints: vec![],
                },
            ],
            computed_fields: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Internet Gateway ID".to_string(),
                    required: false,
                    computed: true,
                    force_new: false,
                    sensitive: false,
                    default: None,
                    constraints: vec![],
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "Internet Gateway ARN".to_string(),
                    required: false,
                    computed: true,
                    force_new: false,
                    sensitive: false,
                    default: None,
                    constraints: vec![],
                },
                SchemaField {
                    name: "owner_id".to_string(),
                    field_type: FieldType::String,
                    description: "AWS account ID of the owner".to_string(),
                    required: false,
                    computed: true,
                    force_new: false,
                    sensitive: false,
                    default: None,
                    constraints: vec![],
                },
            ],
            timeouts: ResourceTimeouts {
                create: Some(std::time::Duration::from_secs(300)),
                read: Some(std::time::Duration::from_secs(60)),
                update: Some(std::time::Duration::from_secs(300)),
                delete: Some(std::time::Duration::from_secs(300)),
            },
        }
    }

    /// Compare tags for changes
    fn tags_changed(current: &HashMap<String, String>, desired: &HashMap<String, String>) -> bool {
        if current.len() != desired.len() {
            return true;
        }
        for (key, value) in desired {
            if current.get(key) != Some(value) {
                return true;
            }
        }
        false
    }

    /// Create AWS EC2 client
    #[cfg(feature = "aws")]
    async fn create_client(ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let region = ctx
            .config
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-east-1");

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_ec2::config::Region::new(region.to_string()))
            .load()
            .await;

        Ok(Client::new(&config))
    }

    /// Convert HashMap tags to AWS Tag format
    #[cfg(feature = "aws")]
    fn to_aws_tags(tags: &HashMap<String, String>) -> Vec<Tag> {
        tags.iter()
            .map(|(k, v)| Tag::builder().key(k).value(v).build())
            .collect()
    }

    /// Convert AWS tags to HashMap
    #[cfg(feature = "aws")]
    fn from_aws_tags(tags: &[Tag]) -> HashMap<String, String> {
        tags.iter()
            .filter_map(|t| {
                let key = t.key()?;
                let value = t.value()?;
                Some((key.to_string(), value.to_string()))
            })
            .collect()
    }
}

#[async_trait]
impl Resource for AwsInternetGatewayResource {
    fn resource_type(&self) -> &str {
        "aws_internet_gateway"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        Self::build_schema()
    }

    #[cfg(feature = "aws")]
    async fn read(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceReadResult> {
        let client = Self::create_client(ctx).await?;

        let result = client
            .describe_internet_gateways()
            .internet_gateway_ids(id)
            .send()
            .await;

        match result {
            Ok(response) => {
                if let Some(igw) = response.internet_gateways().first() {
                    let igw_id = igw.internet_gateway_id().unwrap_or_default();
                    let owner_id = igw.owner_id().unwrap_or_default();
                    let tags = Self::from_aws_tags(igw.tags());

                    // Get attached VPC ID
                    let vpc_id = igw
                        .attachments()
                        .first()
                        .and_then(|a| a.vpc_id())
                        .map(|s| s.to_string());

                    let region = ctx
                        .config
                        .get("region")
                        .and_then(|v| v.as_str())
                        .unwrap_or("us-east-1");

                    let arn = format!(
                        "arn:aws:ec2:{}:{}:internet-gateway/{}",
                        region, owner_id, igw_id
                    );

                    let attributes = InternetGatewayAttributes {
                        id: igw_id.to_string(),
                        arn,
                        owner_id: owner_id.to_string(),
                        vpc_id: vpc_id.clone(),
                        tags: tags.clone(),
                    };

                    let mut config_map = serde_json::Map::new();
                    if let Some(ref vpc) = vpc_id {
                        config_map.insert("vpc_id".to_string(), Value::String(vpc.clone()));
                    }
                    if !tags.is_empty() {
                        let tags_obj: serde_json::Map<String, Value> = tags
                            .iter()
                            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                            .collect();
                        config_map.insert("tags".to_string(), Value::Object(tags_obj));
                    }

                    Ok(ResourceReadResult {
                        exists: true,
                        id: Some(igw_id.to_string()),
                        attributes: serde_json::to_value(&attributes)
                            .unwrap_or(Value::Object(serde_json::Map::new())),
                        config: Value::Object(config_map),
                    })
                } else {
                    Ok(ResourceReadResult {
                        exists: false,
                        id: None,
                        attributes: Value::Null,
                        config: Value::Null,
                    })
                }
            }
            Err(e) => {
                if e.to_string().contains("InvalidInternetGatewayID.NotFound") {
                    Ok(ResourceReadResult {
                        exists: false,
                        id: None,
                        attributes: Value::Null,
                        config: Value::Null,
                    })
                } else {
                    Err(ProvisioningError::ProviderError(format!(
                        "Failed to read Internet Gateway: {}",
                        e
                    )))
                }
            }
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn read(&self, _id: &str, _ctx: &ProviderContext) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::ProviderError(
            "AWS feature not enabled".to_string(),
        ))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let desired_config = InternetGatewayConfig::from_value(desired)?;

        match current {
            None => Ok(ResourceDiff {
                change_type: ChangeType::Create,
                before: None,
                after: Some(desired_config.to_value()),
                changed_fields: vec!["vpc_id".to_string(), "tags".to_string()],
                forces_replacement: false,
            }),
            Some(current_val) => {
                let current_config = InternetGatewayConfig::from_value(current_val)?;
                let mut changed_fields = Vec::new();
                let mut forces_replacement = false;

                // VPC attachment change forces replacement
                if desired_config.vpc_id != current_config.vpc_id {
                    changed_fields.push("vpc_id".to_string());
                    forces_replacement = true;
                }

                // Tags can be updated in place
                if Self::tags_changed(&current_config.tags, &desired_config.tags) {
                    changed_fields.push("tags".to_string());
                }

                if changed_fields.is_empty() {
                    Ok(ResourceDiff {
                        change_type: ChangeType::NoOp,
                        before: Some(current_config.to_value()),
                        after: Some(desired_config.to_value()),
                        changed_fields: vec![],
                        forces_replacement: false,
                    })
                } else if forces_replacement {
                    Ok(ResourceDiff {
                        change_type: ChangeType::Replace,
                        before: Some(current_config.to_value()),
                        after: Some(desired_config.to_value()),
                        changed_fields,
                        forces_replacement: true,
                    })
                } else {
                    Ok(ResourceDiff {
                        change_type: ChangeType::Update,
                        before: Some(current_config.to_value()),
                        after: Some(desired_config.to_value()),
                        changed_fields,
                        forces_replacement: false,
                    })
                }
            }
        }
    }

    #[cfg(feature = "aws")]
    async fn create(&self, config: &Value, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;
        let igw_config = InternetGatewayConfig::from_value(config)?;

        // Create the Internet Gateway
        let mut create_request = client.create_internet_gateway();

        // Add tags if specified
        if !igw_config.tags.is_empty() {
            let tag_spec = TagSpecification::builder()
                .resource_type(ResourceType::InternetGateway)
                .set_tags(Some(Self::to_aws_tags(&igw_config.tags)))
                .build();
            create_request = create_request.tag_specifications(tag_spec);
        }

        let create_result = create_request.send().await.map_err(|e| {
            ProvisioningError::ProviderError(format!("Failed to create Internet Gateway: {}", e))
        })?;

        let igw = create_result.internet_gateway().ok_or_else(|| {
            ProvisioningError::ProviderError("No Internet Gateway returned".to_string())
        })?;

        let igw_id = igw.internet_gateway_id().ok_or_else(|| {
            ProvisioningError::ProviderError("No Internet Gateway ID returned".to_string())
        })?;

        // Attach to VPC if specified
        if let Some(ref vpc_id) = igw_config.vpc_id {
            client
                .attach_internet_gateway()
                .internet_gateway_id(igw_id)
                .vpc_id(vpc_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::ProviderError(format!(
                        "Failed to attach Internet Gateway to VPC: {}",
                        e
                    ))
                })?;
        }

        // Read back the resource to get all attributes
        let read_result = self.read(igw_id, ctx).await?;

        Ok(ResourceResult {
            id: igw_id.to_string(),
            attributes: read_result.attributes,
            outputs: HashMap::new(),
        })
    }

    #[cfg(not(feature = "aws"))]
    async fn create(&self, _config: &Value, _ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderError(
            "AWS feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "aws")]
    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;
        let new_config = InternetGatewayConfig::from_value(new)?;

        // Update tags
        if !new_config.tags.is_empty() {
            // Delete existing tags first
            let current = self.read(id, ctx).await?;
            if current.exists {
                if let Some(attrs) = current.attributes.as_object() {
                    if let Some(tags) = attrs.get("tags").and_then(|v| v.as_object()) {
                        let tag_keys: Vec<String> = tags.keys().cloned().collect();
                        if !tag_keys.is_empty() {
                            client
                                .delete_tags()
                                .resources(id)
                                .set_tags(Some(
                                    tag_keys
                                        .iter()
                                        .map(|k| Tag::builder().key(k).build())
                                        .collect(),
                                ))
                                .send()
                                .await
                                .map_err(|e| {
                                    ProvisioningError::ProviderError(format!(
                                        "Failed to delete tags: {}",
                                        e
                                    ))
                                })?;
                        }
                    }
                }
            }

            // Create new tags
            client
                .create_tags()
                .resources(id)
                .set_tags(Some(Self::to_aws_tags(&new_config.tags)))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::ProviderError(format!("Failed to create tags: {}", e))
                })?;
        }

        // Read back the resource
        let read_result = self.read(id, ctx).await?;

        Ok(ResourceResult {
            id: id.to_string(),
            attributes: read_result.attributes,
            outputs: HashMap::new(),
        })
    }

    #[cfg(not(feature = "aws"))]
    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderError(
            "AWS feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "aws")]
    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;

        // First check if attached to a VPC and detach
        let read_result = self.read(id, ctx).await?;
        if read_result.exists {
            if let Some(vpc_id) = read_result
                .attributes
                .get("vpc_id")
                .and_then(|v| v.as_str())
            {
                client
                    .detach_internet_gateway()
                    .internet_gateway_id(id)
                    .vpc_id(vpc_id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::ProviderError(format!(
                            "Failed to detach Internet Gateway: {}",
                            e
                        ))
                    })?;
            }
        }

        // Delete the Internet Gateway
        client
            .delete_internet_gateway()
            .internet_gateway_id(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ProviderError(format!("Failed to delete Internet Gateway: {}", e))
            })?;

        Ok(ResourceResult {
            id: id.to_string(),
            attributes: Value::Null,
            outputs: HashMap::new(),
        })
    }

    #[cfg(not(feature = "aws"))]
    async fn destroy(&self, _id: &str, _ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderError(
            "AWS feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "aws")]
    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;

        if !read_result.exists {
            return Err(ProvisioningError::ResourceNotFound(format!(
                "Internet Gateway {} not found",
                id
            )));
        }

        Ok(ResourceResult {
            id: id.to_string(),
            attributes: read_result.attributes,
            outputs: HashMap::new(),
        })
    }

    #[cfg(not(feature = "aws"))]
    async fn import(&self, _id: &str, _ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderError(
            "AWS feature not enabled".to_string(),
        ))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // VPC dependency
        if let Some(vpc_id) = config.get("vpc_id").and_then(|v| v.as_str()) {
            if vpc_id.contains("resources.aws_vpc.") {
                // Extract resource reference
                if let Some(name) = vpc_id
                    .strip_prefix("{{ resources.aws_vpc.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_vpc".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                    });
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["vpc_id".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internet_gateway_config_parsing() {
        let value = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "tags": {
                "Name": "test-igw",
                "Environment": "test"
            }
        });

        let config = InternetGatewayConfig::from_value(&value).unwrap();
        assert_eq!(config.vpc_id, Some("vpc-12345678".to_string()));
        assert_eq!(config.tags.get("Name"), Some(&"test-igw".to_string()));
    }

    #[test]
    fn test_internet_gateway_config_to_value() {
        let config = InternetGatewayConfig {
            vpc_id: Some("vpc-12345678".to_string()),
            tags: [("Name".to_string(), "test-igw".to_string())]
                .into_iter()
                .collect(),
        };

        let value = config.to_value();
        assert_eq!(value.get("vpc_id").unwrap().as_str(), Some("vpc-12345678"));
    }

    #[test]
    fn test_schema() {
        let resource = AwsInternetGatewayResource::new();
        let schema = resource.schema();
        assert_eq!(schema.resource_type, "aws_internet_gateway");
        assert_eq!(schema.provider, "aws");
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsInternetGatewayResource::new();
        let config = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}"
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].resource_type, "aws_vpc");
        assert_eq!(deps[0].resource_name, "main");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsInternetGatewayResource::new();
        let forces = resource.forces_replacement();
        assert!(forces.contains(&"vpc_id".to_string()));
    }

    #[tokio::test]
    async fn test_plan_create() {
        let resource = AwsInternetGatewayResource::new();
        let ctx = ProviderContext {
            config: serde_json::json!({"region": "us-east-1"}),
            credentials: None,
        };

        let desired = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "tags": {"Name": "test"}
        });

        let diff = resource.plan(&desired, None, &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Create));
    }

    #[tokio::test]
    async fn test_plan_no_change() {
        let resource = AwsInternetGatewayResource::new();
        let ctx = ProviderContext {
            config: serde_json::json!({"region": "us-east-1"}),
            credentials: None,
        };

        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "tags": {"Name": "test"}
        });

        let diff = resource.plan(&config, Some(&config), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::NoOp));
    }

    #[tokio::test]
    async fn test_plan_update_tags() {
        let resource = AwsInternetGatewayResource::new();
        let ctx = ProviderContext {
            config: serde_json::json!({"region": "us-east-1"}),
            credentials: None,
        };

        let current = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "tags": {"Name": "test"}
        });

        let desired = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "tags": {"Name": "test", "Environment": "prod"}
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Update));
        assert!(diff.changed_fields.contains(&"tags".to_string()));
    }

    #[tokio::test]
    async fn test_plan_replace_vpc() {
        let resource = AwsInternetGatewayResource::new();
        let ctx = ProviderContext {
            config: serde_json::json!({"region": "us-east-1"}),
            credentials: None,
        };

        let current = serde_json::json!({
            "vpc_id": "vpc-12345678"
        });

        let desired = serde_json::json!({
            "vpc_id": "vpc-87654321"
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Replace));
        assert!(diff.forces_replacement);
    }
}
