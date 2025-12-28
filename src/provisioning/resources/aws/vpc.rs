//! AWS VPC Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS VPCs, enabling declarative
//! VPC management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_vpc:
//!     production:
//!       cidr_block: "10.0.0.0/16"
//!       enable_dns_support: true
//!       enable_dns_hostnames: true
//!       instance_tenancy: "default"
//!       tags:
//!         Name: production-vpc
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
use aws_sdk_ec2::types::{Filter, ResourceType, Tag, TagSpecification, Tenancy};
#[cfg(feature = "aws")]
use aws_sdk_ec2::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// VPC Resource Configuration
// ============================================================================

/// VPC tenancy options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VpcTenancy {
    /// Instances run on shared hardware (default)
    #[default]
    Default,
    /// Instances run on single-tenant hardware
    Dedicated,
    /// Instances run on Dedicated Hosts
    Host,
}

impl VpcTenancy {
    /// Parse tenancy from string
    pub fn from_str(s: &str) -> ProvisioningResult<Self> {
        match s.to_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "dedicated" => Ok(Self::Dedicated),
            "host" => Ok(Self::Host),
            _ => Err(ProvisioningError::ValidationError(format!(
                "Invalid instance_tenancy '{}'. Valid values: default, dedicated, host",
                s
            ))),
        }
    }

    /// Convert to AWS SDK tenancy type
    #[cfg(feature = "aws")]
    pub fn to_aws_tenancy(&self) -> Tenancy {
        match self {
            Self::Default => Tenancy::Default,
            Self::Dedicated => Tenancy::Dedicated,
            Self::Host => Tenancy::Host,
        }
    }

    /// Get as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Dedicated => "dedicated",
            Self::Host => "host",
        }
    }
}

/// VPC resource attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcAttributes {
    /// VPC ID (e.g., vpc-12345678)
    pub id: String,
    /// VPC ARN
    pub arn: String,
    /// CIDR block
    pub cidr_block: String,
    /// Main route table ID
    pub main_route_table_id: Option<String>,
    /// Default network ACL ID
    pub default_network_acl_id: Option<String>,
    /// Default security group ID
    pub default_security_group_id: Option<String>,
    /// Default route table ID
    pub default_route_table_id: Option<String>,
    /// Owner account ID
    pub owner_id: String,
    /// Whether DNS support is enabled
    pub enable_dns_support: bool,
    /// Whether DNS hostnames are enabled
    pub enable_dns_hostnames: bool,
    /// Instance tenancy
    pub instance_tenancy: String,
    /// VPC state (available, pending)
    pub state: String,
    /// Whether this is the default VPC
    pub is_default: bool,
    /// Tags
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS VPC Resource Implementation
// ============================================================================

/// AWS VPC resource for infrastructure provisioning
///
/// This resource manages Amazon Virtual Private Clouds (VPCs), which provide
/// isolated virtual networks for your AWS resources.
#[derive(Debug, Clone, Default)]
pub struct AwsVpcResource;

impl AwsVpcResource {
    /// Create a new VPC resource
    pub fn new() -> Self {
        Self
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_vpc".to_string(),
            description: "Amazon Virtual Private Cloud (VPC) resource".to_string(),
            required_args: vec![SchemaField {
                name: "cidr_block".to_string(),
                field_type: FieldType::String,
                description: "The IPv4 CIDR block for the VPC (e.g., 10.0.0.0/16)".to_string(),
                default: None,
                constraints: vec![FieldConstraint::CidrBlock],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "enable_dns_support".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable DNS support in the VPC (default: true)".to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_dns_hostnames".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable DNS hostnames in the VPC (default: false)".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance_tenancy".to_string(),
                    field_type: FieldType::String,
                    description: "Instance tenancy: default, dedicated, or host (default: default)"
                        .to_string(),
                    default: Some(Value::String("default".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "default".to_string(),
                            "dedicated".to_string(),
                            "host".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the VPC".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ARN".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "main_route_table_id".to_string(),
                    field_type: FieldType::String,
                    description: "Main route table ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_network_acl_id".to_string(),
                    field_type: FieldType::String,
                    description: "Default network ACL ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_security_group_id".to_string(),
                    field_type: FieldType::String,
                    description: "Default security group ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_route_table_id".to_string(),
                    field_type: FieldType::String,
                    description: "Default route table ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "owner_id".to_string(),
                    field_type: FieldType::String,
                    description: "Owner account ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["cidr_block".to_string(), "instance_tenancy".to_string()],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
                delete: 600, // VPC deletion can take time if dependencies exist
            },
        }
    }

    /// Validate CIDR block format
    fn validate_cidr(cidr: &str) -> ProvisioningResult<()> {
        // Basic CIDR validation: X.X.X.X/N format
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return Err(ProvisioningError::ValidationError(format!(
                "Invalid CIDR block '{}': must be in format X.X.X.X/N",
                cidr
            )));
        }

        // Validate IP part
        let ip_parts: Vec<&str> = parts[0].split('.').collect();
        if ip_parts.len() != 4 {
            return Err(ProvisioningError::ValidationError(format!(
                "Invalid CIDR block '{}': IP address must have 4 octets",
                cidr
            )));
        }

        for part in &ip_parts {
            match part.parse::<u8>() {
                Ok(_) => {}
                Err(_) => {
                    return Err(ProvisioningError::ValidationError(format!(
                        "Invalid CIDR block '{}': each octet must be 0-255",
                        cidr
                    )));
                }
            }
        }

        // Validate prefix length
        match parts[1].parse::<u8>() {
            Ok(prefix) if prefix <= 32 => {
                // AWS VPC CIDR must be between /16 and /28
                if prefix < 16 || prefix > 28 {
                    return Err(ProvisioningError::ValidationError(format!(
                        "Invalid CIDR block '{}': VPC CIDR prefix must be between /16 and /28",
                        cidr
                    )));
                }
            }
            _ => {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid CIDR block '{}': prefix must be 0-32",
                    cidr
                )));
            }
        }

        Ok(())
    }

    /// Extract configuration values from JSON
    fn extract_config(config: &Value) -> ProvisioningResult<VpcConfig> {
        let cidr_block = config
            .get("cidr_block")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("cidr_block is required".to_string())
            })?
            .to_string();

        let enable_dns_support = config
            .get("enable_dns_support")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let enable_dns_hostnames = config
            .get("enable_dns_hostnames")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let instance_tenancy = config
            .get("instance_tenancy")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();

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

        Ok(VpcConfig {
            cidr_block,
            enable_dns_support,
            enable_dns_hostnames,
            instance_tenancy,
            tags,
        })
    }

    /// Create AWS EC2 client
    #[cfg(feature = "aws")]
    async fn create_client(ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_ec2::config::Region::new(region.clone()))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Read VPC by ID from AWS
    #[cfg(feature = "aws")]
    async fn read_vpc_by_id(
        client: &Client,
        vpc_id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<VpcAttributes>> {
        let resp = client
            .describe_vpcs()
            .vpc_ids(vpc_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe VPC: {}", e))
            })?;

        if let Some(vpc) = resp.vpcs().first() {
            let vpc_id = vpc.vpc_id().unwrap_or_default().to_string();

            // Get DNS attributes
            let dns_support = client
                .describe_vpc_attribute()
                .vpc_id(&vpc_id)
                .attribute(aws_sdk_ec2::types::VpcAttributeName::EnableDnsSupport)
                .send()
                .await
                .ok()
                .and_then(|r| r.enable_dns_support().and_then(|a| a.value()))
                .unwrap_or(true);

            let dns_hostnames = client
                .describe_vpc_attribute()
                .vpc_id(&vpc_id)
                .attribute(aws_sdk_ec2::types::VpcAttributeName::EnableDnsHostnames)
                .send()
                .await
                .ok()
                .and_then(|r| r.enable_dns_hostnames().and_then(|a| a.value()))
                .unwrap_or(false);

            // Get associated resources
            let route_tables = client
                .describe_route_tables()
                .filters(Filter::builder().name("vpc-id").values(&vpc_id).build())
                .send()
                .await
                .ok();

            let main_route_table_id = route_tables.as_ref().and_then(|rt| {
                rt.route_tables().iter().find_map(|table| {
                    if table.associations().iter().any(|a| a.main() == Some(true)) {
                        table.route_table_id().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            });

            let default_route_table_id = route_tables.as_ref().and_then(|rt| {
                rt.route_tables()
                    .first()
                    .and_then(|t| t.route_table_id().map(|s| s.to_string()))
            });

            // Get default network ACL
            let network_acls = client
                .describe_network_acls()
                .filters(Filter::builder().name("vpc-id").values(&vpc_id).build())
                .filters(Filter::builder().name("default").values("true").build())
                .send()
                .await
                .ok();

            let default_network_acl_id = network_acls.and_then(|acls| {
                acls.network_acls()
                    .first()
                    .and_then(|a| a.network_acl_id().map(|s| s.to_string()))
            });

            // Get default security group
            let security_groups = client
                .describe_security_groups()
                .filters(Filter::builder().name("vpc-id").values(&vpc_id).build())
                .filters(
                    Filter::builder()
                        .name("group-name")
                        .values("default")
                        .build(),
                )
                .send()
                .await
                .ok();

            let default_security_group_id = security_groups.and_then(|sgs| {
                sgs.security_groups()
                    .first()
                    .and_then(|sg| sg.group_id().map(|s| s.to_string()))
            });

            // Extract tags
            let mut tags = HashMap::new();
            for tag in vpc.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            // Build ARN
            let region = ctx.region.as_deref().unwrap_or("us-east-1");
            let owner_id = vpc.owner_id().unwrap_or_default();
            let arn = format!("arn:aws:ec2:{}:{}:vpc/{}", region, owner_id, vpc_id);

            Ok(Some(VpcAttributes {
                id: vpc_id,
                arn,
                cidr_block: vpc.cidr_block().unwrap_or_default().to_string(),
                main_route_table_id,
                default_network_acl_id,
                default_security_group_id,
                default_route_table_id,
                owner_id: owner_id.to_string(),
                enable_dns_support: dns_support,
                enable_dns_hostnames: dns_hostnames,
                instance_tenancy: vpc
                    .instance_tenancy()
                    .map(|t| t.as_str().to_string())
                    .unwrap_or_else(|| "default".to_string()),
                state: vpc
                    .state()
                    .map(|s| s.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                is_default: vpc.is_default().unwrap_or(false),
                tags,
            }))
        } else {
            Ok(None)
        }
    }

    /// Create VPC in AWS
    #[cfg(feature = "aws")]
    async fn create_vpc(
        client: &Client,
        config: &VpcConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<VpcAttributes> {
        let tenancy = VpcTenancy::from_str(&config.instance_tenancy)?;

        // Build tags including default tags from context
        let mut all_tags: Vec<Tag> = ctx
            .default_tags
            .iter()
            .map(|(k, v)| Tag::builder().key(k).value(v).build())
            .collect();

        for (k, v) in &config.tags {
            all_tags.push(Tag::builder().key(k).value(v).build());
        }

        let resp = client
            .create_vpc()
            .cidr_block(&config.cidr_block)
            .instance_tenancy(tenancy.to_aws_tenancy())
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::Vpc)
                    .set_tags(Some(all_tags))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to create VPC: {}", e))
            })?;

        let vpc = resp.vpc().ok_or_else(|| {
            ProvisioningError::CloudApiError("No VPC returned from create".to_string())
        })?;

        let vpc_id = vpc.vpc_id().unwrap_or_default().to_string();

        // Wait for VPC to be available
        Self::wait_for_vpc_available(client, &vpc_id).await?;

        // Configure DNS settings
        if config.enable_dns_support {
            client
                .modify_vpc_attribute()
                .vpc_id(&vpc_id)
                .enable_dns_support(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(true)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to enable DNS support: {}", e))
                })?;
        }

        if config.enable_dns_hostnames {
            client
                .modify_vpc_attribute()
                .vpc_id(&vpc_id)
                .enable_dns_hostnames(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(true)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to enable DNS hostnames: {}",
                        e
                    ))
                })?;
        }

        // Read the full VPC attributes
        Self::read_vpc_by_id(client, &vpc_id, ctx)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read created VPC".to_string())
            })
    }

    /// Wait for VPC to be available
    #[cfg(feature = "aws")]
    async fn wait_for_vpc_available(client: &Client, vpc_id: &str) -> ProvisioningResult<()> {
        use std::time::Duration;

        let max_attempts = 20;
        let delay = Duration::from_secs(3);

        for _ in 0..max_attempts {
            let resp = client
                .describe_vpcs()
                .vpc_ids(vpc_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to describe VPC: {}", e))
                })?;

            if let Some(vpc) = resp.vpcs().first() {
                if let Some(state) = vpc.state() {
                    if state.as_str() == "available" {
                        return Ok(());
                    }
                }
            }

            tokio::time::sleep(delay).await;
        }

        Err(ProvisioningError::Timeout {
            operation: format!("Waiting for VPC {} to be available", vpc_id),
            seconds: (max_attempts * 3) as u64,
        })
    }

    /// Update VPC in AWS (DNS settings and tags only - CIDR and tenancy force replacement)
    #[cfg(feature = "aws")]
    async fn update_vpc(
        client: &Client,
        vpc_id: &str,
        old_config: &VpcConfig,
        new_config: &VpcConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<VpcAttributes> {
        // Update DNS support if changed
        if old_config.enable_dns_support != new_config.enable_dns_support {
            client
                .modify_vpc_attribute()
                .vpc_id(vpc_id)
                .enable_dns_support(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(new_config.enable_dns_support)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to modify DNS support: {}", e))
                })?;
        }

        // Update DNS hostnames if changed
        if old_config.enable_dns_hostnames != new_config.enable_dns_hostnames {
            client
                .modify_vpc_attribute()
                .vpc_id(vpc_id)
                .enable_dns_hostnames(
                    aws_sdk_ec2::types::AttributeBooleanValue::builder()
                        .value(new_config.enable_dns_hostnames)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to modify DNS hostnames: {}",
                        e
                    ))
                })?;
        }

        // Update tags if changed
        if old_config.tags != new_config.tags {
            // Delete old tags that are no longer present
            let tags_to_delete: Vec<_> = old_config
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k))
                .map(|k| Tag::builder().key(k).build())
                .collect();

            if !tags_to_delete.is_empty() {
                client
                    .delete_tags()
                    .resources(vpc_id)
                    .set_tags(Some(tags_to_delete))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to delete tags: {}", e))
                    })?;
            }

            // Create/update tags
            let tags_to_create: Vec<_> = new_config
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            if !tags_to_create.is_empty() {
                client
                    .create_tags()
                    .resources(vpc_id)
                    .set_tags(Some(tags_to_create))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to create tags: {}", e))
                    })?;
            }
        }

        // Read updated VPC
        Self::read_vpc_by_id(client, vpc_id, ctx)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read updated VPC".to_string())
            })
    }

    /// Delete VPC in AWS
    #[cfg(feature = "aws")]
    async fn delete_vpc(client: &Client, vpc_id: &str) -> ProvisioningResult<()> {
        client
            .delete_vpc()
            .vpc_id(vpc_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete VPC: {}", e))
            })?;

        Ok(())
    }

    /// Compute diff between desired and current state
    fn compute_diff(
        desired: &Value,
        current: Option<&Value>,
        force_new_fields: &[String],
    ) -> ProvisioningResult<ResourceDiff> {
        // No current state means create
        if current.is_none() {
            return Ok(ResourceDiff::create(desired.clone()));
        }

        let current = current.unwrap();

        let mut modifications = HashMap::new();
        let mut additions = HashMap::new();
        let mut deletions = Vec::new();
        let mut replacement_fields = Vec::new();

        // Compare fields
        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current.as_object()) {
            // Check for additions and modifications
            for (key, desired_val) in desired_obj {
                // Skip computed fields
                if ["id", "arn", "owner_id", "state", "is_default"].contains(&key.as_str()) {
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
                } else {
                    additions.insert(key.clone(), desired_val.clone());
                }
            }

            // Check for deletions (fields in current but not in desired)
            for key in current_obj.keys() {
                // Skip computed fields
                if ["id", "arn", "owner_id", "state", "is_default"].contains(&key.as_str()) {
                    continue;
                }

                if !desired_obj.contains_key(key) {
                    deletions.push(key.clone());
                }
            }
        }

        // Determine change type
        let requires_replacement = !replacement_fields.is_empty();
        let has_changes =
            !additions.is_empty() || !modifications.is_empty() || !deletions.is_empty();

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
}

/// Internal VPC configuration structure
#[derive(Debug, Clone)]
struct VpcConfig {
    cidr_block: String,
    enable_dns_support: bool,
    enable_dns_hostnames: bool,
    instance_tenancy: String,
    tags: HashMap<String, String>,
}

#[async_trait]
impl Resource for AwsVpcResource {
    fn resource_type(&self) -> &str {
        "aws_vpc"
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

        match Self::read_vpc_by_id(&client, id, ctx).await? {
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
        let vpc_config = Self::extract_config(config)?;
        let client = Self::create_client(ctx).await?;

        match Self::create_vpc(&client, &vpc_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.id, attributes);
                result
                    .outputs
                    .insert("id".to_string(), Value::String(attrs.id.clone()));
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

        match Self::update_vpc(&client, id, &old_config, &new_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
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

        match Self::delete_vpc(&client, id).await {
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

        match Self::read_vpc_by_id(&client, id, ctx).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
            }
            None => Err(ProvisioningError::ImportError {
                resource_type: "aws_vpc".to_string(),
                resource_id: id.to_string(),
                message: "VPC not found".to_string(),
            }),
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

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        // VPC has no dependencies - it's a foundational resource
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["cidr_block".to_string(), "instance_tenancy".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate required cidr_block
        let cidr_block = config
            .get("cidr_block")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("cidr_block is required".to_string())
            })?;

        Self::validate_cidr(cidr_block)?;

        // Validate instance_tenancy if provided
        if let Some(tenancy) = config.get("instance_tenancy").and_then(|v| v.as_str()) {
            VpcTenancy::from_str(tenancy)?;
        }

        // Validate enable_dns_support is boolean if provided
        if let Some(dns_support) = config.get("enable_dns_support") {
            if !dns_support.is_boolean() {
                return Err(ProvisioningError::ValidationError(
                    "enable_dns_support must be a boolean".to_string(),
                ));
            }
        }

        // Validate enable_dns_hostnames is boolean if provided
        if let Some(dns_hostnames) = config.get("enable_dns_hostnames") {
            if !dns_hostnames.is_boolean() {
                return Err(ProvisioningError::ValidationError(
                    "enable_dns_hostnames must be a boolean".to_string(),
                ));
            }
        }

        // Validate tags is an object if provided
        if let Some(tags) = config.get("tags") {
            if !tags.is_object() && !tags.is_null() {
                return Err(ProvisioningError::ValidationError(
                    "tags must be an object".to_string(),
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
    fn test_vpc_tenancy_from_str() {
        assert_eq!(
            VpcTenancy::from_str("default").unwrap(),
            VpcTenancy::Default
        );
        assert_eq!(
            VpcTenancy::from_str("dedicated").unwrap(),
            VpcTenancy::Dedicated
        );
        assert_eq!(VpcTenancy::from_str("host").unwrap(), VpcTenancy::Host);
        assert_eq!(
            VpcTenancy::from_str("DEFAULT").unwrap(),
            VpcTenancy::Default
        );
        assert!(VpcTenancy::from_str("invalid").is_err());
    }

    #[test]
    fn test_vpc_tenancy_as_str() {
        assert_eq!(VpcTenancy::Default.as_str(), "default");
        assert_eq!(VpcTenancy::Dedicated.as_str(), "dedicated");
        assert_eq!(VpcTenancy::Host.as_str(), "host");
    }

    #[test]
    fn test_resource_type() {
        let resource = AwsVpcResource::new();
        assert_eq!(resource.resource_type(), "aws_vpc");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsVpcResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_vpc");
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "cidr_block");
        assert_eq!(schema.optional_args.len(), 4);
        assert_eq!(schema.computed_attrs.len(), 7);
        assert_eq!(schema.force_new, vec!["cidr_block", "instance_tenancy"]);
    }

    #[test]
    fn test_schema_required_args() {
        let resource = AwsVpcResource::new();
        let schema = resource.schema();

        let cidr_field = &schema.required_args[0];
        assert_eq!(cidr_field.name, "cidr_block");
        assert_eq!(cidr_field.field_type, FieldType::String);
        assert!(cidr_field.constraints.contains(&FieldConstraint::CidrBlock));
        assert!(!cidr_field.sensitive);
    }

    #[test]
    fn test_schema_optional_args() {
        let resource = AwsVpcResource::new();
        let schema = resource.schema();

        let field_names: Vec<_> = schema
            .optional_args
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(field_names.contains(&"enable_dns_support"));
        assert!(field_names.contains(&"enable_dns_hostnames"));
        assert!(field_names.contains(&"instance_tenancy"));
        assert!(field_names.contains(&"tags"));

        // Check defaults
        let dns_support = schema
            .optional_args
            .iter()
            .find(|f| f.name == "enable_dns_support")
            .unwrap();
        assert_eq!(dns_support.default, Some(Value::Bool(true)));

        let dns_hostnames = schema
            .optional_args
            .iter()
            .find(|f| f.name == "enable_dns_hostnames")
            .unwrap();
        assert_eq!(dns_hostnames.default, Some(Value::Bool(false)));

        let tenancy = schema
            .optional_args
            .iter()
            .find(|f| f.name == "instance_tenancy")
            .unwrap();
        assert_eq!(tenancy.default, Some(Value::String("default".to_string())));
    }

    #[test]
    fn test_schema_computed_attrs() {
        let resource = AwsVpcResource::new();
        let schema = resource.schema();

        let attr_names: Vec<_> = schema
            .computed_attrs
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(attr_names.contains(&"id"));
        assert!(attr_names.contains(&"arn"));
        assert!(attr_names.contains(&"main_route_table_id"));
        assert!(attr_names.contains(&"default_network_acl_id"));
        assert!(attr_names.contains(&"default_security_group_id"));
        assert!(attr_names.contains(&"default_route_table_id"));
        assert!(attr_names.contains(&"owner_id"));
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsVpcResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"cidr_block".to_string()));
        assert!(force_new.contains(&"instance_tenancy".to_string()));
        assert_eq!(force_new.len(), 2);
    }

    #[test]
    fn test_dependencies_empty() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16"
        });
        let deps = resource.dependencies(&config);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_validate_cidr_valid() {
        assert!(AwsVpcResource::validate_cidr("10.0.0.0/16").is_ok());
        assert!(AwsVpcResource::validate_cidr("192.168.0.0/24").is_ok());
        assert!(AwsVpcResource::validate_cidr("172.16.0.0/28").is_ok());
    }

    #[test]
    fn test_validate_cidr_invalid_format() {
        assert!(AwsVpcResource::validate_cidr("10.0.0.0").is_err());
        assert!(AwsVpcResource::validate_cidr("10.0.0.0/").is_err());
        assert!(AwsVpcResource::validate_cidr("/16").is_err());
        assert!(AwsVpcResource::validate_cidr("invalid").is_err());
    }

    #[test]
    fn test_validate_cidr_invalid_ip() {
        assert!(AwsVpcResource::validate_cidr("256.0.0.0/16").is_err());
        assert!(AwsVpcResource::validate_cidr("10.0.0/16").is_err());
        assert!(AwsVpcResource::validate_cidr("10.0.0.0.0/16").is_err());
    }

    #[test]
    fn test_validate_cidr_invalid_prefix() {
        assert!(AwsVpcResource::validate_cidr("10.0.0.0/33").is_err());
        assert!(AwsVpcResource::validate_cidr("10.0.0.0/15").is_err()); // Too large for VPC
        assert!(AwsVpcResource::validate_cidr("10.0.0.0/29").is_err()); // Too small for VPC
    }

    #[test]
    fn test_validate_config_valid() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "enable_dns_hostnames": false,
            "instance_tenancy": "default",
            "tags": {
                "Name": "test-vpc",
                "Environment": "test"
            }
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_config_minimal() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_cidr() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "enable_dns_support": true
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_config_invalid_tenancy() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "instance_tenancy": "invalid"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_config_invalid_dns_type() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": "yes"  // Should be boolean
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_config_invalid_tags_type() {
        let resource = AwsVpcResource::new();
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "tags": "invalid"  // Should be object
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_extract_config() {
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "enable_dns_hostnames": true,
            "instance_tenancy": "dedicated",
            "tags": {
                "Name": "test-vpc"
            }
        });

        let vpc_config = AwsVpcResource::extract_config(&config).unwrap();
        assert_eq!(vpc_config.cidr_block, "10.0.0.0/16");
        assert!(vpc_config.enable_dns_support);
        assert!(vpc_config.enable_dns_hostnames);
        assert_eq!(vpc_config.instance_tenancy, "dedicated");
        assert_eq!(vpc_config.tags.get("Name"), Some(&"test-vpc".to_string()));
    }

    #[test]
    fn test_extract_config_defaults() {
        let config = serde_json::json!({
            "cidr_block": "10.0.0.0/16"
        });

        let vpc_config = AwsVpcResource::extract_config(&config).unwrap();
        assert_eq!(vpc_config.cidr_block, "10.0.0.0/16");
        assert!(vpc_config.enable_dns_support); // Default true
        assert!(!vpc_config.enable_dns_hostnames); // Default false
        assert_eq!(vpc_config.instance_tenancy, "default");
        assert!(vpc_config.tags.is_empty());
    }

    #[test]
    fn test_compute_diff_create() {
        let desired = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true
        });

        let diff = AwsVpcResource::compute_diff(&desired, None, &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
        assert!(diff.additions.contains_key("cidr_block"));
        assert!(!diff.requires_replacement);
    }

    #[test]
    fn test_compute_diff_no_change() {
        let desired = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true
        });

        let current = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "id": "vpc-12345",
            "arn": "arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345"
        });

        let diff = AwsVpcResource::compute_diff(&desired, Some(&current), &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_compute_diff_update() {
        let desired = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "enable_dns_hostnames": true  // Changed
        });

        let current = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "enable_dns_hostnames": false,
            "id": "vpc-12345"
        });

        let diff = AwsVpcResource::compute_diff(&desired, Some(&current), &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::Update);
        assert!(diff.modifications.contains_key("enable_dns_hostnames"));
        assert!(!diff.requires_replacement);
    }

    #[test]
    fn test_compute_diff_replace() {
        let desired = serde_json::json!({
            "cidr_block": "192.168.0.0/16",  // Changed (force_new)
            "enable_dns_support": true
        });

        let current = serde_json::json!({
            "cidr_block": "10.0.0.0/16",
            "enable_dns_support": true,
            "id": "vpc-12345"
        });

        let force_new = vec!["cidr_block".to_string()];
        let diff = AwsVpcResource::compute_diff(&desired, Some(&current), &force_new).unwrap();
        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
        assert!(diff.replacement_fields.contains(&"cidr_block".to_string()));
    }

    #[test]
    fn test_vpc_attributes_serialization() {
        let attrs = VpcAttributes {
            id: "vpc-12345".to_string(),
            arn: "arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345".to_string(),
            cidr_block: "10.0.0.0/16".to_string(),
            main_route_table_id: Some("rtb-12345".to_string()),
            default_network_acl_id: Some("acl-12345".to_string()),
            default_security_group_id: Some("sg-12345".to_string()),
            default_route_table_id: Some("rtb-12345".to_string()),
            owner_id: "123456789012".to_string(),
            enable_dns_support: true,
            enable_dns_hostnames: false,
            instance_tenancy: "default".to_string(),
            state: "available".to_string(),
            is_default: false,
            tags: HashMap::from([("Name".to_string(), "test-vpc".to_string())]),
        };

        let json = serde_json::to_value(&attrs).unwrap();
        assert_eq!(json["id"], "vpc-12345");
        assert_eq!(json["cidr_block"], "10.0.0.0/16");
        assert_eq!(json["enable_dns_support"], true);
        assert_eq!(json["tags"]["Name"], "test-vpc");
    }

    #[test]
    fn test_resource_timeouts() {
        let resource = AwsVpcResource::new();
        let schema = resource.schema();

        assert_eq!(schema.timeouts.create, 300);
        assert_eq!(schema.timeouts.read, 60);
        assert_eq!(schema.timeouts.update, 300);
        assert_eq!(schema.timeouts.delete, 600);
    }
}
