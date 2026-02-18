//! AWS DB Subnet Group Resource for Infrastructure Provisioning
//!
//! This module implements the `aws_db_subnet_group` resource type for managing
//! RDS DB subnet groups in AWS.
//!
//! # Example
//!
//! ```yaml
//! resources:
//!   aws_db_subnet_group:
//!     database:
//!       name: main-db-subnet-group
//!       description: Subnet group for production RDS instances
//!       subnet_ids:
//!         - "{{ resources.aws_subnet.private_a.id }}"
//!         - "{{ resources.aws_subnet.private_b.id }}"
//!       tags:
//!         Environment: production
//!         Team: database
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_rds::types::Tag;
use aws_sdk_rds::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// DB Subnet Group Resource
// ============================================================================

/// AWS DB Subnet Group resource implementation
#[derive(Debug, Clone)]
pub struct AwsDbSubnetGroupResource;

impl AwsDbSubnetGroupResource {
    /// Create a new instance
    pub fn new() -> Self {
        Self
    }

    /// Create AWS RDS client from provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let region = ctx.region.clone();

        let config = if let Some(region_str) = region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_rds::config::Region::new(region_str))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Parse configuration from JSON Value
    fn parse_config(&self, config: &Value) -> ProvisioningResult<DbSubnetGroupConfig> {
        serde_json::from_value(config.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid DB subnet group configuration: {}",
                e
            ))
        })
    }

    /// Find DB subnet group by name
    async fn find_by_name(
        &self,
        name: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<DbSubnetGroupState>> {
        let client = self.create_client(ctx).await?;

        let resp = client
            .describe_db_subnet_groups()
            .db_subnet_group_name(name)
            .send()
            .await;

        match resp {
            Ok(r) => {
                if let Some(group) = r.db_subnet_groups().iter().next() {
                    return Ok(Some(self.parse_subnet_group(group)));
                }
                Ok(None)
            }
            Err(e) => {
                if e.to_string().contains("DBSubnetGroupNotFoundFault") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to describe DB subnet group: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Parse AWS DBSubnetGroup into our state
    fn parse_subnet_group(&self, group: &aws_sdk_rds::types::DbSubnetGroup) -> DbSubnetGroupState {
        let subnet_ids: Vec<String> = group
            .subnets()
            .iter()
            .filter_map(|s| s.subnet_identifier().map(|id| id.to_string()))
            .collect();

        DbSubnetGroupState {
            name: group.db_subnet_group_name().unwrap_or_default().to_string(),
            arn: group.db_subnet_group_arn().unwrap_or_default().to_string(),
            description: group
                .db_subnet_group_description()
                .unwrap_or_default()
                .to_string(),
            subnet_ids,
            vpc_id: group.vpc_id().unwrap_or_default().to_string(),
            status: group.subnet_group_status().unwrap_or_default().to_string(),
            supported_network_types: group
                .supported_network_types()
                .iter()
                .map(|t| t.to_string())
                .collect(),
        }
    }
}

impl Default for AwsDbSubnetGroupResource {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DB Subnet Group Configuration (from YAML/JSON)
// ============================================================================

/// DB Subnet Group configuration as parsed from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSubnetGroupConfig {
    /// Subnet group name (required)
    pub name: String,

    /// Description
    #[serde(default = "default_description")]
    pub description: String,

    /// List of subnet IDs
    pub subnet_ids: Vec<String>,

    /// Resource tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_description() -> String {
    "Managed by Rustible".to_string()
}

// ============================================================================
// DB Subnet Group State (from AWS)
// ============================================================================

/// Current state of a DB subnet group from AWS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSubnetGroupState {
    /// Subnet group name
    pub name: String,

    /// Subnet group ARN
    pub arn: String,

    /// Description
    pub description: String,

    /// Subnet IDs
    pub subnet_ids: Vec<String>,

    /// VPC ID (inferred from subnets)
    pub vpc_id: String,

    /// Status
    pub status: String,

    /// Supported network types
    pub supported_network_types: Vec<String>,
}

// ============================================================================
// Resource Trait Implementation
// ============================================================================

#[async_trait]
impl Resource for AwsDbSubnetGroupResource {
    fn resource_type(&self) -> &str {
        "aws_db_subnet_group"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_db_subnet_group".to_string(),
            description: "Provides an AWS RDS DB subnet group resource".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the DB subnet group".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 1 },
                        FieldConstraint::MaxLength { max: 255 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "subnet_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of subnet IDs (at least 2 in different AZs)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Description of the DB subnet group".to_string(),
                    default: Some(Value::String("Managed by Rustible".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Resource tags".to_string(),
                    default: Some(Value::Object(serde_json::Map::new())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "ARN of the DB subnet group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    description: "Status of the DB subnet group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string()],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
                delete: 300,
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        debug!("Reading DB subnet group: {}", id);

        match self.find_by_name(id, ctx).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize DB subnet group state: {}",
                        e
                    ))
                })?;
                Ok(ResourceReadResult::found(&state.name, attributes))
            }
            None => Ok(ResourceReadResult::not_found()),
        }
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let config = self.parse_config(desired)?;

        match current {
            None => {
                // Resource doesn't exist - create
                Ok(ResourceDiff::create(desired.clone()))
            }
            Some(current_value) => {
                let current_state: DbSubnetGroupState =
                    serde_json::from_value(current_value.clone()).map_err(|e| {
                        ProvisioningError::SerializationError(format!(
                            "Failed to parse current state: {}",
                            e
                        ))
                    })?;

                // Name change requires replacement
                if config.name != current_state.name {
                    return Ok(ResourceDiff {
                        change_type: ChangeType::Replace,
                        additions: HashMap::new(),
                        modifications: HashMap::new(),
                        deletions: Vec::new(),
                        requires_replacement: true,
                        replacement_fields: vec!["name".to_string()],
                    });
                }

                // Check for modifiable fields
                let mut modifications = HashMap::new();

                // Description can be updated
                if config.description != current_state.description {
                    modifications.insert(
                        "description".to_string(),
                        (
                            Value::String(current_state.description.clone()),
                            Value::String(config.description.clone()),
                        ),
                    );
                }

                // Subnet IDs can be updated
                let mut current_subnets = current_state.subnet_ids.clone();
                let mut new_subnets = config.subnet_ids.clone();
                current_subnets.sort();
                new_subnets.sort();

                if current_subnets != new_subnets {
                    modifications.insert(
                        "subnet_ids".to_string(),
                        (
                            serde_json::to_value(&current_state.subnet_ids).unwrap(),
                            serde_json::to_value(&config.subnet_ids).unwrap(),
                        ),
                    );
                }

                if modifications.is_empty() {
                    return Ok(ResourceDiff::no_change());
                }

                Ok(ResourceDiff {
                    change_type: ChangeType::Update,
                    additions: HashMap::new(),
                    modifications,
                    deletions: Vec::new(),
                    requires_replacement: false,
                    replacement_fields: Vec::new(),
                })
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let group_config = self.parse_config(config)?;
        let client = self.create_client(ctx).await?;

        info!("Creating DB subnet group: {}", group_config.name);

        // Build tags
        let mut tags = vec![];
        for (key, value) in &group_config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }

        // Apply default tags from provider context
        for (key, value) in &ctx.default_tags {
            if !group_config.tags.contains_key(key) {
                tags.push(Tag::builder().key(key).value(value).build());
            }
        }

        // Create the subnet group
        let mut req = client
            .create_db_subnet_group()
            .db_subnet_group_name(&group_config.name)
            .db_subnet_group_description(&group_config.description)
            .set_subnet_ids(Some(group_config.subnet_ids.clone()));

        if !tags.is_empty() {
            req = req.set_tags(Some(tags));
        }

        req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create DB subnet group: {}", e))
        })?;

        info!("Created DB subnet group: {}", group_config.name);

        // Read back the created group
        let state = self
            .find_by_name(&group_config.name, ctx)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError(
                    "DB subnet group not found after creation".to_string(),
                )
            })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize DB subnet group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(&state.name, attributes)
            .with_output("name", Value::String(state.name.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("vpc_id", Value::String(state.vpc_id.clone())))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let _old_state: DbSubnetGroupState = serde_json::from_value(old.clone()).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to parse old state: {}", e))
        })?;

        let new_config = self.parse_config(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating DB subnet group: {}", id);

        // Update the subnet group
        client
            .modify_db_subnet_group()
            .db_subnet_group_name(id)
            .db_subnet_group_description(&new_config.description)
            .set_subnet_ids(Some(new_config.subnet_ids.clone()))
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to modify DB subnet group: {}", e))
            })?;

        // Read back updated state
        let state = self.find_by_name(id, ctx).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("DB subnet group not found after update".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize DB subnet group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        info!("Deleting DB subnet group: {}", id);

        client
            .delete_db_subnet_group()
            .db_subnet_group_name(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete DB subnet group: {}", e))
            })?;

        info!("Deleted DB subnet group: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        debug!("Importing DB subnet group: {}", id);

        let state =
            self.find_by_name(id, ctx)
                .await?
                .ok_or_else(|| ProvisioningError::ImportError {
                    resource_type: "aws_db_subnet_group".to_string(),
                    resource_id: id.to_string(),
                    message: "DB subnet group not found".to_string(),
                })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize DB subnet group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes)
            .with_output("name", Value::String(state.name.clone()))
            .with_output("arn", Value::String(state.arn.clone())))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Check for subnet_ids references
        if let Some(subnet_ids) = config.get("subnet_ids").and_then(|v| v.as_array()) {
            for subnet_id in subnet_ids {
                if let Some(subnet_str) = subnet_id.as_str() {
                    if let Some(captures) = parse_resource_reference(subnet_str) {
                        deps.push(ResourceDependency::new(
                            captures.resource_type,
                            captures.resource_name,
                            captures.attribute,
                        ));
                    }
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate name is present
        let name = config.get("name").and_then(|v| v.as_str());
        if name.is_none() || name.unwrap().is_empty() {
            return Err(ProvisioningError::ValidationError(
                "name is required".to_string(),
            ));
        }

        // Validate subnet_ids is present and has at least 2 entries
        let subnet_ids = config.get("subnet_ids").and_then(|v| v.as_array());

        if subnet_ids.is_none() {
            return Err(ProvisioningError::ValidationError(
                "subnet_ids is required".to_string(),
            ));
        }

        let subnet_ids = subnet_ids.unwrap();
        if subnet_ids.len() < 2 {
            return Err(ProvisioningError::ValidationError(
                "subnet_ids must contain at least 2 subnet IDs in different availability zones"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parsed resource reference
struct ResourceReference {
    resource_type: String,
    resource_name: String,
    attribute: String,
}

/// Parse a resource reference string like "{{ resources.aws_subnet.private_a.id }}"
fn parse_resource_reference(reference: &str) -> Option<ResourceReference> {
    let trimmed = reference.trim();
    if !trimmed.starts_with("{{") || !trimmed.ends_with("}}") {
        return None;
    }

    let inner = trimmed
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
        .map(|s| s.trim())?;

    if !inner.starts_with("resources.") {
        return None;
    }

    let parts: Vec<&str> = inner.strip_prefix("resources.")?.split('.').collect();
    if parts.len() < 3 {
        return None;
    }

    Some(ResourceReference {
        resource_type: parts[0].to_string(),
        resource_name: parts[1].to_string(),
        attribute: parts[2..].join("."),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_type() {
        let resource = AwsDbSubnetGroupResource::new();
        assert_eq!(resource.resource_type(), "aws_db_subnet_group");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsDbSubnetGroupResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_db_subnet_group");
        assert!(schema.required_args.iter().any(|f| f.name == "name"));
        assert!(schema.required_args.iter().any(|f| f.name == "subnet_ids"));
    }

    #[test]
    fn test_parse_config() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "test-subnet-group",
            "description": "Test subnet group",
            "subnet_ids": ["subnet-1", "subnet-2"],
            "tags": {
                "Environment": "test"
            }
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.name, "test-subnet-group");
        assert_eq!(parsed.description, "Test subnet group");
        assert_eq!(parsed.subnet_ids.len(), 2);
        assert_eq!(parsed.tags.get("Environment"), Some(&"test".to_string()));
    }

    #[test]
    fn test_parse_config_defaults() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "minimal-group",
            "subnet_ids": ["subnet-1", "subnet-2"]
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.description, "Managed by Rustible");
        assert!(parsed.tags.is_empty());
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "test-group",
            "subnet_ids": ["subnet-1", "subnet-2"]
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_name() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "subnet_ids": ["subnet-1", "subnet-2"]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_missing_subnets() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "test-group"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_insufficient_subnets() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "test-group",
            "subnet_ids": ["subnet-1"]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsDbSubnetGroupResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"name".to_string()));
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsDbSubnetGroupResource::new();
        let config = serde_json::json!({
            "name": "test-group",
            "subnet_ids": [
                "{{ resources.aws_subnet.private_a.id }}",
                "{{ resources.aws_subnet.private_b.id }}"
            ]
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "private_a"));
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "private_b"));
    }

    #[test]
    fn test_state_serialization() {
        let state = DbSubnetGroupState {
            name: "test-group".to_string(),
            arn: "arn:aws:rds:us-east-1:123456789:subgrp:test-group".to_string(),
            description: "Test group".to_string(),
            subnet_ids: vec!["subnet-1".to_string(), "subnet-2".to_string()],
            vpc_id: "vpc-123".to_string(),
            status: "Complete".to_string(),
            supported_network_types: vec!["IPV4".to_string()],
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["name"], "test-group");
        assert_eq!(json["vpc_id"], "vpc-123");
    }

    #[test]
    fn test_parse_resource_reference() {
        let ref1 = parse_resource_reference("{{ resources.aws_subnet.private_a.id }}");
        assert!(ref1.is_some());
        let ref1 = ref1.unwrap();
        assert_eq!(ref1.resource_type, "aws_subnet");
        assert_eq!(ref1.resource_name, "private_a");
        assert_eq!(ref1.attribute, "id");

        // Not a reference
        let ref2 = parse_resource_reference("subnet-12345");
        assert!(ref2.is_none());
    }
}
