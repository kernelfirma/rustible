//! AWS Subnet Resource for Infrastructure Provisioning
//!
//! This module provides a declarative resource for managing AWS VPC subnets
//! using the provisioning framework. It enables Terraform-like subnet management
//! through cloud APIs.
//!
//! # Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_subnet:
//!     public:
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//!       cidr_block: "10.0.1.0/24"
//!       availability_zone: "us-east-1a"
//!       map_public_ip_on_launch: true
//!       tags:
//!         Name: public-subnet
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{ResourceType, Tag, TagSpecification};
use aws_sdk_ec2::Client;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// AWS Subnet Resource
// ============================================================================

/// AWS Subnet Resource for VPC subnet management
///
/// This resource provides full lifecycle management for AWS VPC subnets:
/// - Create subnets with CIDR blocks within a VPC
/// - Configure public IP auto-assignment
/// - Set availability zones
/// - Manage IPv6 configurations
/// - Apply and update tags
#[derive(Debug, Clone)]
pub struct AwsSubnetResource;

impl Default for AwsSubnetResource {
    fn default() -> Self {
        Self::new()
    }
}

impl AwsSubnetResource {
    /// Create a new AWS Subnet resource instance
    pub fn new() -> Self {
        Self
    }

    /// Create an AWS EC2 client from the provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let region = ctx.region.clone();

        let config = if let Some(region_str) = region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_ec2::config::Region::new(region_str))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Extract tags from configuration
    fn extract_tags(config: &Value) -> HashMap<String, String> {
        let mut tags = HashMap::new();

        if let Some(tag_obj) = config.get("tags").and_then(|v| v.as_object()) {
            for (k, v) in tag_obj {
                if let Some(vs) = v.as_str() {
                    tags.insert(k.clone(), vs.to_string());
                } else {
                    tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                }
            }
        }

        tags
    }

    /// Build tag specifications for subnet creation
    fn build_tag_specifications(&self, config: &Value) -> TagSpecification {
        let mut tags = Vec::new();

        // Add tags from config
        let user_tags = Self::extract_tags(config);
        for (key, value) in user_tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }

        TagSpecification::builder()
            .resource_type(ResourceType::Subnet)
            .set_tags(Some(tags))
            .build()
    }

    /// Compare tags between current and desired state
    fn compare_tags(
        current: &HashMap<String, String>,
        desired: &HashMap<String, String>,
    ) -> (HashMap<String, String>, Vec<String>) {
        let mut to_add = HashMap::new();
        let mut to_remove = Vec::new();

        // Find tags to add or update
        for (key, value) in desired {
            if current.get(key) != Some(value) {
                to_add.insert(key.clone(), value.clone());
            }
        }

        // Find tags to remove
        for key in current.keys() {
            if !desired.contains_key(key) {
                to_remove.push(key.clone());
            }
        }

        (to_add, to_remove)
    }

    /// Validate CIDR block format
    fn validate_cidr(cidr: &str) -> bool {
        let cidr_regex =
            Regex::new(r"^(\d{1,3}\.){3}\d{1,3}/(\d{1,2})$").expect("Invalid regex pattern");
        if !cidr_regex.is_match(cidr) {
            return false;
        }

        // Validate IP octets and prefix length
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return false;
        }

        // Validate IP address
        let ip_parts: Vec<&str> = parts[0].split('.').collect();
        for part in &ip_parts {
            if let Ok(num) = part.parse::<u32>() {
                if num > 255 {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Validate prefix length (AWS subnets must be between /16 and /28)
        if let Ok(prefix) = parts[1].parse::<u32>() {
            if !(16..=28).contains(&prefix) {
                return false;
            }
        } else {
            return false;
        }

        true
    }

    /// Extract VPC ID reference from configuration
    fn extract_vpc_reference(config: &Value) -> Option<(String, String)> {
        if let Some(vpc_id) = config.get("vpc_id").and_then(|v| v.as_str()) {
            // Check if it's a resource reference pattern: {{ resources.aws_vpc.name.id }}
            let reference_regex = Regex::new(
                r"\{\{\s*resources\.([a-zA-Z_][a-zA-Z0-9_]*)\.([a-zA-Z_][a-zA-Z0-9_]*)\.([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\}",
            ).ok()?;

            if let Some(caps) = reference_regex.captures(vpc_id) {
                let resource_type = caps.get(1)?.as_str().to_string();
                let resource_name = caps.get(2)?.as_str().to_string();
                return Some((resource_type, resource_name));
            }
        }
        None
    }
}

#[async_trait]
impl Resource for AwsSubnetResource {
    fn resource_type(&self) -> &str {
        "aws_subnet"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_subnet".to_string(),
            description: "Provides an AWS VPC subnet resource".to_string(),
            required_args: vec![
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "The VPC ID where the subnet will be created".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "cidr_block".to_string(),
                    field_type: FieldType::String,
                    description: "The IPv4 CIDR block for the subnet".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::CidrBlock],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "availability_zone".to_string(),
                    field_type: FieldType::String,
                    description: "The AZ for the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "availability_zone_id".to_string(),
                    field_type: FieldType::String,
                    description: "The AZ ID of the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "map_public_ip_on_launch".to_string(),
                    field_type: FieldType::Boolean,
                    description:
                        "Whether instances launched in this subnet receive a public IP address"
                            .to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "assign_ipv6_address_on_creation".to_string(),
                    field_type: FieldType::Boolean,
                    description:
                        "Whether network interfaces created in this subnet receive an IPv6 address"
                            .to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ipv6_cidr_block".to_string(),
                    field_type: FieldType::String,
                    description: "The IPv6 CIDR block for the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_dns64".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether DNS queries made to the Amazon-provided DNS Resolver in this subnet should return synthetic IPv6 addresses for IPv4-only destinations".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_resource_name_dns_aaaa_record_on_launch".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Indicates whether to respond to DNS queries for instance hostnames with DNS AAAA records".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_resource_name_dns_a_record_on_launch".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Indicates whether to respond to DNS queries for instance hostnames with DNS A records".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_dns_hostname_type_on_launch".to_string(),
                    field_type: FieldType::String,
                    description: "The type of hostnames to assign to instances in the subnet at launch".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "ip-name".to_string(),
                            "resource-name".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "A map of tags to assign to the resource".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "The ID of the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "owner_id".to_string(),
                    field_type: FieldType::String,
                    description: "The ID of the AWS account that owns the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "available_ip_address_count".to_string(),
                    field_type: FieldType::Integer,
                    description: "The number of available IP addresses in the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ipv6_cidr_block_association_id".to_string(),
                    field_type: FieldType::String,
                    description: "The association ID for the IPv6 CIDR block".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "vpc_id".to_string(),
                "cidr_block".to_string(),
                "availability_zone".to_string(),
                "availability_zone_id".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 600,
                read: 60,
                update: 300,
                delete: 600,
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        let client = self.create_client(ctx).await?;

        let resp = client
            .describe_subnets()
            .subnet_ids(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe subnet: {}", e))
            })?;

        let subnets = resp.subnets();
        if subnets.is_empty() {
            return Ok(ResourceReadResult::not_found());
        }

        let subnet = &subnets[0];
        let subnet_id = subnet.subnet_id().unwrap_or_default();

        // Extract tags
        let mut tags = HashMap::new();
        for tag in subnet.tags() {
            if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                tags.insert(key.to_string(), value.to_string());
            }
        }

        // Extract IPv6 CIDR block association ID
        let ipv6_cidr_block_association_id = subnet
            .ipv6_cidr_block_association_set()
            .first()
            .and_then(|assoc| assoc.association_id())
            .map(|s| s.to_string());

        let attributes = serde_json::json!({
            "id": subnet_id,
            "arn": subnet.subnet_arn().unwrap_or_default(),
            "vpc_id": subnet.vpc_id().unwrap_or_default(),
            "cidr_block": subnet.cidr_block().unwrap_or_default(),
            "availability_zone": subnet.availability_zone().unwrap_or_default(),
            "availability_zone_id": subnet.availability_zone_id().unwrap_or_default(),
            "owner_id": subnet.owner_id().unwrap_or_default(),
            "available_ip_address_count": subnet.available_ip_address_count().unwrap_or(0),
            "map_public_ip_on_launch": subnet.map_public_ip_on_launch().unwrap_or(false),
            "assign_ipv6_address_on_creation": subnet.assign_ipv6_address_on_creation().unwrap_or(false),
            "ipv6_cidr_block": subnet.ipv6_cidr_block_association_set()
                .first()
                .and_then(|assoc| assoc.ipv6_cidr_block())
                .unwrap_or_default(),
            "ipv6_cidr_block_association_id": ipv6_cidr_block_association_id,
            "enable_dns64": subnet.enable_dns64().unwrap_or(false),
            "private_dns_hostname_type_on_launch": subnet
                .private_dns_name_options_on_launch()
                .and_then(|opts| opts.hostname_type())
                .map(|t| t.as_str())
                .unwrap_or("ip-name"),
            "tags": tags,
        });

        Ok(ResourceReadResult::found(subnet_id, attributes))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => {
                // Resource doesn't exist - create it
                Ok(ResourceDiff::create(desired.clone()))
            }
            Some(current_state) => {
                // Check for force_new fields
                let force_new_fields = self.forces_replacement();
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                for field in &force_new_fields {
                    let desired_val = desired.get(field);
                    let current_val = current_state.get(field);

                    if desired_val.is_some() && desired_val != current_val {
                        requires_replacement = true;
                        replacement_fields.push(field.clone());
                    }
                }

                if requires_replacement {
                    return Ok(ResourceDiff {
                        change_type: ChangeType::Replace,
                        additions: HashMap::new(),
                        modifications: HashMap::new(),
                        deletions: Vec::new(),
                        requires_replacement: true,
                        replacement_fields,
                    });
                }

                // Check for in-place modifications
                let mut modifications = HashMap::new();

                // Check map_public_ip_on_launch
                let desired_public_ip = desired
                    .get("map_public_ip_on_launch")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let current_public_ip = current_state
                    .get("map_public_ip_on_launch")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if desired_public_ip != current_public_ip {
                    modifications.insert(
                        "map_public_ip_on_launch".to_string(),
                        (
                            Value::Bool(current_public_ip),
                            Value::Bool(desired_public_ip),
                        ),
                    );
                }

                // Check assign_ipv6_address_on_creation
                let desired_ipv6 = desired
                    .get("assign_ipv6_address_on_creation")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let current_ipv6 = current_state
                    .get("assign_ipv6_address_on_creation")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if desired_ipv6 != current_ipv6 {
                    modifications.insert(
                        "assign_ipv6_address_on_creation".to_string(),
                        (Value::Bool(current_ipv6), Value::Bool(desired_ipv6)),
                    );
                }

                // Check tags
                let desired_tags = Self::extract_tags(desired);
                let current_tags = Self::extract_tags(current_state);

                if desired_tags != current_tags {
                    modifications.insert(
                        "tags".to_string(),
                        (
                            serde_json::to_value(&current_tags).unwrap_or(Value::Null),
                            serde_json::to_value(&desired_tags).unwrap_or(Value::Null),
                        ),
                    );
                }

                if modifications.is_empty() {
                    Ok(ResourceDiff::no_change())
                } else {
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
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Extract required fields
        let vpc_id = config
            .get("vpc_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("vpc_id is required".to_string()))?;

        let cidr_block = config
            .get("cidr_block")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("cidr_block is required".to_string())
            })?;

        // Build the create request
        let mut req = client.create_subnet().vpc_id(vpc_id).cidr_block(cidr_block);

        // Add optional availability zone
        if let Some(az) = config.get("availability_zone").and_then(|v| v.as_str()) {
            req = req.availability_zone(az);
        }

        if let Some(az_id) = config.get("availability_zone_id").and_then(|v| v.as_str()) {
            req = req.availability_zone_id(az_id);
        }

        // Add IPv6 CIDR block if specified
        if let Some(ipv6_cidr) = config.get("ipv6_cidr_block").and_then(|v| v.as_str()) {
            req = req.ipv6_cidr_block(ipv6_cidr);
        }

        // Add tags
        let tag_spec = self.build_tag_specifications(config);
        if !tag_spec.tags().is_empty() {
            req = req.tag_specifications(tag_spec);
        }

        // Execute the create request
        let resp = req
            .send()
            .await
            .map_err(|e| ProvisioningError::ApplyError {
                resource: "aws_subnet".to_string(),
                message: format!("Failed to create subnet: {}", e),
            })?;

        let subnet = resp.subnet().ok_or_else(|| ProvisioningError::ApplyError {
            resource: "aws_subnet".to_string(),
            message: "No subnet returned from create operation".to_string(),
        })?;

        let subnet_id = subnet.subnet_id().unwrap_or_default().to_string();

        // Modify subnet attributes after creation
        let map_public_ip = config
            .get("map_public_ip_on_launch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if map_public_ip {
            client
                .modify_subnet_attribute()
                .subnet_id(&subnet_id)
                .map_public_ip_on_launch(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(true)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", subnet_id),
                    message: format!("Failed to set map_public_ip_on_launch: {}", e),
                })?;
        }

        let assign_ipv6 = config
            .get("assign_ipv6_address_on_creation")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if assign_ipv6 {
            client
                .modify_subnet_attribute()
                .subnet_id(&subnet_id)
                .assign_ipv6_address_on_creation(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(true)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", subnet_id),
                    message: format!("Failed to set assign_ipv6_address_on_creation: {}", e),
                })?;
        }

        // Enable DNS64 if specified
        let enable_dns64 = config
            .get("enable_dns64")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if enable_dns64 {
            client
                .modify_subnet_attribute()
                .subnet_id(&subnet_id)
                .enable_dns64(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(true)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", subnet_id),
                    message: format!("Failed to enable DNS64: {}", e),
                })?;
        }

        // Read back the final state
        let read_result = self.read(&subnet_id, ctx).await?;

        tracing::info!(
            "Created subnet {} in VPC {} with CIDR {}",
            subnet_id,
            vpc_id,
            cidr_block
        );

        Ok(
            ResourceResult::success(subnet_id.clone(), read_result.attributes)
                .with_output("id", Value::String(subnet_id.clone()))
                .with_output(
                    "arn",
                    Value::String(subnet.subnet_arn().unwrap_or_default().to_string()),
                ),
        )
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Update map_public_ip_on_launch if changed
        if let Some(map_public_ip) = new.get("map_public_ip_on_launch").and_then(|v| v.as_bool()) {
            client
                .modify_subnet_attribute()
                .subnet_id(id)
                .map_public_ip_on_launch(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(map_public_ip)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", id),
                    message: format!("Failed to update map_public_ip_on_launch: {}", e),
                })?;
        }

        // Update assign_ipv6_address_on_creation if changed
        if let Some(assign_ipv6) = new
            .get("assign_ipv6_address_on_creation")
            .and_then(|v| v.as_bool())
        {
            client
                .modify_subnet_attribute()
                .subnet_id(id)
                .assign_ipv6_address_on_creation(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(assign_ipv6)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", id),
                    message: format!("Failed to update assign_ipv6_address_on_creation: {}", e),
                })?;
        }

        // Update tags
        let read_result = self.read(id, ctx).await?;
        let current_tags = Self::extract_tags(&read_result.attributes);
        let desired_tags = Self::extract_tags(new);
        let (tags_to_add, tags_to_remove) = Self::compare_tags(&current_tags, &desired_tags);

        // Remove obsolete tags
        if !tags_to_remove.is_empty() {
            let mut delete_req = client.delete_tags().resources(id);
            for key in &tags_to_remove {
                delete_req = delete_req.tags(Tag::builder().key(key).build());
            }
            delete_req
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", id),
                    message: format!("Failed to delete tags: {}", e),
                })?;
        }

        // Add or update tags
        if !tags_to_add.is_empty() {
            let mut create_req = client.create_tags().resources(id);
            for (key, value) in &tags_to_add {
                create_req = create_req.tags(Tag::builder().key(key).value(value).build());
            }
            create_req
                .send()
                .await
                .map_err(|e| ProvisioningError::ApplyError {
                    resource: format!("aws_subnet.{}", id),
                    message: format!("Failed to create tags: {}", e),
                })?;
        }

        // Read back the final state
        let final_result = self.read(id, ctx).await?;

        tracing::info!("Updated subnet {}", id);

        Ok(ResourceResult::success(id, final_result.attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        client
            .delete_subnet()
            .subnet_id(id)
            .send()
            .await
            .map_err(|e| ProvisioningError::DestroyError {
                resource: format!("aws_subnet.{}", id),
                message: format!("Failed to delete subnet: {}", e),
            })?;

        tracing::info!("Deleted subnet {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;

        if !read_result.exists {
            return Err(ProvisioningError::ImportError {
                resource_type: "aws_subnet".to_string(),
                resource_id: id.to_string(),
                message: "Subnet not found".to_string(),
            });
        }

        tracing::info!("Imported subnet {}", id);

        Ok(ResourceResult::success(
            read_result.cloud_id.unwrap_or_else(|| id.to_string()),
            read_result.attributes,
        ))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Extract VPC dependency
        if let Some((resource_type, resource_name)) = Self::extract_vpc_reference(config) {
            deps.push(ResourceDependency::new(resource_type, resource_name, "id"));
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "vpc_id".to_string(),
            "cidr_block".to_string(),
            "availability_zone".to_string(),
            "availability_zone_id".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate vpc_id is present
        if config.get("vpc_id").is_none() {
            return Err(ProvisioningError::ValidationError(
                "vpc_id is required for aws_subnet".to_string(),
            ));
        }

        // Validate cidr_block is present and valid
        let cidr = config
            .get("cidr_block")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError(
                    "cidr_block is required for aws_subnet".to_string(),
                )
            })?;

        if !Self::validate_cidr(cidr) {
            return Err(ProvisioningError::ValidationError(format!(
                "Invalid CIDR block '{}'. Must be a valid IPv4 CIDR with prefix length between /16 and /28",
                cidr
            )));
        }

        // Validate availability_zone and availability_zone_id are not both specified
        if config.get("availability_zone").is_some() && config.get("availability_zone_id").is_some()
        {
            return Err(ProvisioningError::ValidationError(
                "Cannot specify both availability_zone and availability_zone_id".to_string(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Subnet Info Struct for Serialization
// ============================================================================

/// Subnet information returned from AWS API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetInfo {
    /// Subnet ID
    pub id: String,
    /// Subnet ARN
    pub arn: String,
    /// VPC ID
    pub vpc_id: String,
    /// CIDR block
    pub cidr_block: String,
    /// Availability zone
    pub availability_zone: String,
    /// Availability zone ID
    pub availability_zone_id: String,
    /// Owner ID
    pub owner_id: String,
    /// Available IP address count
    pub available_ip_address_count: i32,
    /// Whether instances get public IPs
    pub map_public_ip_on_launch: bool,
    /// Whether network interfaces get IPv6 addresses
    pub assign_ipv6_address_on_creation: bool,
    /// IPv6 CIDR block
    pub ipv6_cidr_block: Option<String>,
    /// IPv6 CIDR block association ID
    pub ipv6_cidr_block_association_id: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_type() {
        let resource = AwsSubnetResource::new();
        assert_eq!(resource.resource_type(), "aws_subnet");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsSubnetResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_subnet");
        assert_eq!(schema.required_args.len(), 2);
        assert_eq!(schema.required_args[0].name, "vpc_id");
        assert_eq!(schema.required_args[1].name, "cidr_block");

        // Check computed attributes
        let computed_names: Vec<_> = schema.computed_attrs.iter().map(|f| &f.name).collect();
        assert!(computed_names.contains(&&"id".to_string()));
        assert!(computed_names.contains(&&"arn".to_string()));
        assert!(computed_names.contains(&&"owner_id".to_string()));
        assert!(computed_names.contains(&&"available_ip_address_count".to_string()));
        assert!(computed_names.contains(&&"ipv6_cidr_block_association_id".to_string()));

        // Check force_new fields
        assert!(schema.force_new.contains(&"vpc_id".to_string()));
        assert!(schema.force_new.contains(&"cidr_block".to_string()));
        assert!(schema.force_new.contains(&"availability_zone".to_string()));
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsSubnetResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"vpc_id".to_string()));
        assert!(force_new.contains(&"cidr_block".to_string()));
        assert!(force_new.contains(&"availability_zone".to_string()));
        assert!(!force_new.contains(&"map_public_ip_on_launch".to_string()));
    }

    #[test]
    fn test_validate_cidr_valid() {
        assert!(AwsSubnetResource::validate_cidr("10.0.0.0/16"));
        assert!(AwsSubnetResource::validate_cidr("10.0.1.0/24"));
        assert!(AwsSubnetResource::validate_cidr("192.168.0.0/28"));
        assert!(AwsSubnetResource::validate_cidr("172.16.0.0/20"));
    }

    #[test]
    fn test_validate_cidr_invalid() {
        // Invalid format
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0"));
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0/"));
        assert!(!AwsSubnetResource::validate_cidr("not-a-cidr"));

        // Invalid IP
        assert!(!AwsSubnetResource::validate_cidr("256.0.0.0/24"));
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.256/24"));

        // Invalid prefix (AWS subnets must be /16 to /28)
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0/8"));
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0/15"));
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0/29"));
        assert!(!AwsSubnetResource::validate_cidr("10.0.0.0/32"));
    }

    #[test]
    fn test_validate_config_valid() {
        let resource = AwsSubnetResource::new();
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "availability_zone": "us-east-1a",
            "map_public_ip_on_launch": true,
            "tags": {
                "Name": "test-subnet"
            }
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_vpc_id() {
        let resource = AwsSubnetResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.1.0/24"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ProvisioningError::ValidationError(_)
        ));
    }

    #[test]
    fn test_validate_config_missing_cidr_block() {
        let resource = AwsSubnetResource::new();
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_invalid_cidr() {
        let resource = AwsSubnetResource::new();
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "invalid-cidr"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_both_az_and_az_id() {
        let resource = AwsSubnetResource::new();
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "availability_zone": "us-east-1a",
            "availability_zone_id": "use1-az1"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_tags() {
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "tags": {
                "Name": "test-subnet",
                "Environment": "production",
                "Team": "infrastructure"
            }
        });

        let tags = AwsSubnetResource::extract_tags(&config);
        assert_eq!(tags.get("Name"), Some(&"test-subnet".to_string()));
        assert_eq!(tags.get("Environment"), Some(&"production".to_string()));
        assert_eq!(tags.get("Team"), Some(&"infrastructure".to_string()));
    }

    #[test]
    fn test_extract_tags_empty() {
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24"
        });

        let tags = AwsSubnetResource::extract_tags(&config);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_compare_tags() {
        let current = HashMap::from([
            ("Name".to_string(), "old-name".to_string()),
            ("Environment".to_string(), "staging".to_string()),
            ("ToRemove".to_string(), "value".to_string()),
        ]);

        let desired = HashMap::from([
            ("Name".to_string(), "new-name".to_string()),
            ("Environment".to_string(), "staging".to_string()),
            ("NewTag".to_string(), "new-value".to_string()),
        ]);

        let (to_add, to_remove) = AwsSubnetResource::compare_tags(&current, &desired);

        // Name should be updated, NewTag should be added
        assert!(to_add.contains_key("Name"));
        assert!(to_add.contains_key("NewTag"));
        assert!(!to_add.contains_key("Environment")); // Unchanged

        // ToRemove should be removed
        assert!(to_remove.contains(&"ToRemove".to_string()));
        assert!(!to_remove.contains(&"Environment".to_string()));
    }

    #[test]
    fn test_extract_vpc_reference() {
        // Test with valid reference
        let config = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "cidr_block": "10.0.1.0/24"
        });

        let result = AwsSubnetResource::extract_vpc_reference(&config);
        assert!(result.is_some());
        let (resource_type, resource_name) = result.unwrap();
        assert_eq!(resource_type, "aws_vpc");
        assert_eq!(resource_name, "main");
    }

    #[test]
    fn test_extract_vpc_reference_literal() {
        // Test with literal VPC ID (no reference)
        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24"
        });

        let result = AwsSubnetResource::extract_vpc_reference(&config);
        assert!(result.is_none());
    }

    #[test]
    fn test_dependencies() {
        let resource = AwsSubnetResource::new();

        // With reference
        let config = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.production.id }}",
            "cidr_block": "10.0.1.0/24"
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].resource_type, "aws_vpc");
        assert_eq!(deps[0].resource_name, "production");
        assert_eq!(deps[0].attribute, "id");
    }

    #[test]
    fn test_dependencies_no_reference() {
        let resource = AwsSubnetResource::new();

        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24"
        });

        let deps = resource.dependencies(&config);
        assert!(deps.is_empty());
    }

    #[tokio::test]
    async fn test_plan_create() {
        use crate::provisioning::traits::{DebugCredentials, RetryConfig};
        use std::sync::Arc;

        let resource = AwsSubnetResource::new();

        let ctx = ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: Value::Null,
            credentials: Arc::new(DebugCredentials::new("test")),
            timeout_seconds: 60,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        };

        let desired = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "availability_zone": "us-east-1a"
        });

        let diff = resource.plan(&desired, None, &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
        assert!(!diff.additions.is_empty());
    }

    #[tokio::test]
    async fn test_plan_no_change() {
        use crate::provisioning::traits::{DebugCredentials, RetryConfig};
        use std::sync::Arc;

        let resource = AwsSubnetResource::new();

        let ctx = ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: Value::Null,
            credentials: Arc::new(DebugCredentials::new("test")),
            timeout_seconds: 60,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        };

        let config = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "map_public_ip_on_launch": true
        });

        let current = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "map_public_ip_on_launch": true
        });

        let diff = resource.plan(&config, Some(&current), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[tokio::test]
    async fn test_plan_update() {
        use crate::provisioning::traits::{DebugCredentials, RetryConfig};
        use std::sync::Arc;

        let resource = AwsSubnetResource::new();

        let ctx = ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: Value::Null,
            credentials: Arc::new(DebugCredentials::new("test")),
            timeout_seconds: 60,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        };

        let desired = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "map_public_ip_on_launch": true
        });

        let current = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24",
            "map_public_ip_on_launch": false
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Update);
        assert!(diff.modifications.contains_key("map_public_ip_on_launch"));
    }

    #[tokio::test]
    async fn test_plan_replace() {
        use crate::provisioning::traits::{DebugCredentials, RetryConfig};
        use std::sync::Arc;

        let resource = AwsSubnetResource::new();

        let ctx = ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: Value::Null,
            credentials: Arc::new(DebugCredentials::new("test")),
            timeout_seconds: 60,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        };

        // Changing CIDR block requires replacement
        let desired = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.2.0/24"
        });

        let current = serde_json::json!({
            "vpc_id": "vpc-12345678",
            "cidr_block": "10.0.1.0/24"
        });

        let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
        assert!(diff.replacement_fields.contains(&"cidr_block".to_string()));
    }

    #[test]
    fn test_default() {
        let resource = AwsSubnetResource::default();
        assert_eq!(resource.resource_type(), "aws_subnet");
    }
}
