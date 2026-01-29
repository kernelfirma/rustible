//! AWS Launch Template Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsLaunchTemplateResource` which implements the `Resource` trait
//! for managing AWS EC2 Launch Templates declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_launch_template:
//!     web_template:
//!       name: web-servers
//!       image_id: ami-12345678
//!       instance_type: t3.micro
//!       key_name: my-key
//!       vpc_security_group_ids:
//!         - sg-12345678
//!       block_device_mappings:
//!         - device_name: /dev/xvda
//!           ebs:
//!             volume_size: 20
//!             volume_type: gp3
//!       tags:
//!         Name: web-template
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{
    LaunchTemplateBlockDeviceMappingRequest, LaunchTemplateEbsBlockDeviceRequest,
    LaunchTemplateIamInstanceProfileSpecificationRequest,
    LaunchTemplateInstanceMarketOptionsRequest,
    LaunchTemplateInstanceNetworkInterfaceSpecificationRequest, LaunchTemplatePlacementRequest,
    LaunchTemplateTagSpecificationRequest, MarketType, RequestLaunchTemplateData, ResourceType,
    SpotInstanceType, Tag, TagSpecification,
};
use aws_sdk_ec2::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Supporting Types
// ============================================================================

/// EBS block device configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EbsConfig {
    /// Volume size in GiB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_size: Option<i32>,
    /// Volume type: gp2, gp3, io1, io2, st1, sc1, standard
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    /// IOPS for io1/io2/gp3
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    /// Throughput for gp3 (MiB/s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
    /// Delete on termination
    #[serde(default = "default_true")]
    pub delete_on_termination: bool,
    /// Whether to encrypt the volume
    #[serde(default)]
    pub encrypted: bool,
    /// KMS key ID for encryption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    /// Snapshot ID to restore from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
}

/// Block device mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDeviceMapping {
    /// Device name (e.g., /dev/xvda)
    pub device_name: String,
    /// EBS volume configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebs: Option<EbsConfig>,
    /// Virtual device name for instance store
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_name: Option<String>,
    /// Suppress device
    #[serde(default)]
    pub no_device: bool,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkInterfaceConfig {
    /// Associate public IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associate_public_ip_address: Option<bool>,
    /// Delete on termination
    #[serde(default = "default_true")]
    pub delete_on_termination: bool,
    /// Device index
    #[serde(default)]
    pub device_index: i32,
    /// Network interface ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interface_id: Option<String>,
    /// Subnet ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
    /// Security group IDs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security_groups: Vec<String>,
    /// Private IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_address: Option<String>,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// IAM instance profile configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IamInstanceProfile {
    /// IAM instance profile ARN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arn: Option<String>,
    /// IAM instance profile name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Instance market options (spot)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceMarketOptions {
    /// Market type: spot
    pub market_type: String,
    /// Spot options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_options: Option<SpotOptions>,
}

/// Spot instance options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpotOptions {
    /// Maximum price
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<String>,
    /// Spot instance type: one-time, persistent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_instance_type: Option<String>,
    /// Block duration minutes (deprecated but still supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_duration_minutes: Option<i32>,
}

/// Placement configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlacementConfig {
    /// Availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    /// Placement group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    /// Tenancy: default, dedicated, host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenancy: Option<String>,
    /// Host ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    /// Partition number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_number: Option<i32>,
}

/// Tag specification for resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSpecConfig {
    /// Resource type: instance, volume, spot-instances-request
    pub resource_type: String,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// Launch template configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchTemplateConfig {
    /// Launch template name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Name prefix (for unique name generation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_prefix: Option<String>,
    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_version: Option<i64>,
    /// Update default version on new version
    #[serde(default = "default_true")]
    pub update_default_version: bool,
    /// AMI ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    /// Instance type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// Key name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// VPC security group IDs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vpc_security_group_ids: Vec<String>,
    /// User data (base64 encoded or plain text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
    /// Block device mappings
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub block_device_mappings: Vec<BlockDeviceMapping>,
    /// Network interfaces
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<NetworkInterfaceConfig>,
    /// IAM instance profile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iam_instance_profile: Option<IamInstanceProfile>,
    /// Instance market options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_market_options: Option<InstanceMarketOptions>,
    /// Placement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementConfig>,
    /// EBS optimized
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebs_optimized: Option<bool>,
    /// Disable API termination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_api_termination: Option<bool>,
    /// Instance initiated shutdown behavior: stop, terminate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_initiated_shutdown_behavior: Option<String>,
    /// Enable detailed monitoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<bool>,
    /// Tag specifications
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_specifications: Vec<TagSpecConfig>,
    /// Resource tags (for the launch template itself)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

impl LaunchTemplateConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid launch template configuration: {}",
                e
            ))
        })
    }
}

/// Computed attributes returned after launch template operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaunchTemplateState {
    /// Launch template ID
    pub id: String,
    /// Launch template ARN
    pub arn: String,
    /// Launch template name
    pub name: String,
    /// Default version number
    pub default_version: i64,
    /// Latest version number
    pub latest_version: i64,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS Launch Template Resource
// ============================================================================

/// AWS Launch Template Resource implementation
#[derive(Debug, Clone)]
pub struct AwsLaunchTemplateResource;

impl AwsLaunchTemplateResource {
    /// Create a new AWS Launch Template resource
    pub fn new() -> Self {
        Self
    }

    /// Create AWS EC2 client from provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
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

    /// Build launch template ARN
    fn build_arn(&self, template_id: &str, region: &str, account_id: Option<&str>) -> String {
        let account = account_id.unwrap_or("*");
        format!(
            "arn:aws:ec2:{}:{}:launch-template/{}",
            region, account, template_id
        )
    }

    /// Describe launch template by ID or name
    async fn describe_launch_template(
        &self,
        client: &Client,
        identifier: &str,
    ) -> ProvisioningResult<Option<LaunchTemplateState>> {
        let resp = if identifier.starts_with("lt-") {
            client
                .describe_launch_templates()
                .launch_template_ids(identifier)
                .send()
                .await
        } else {
            client
                .describe_launch_templates()
                .launch_template_names(identifier)
                .send()
                .await
        };

        match resp {
            Ok(output) => {
                if let Some(lt) = output.launch_templates().first() {
                    let template_id = lt.launch_template_id().unwrap_or_default().to_string();
                    let name = lt.launch_template_name().unwrap_or_default().to_string();
                    let default_version = lt.default_version_number().unwrap_or(1);
                    let latest_version = lt.latest_version_number().unwrap_or(1);

                    let mut tags = HashMap::new();
                    for tag in lt.tags() {
                        if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                            tags.insert(key.to_string(), value.to_string());
                        }
                    }

                    let region = "us-east-1"; // Would need to extract from context
                    let arn = self.build_arn(&template_id, region, None);

                    Ok(Some(LaunchTemplateState {
                        id: template_id,
                        arn,
                        name,
                        default_version,
                        latest_version,
                        tags,
                    }))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("InvalidLaunchTemplateId")
                    || err_str.contains("InvalidLaunchTemplateName")
                    || err_str.contains("not found")
                {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to describe launch template: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Build launch template data from config
    fn build_template_data(
        &self,
        config: &LaunchTemplateConfig,
    ) -> ProvisioningResult<RequestLaunchTemplateData> {
        let mut data = RequestLaunchTemplateData::builder();

        // Image ID
        if let Some(ref image_id) = config.image_id {
            data = data.image_id(image_id);
        }

        // Instance type
        if let Some(ref instance_type) = config.instance_type {
            data = data.instance_type(instance_type.parse().map_err(|_| {
                ProvisioningError::ValidationError(format!(
                    "Invalid instance type: {}",
                    instance_type
                ))
            })?);
        }

        // Key name
        if let Some(ref key_name) = config.key_name {
            data = data.key_name(key_name);
        }

        // Security groups (only if no network interfaces)
        if config.network_interfaces.is_empty() {
            for sg in &config.vpc_security_group_ids {
                data = data.security_group_ids(sg);
            }
        }

        // User data
        if let Some(ref user_data) = config.user_data {
            let encoded = self.encode_user_data(user_data);
            data = data.user_data(encoded);
        }

        // Block device mappings
        for bdm in &config.block_device_mappings {
            let mut mapping =
                LaunchTemplateBlockDeviceMappingRequest::builder().device_name(&bdm.device_name);

            if bdm.no_device {
                mapping = mapping.no_device("");
            } else if let Some(ref virtual_name) = bdm.virtual_name {
                mapping = mapping.virtual_name(virtual_name);
            } else if let Some(ref ebs) = bdm.ebs {
                let mut ebs_builder = LaunchTemplateEbsBlockDeviceRequest::builder()
                    .delete_on_termination(ebs.delete_on_termination)
                    .encrypted(ebs.encrypted);

                if let Some(size) = ebs.volume_size {
                    ebs_builder = ebs_builder.volume_size(size);
                }
                if let Some(ref vol_type) = ebs.volume_type {
                    ebs_builder = ebs_builder.volume_type(vol_type.parse().map_err(|_| {
                        ProvisioningError::ValidationError(format!(
                            "Invalid volume type: {}",
                            vol_type
                        ))
                    })?);
                }
                if let Some(iops) = ebs.iops {
                    ebs_builder = ebs_builder.iops(iops);
                }
                if let Some(throughput) = ebs.throughput {
                    ebs_builder = ebs_builder.throughput(throughput);
                }
                if let Some(ref kms_key) = ebs.kms_key_id {
                    ebs_builder = ebs_builder.kms_key_id(kms_key);
                }
                if let Some(ref snapshot) = ebs.snapshot_id {
                    ebs_builder = ebs_builder.snapshot_id(snapshot);
                }

                mapping = mapping.ebs(ebs_builder.build());
            }

            data = data.block_device_mappings(mapping.build());
        }

        // Network interfaces
        for ni in &config.network_interfaces {
            let mut ni_builder =
                LaunchTemplateInstanceNetworkInterfaceSpecificationRequest::builder()
                    .device_index(ni.device_index)
                    .delete_on_termination(ni.delete_on_termination);

            if let Some(associate_public) = ni.associate_public_ip_address {
                ni_builder = ni_builder.associate_public_ip_address(associate_public);
            }
            if let Some(ref ni_id) = ni.network_interface_id {
                ni_builder = ni_builder.network_interface_id(ni_id);
            }
            if let Some(ref subnet_id) = ni.subnet_id {
                ni_builder = ni_builder.subnet_id(subnet_id);
            }
            for sg in &ni.security_groups {
                ni_builder = ni_builder.groups(sg);
            }
            if let Some(ref private_ip) = ni.private_ip_address {
                ni_builder = ni_builder.private_ip_address(private_ip);
            }
            if let Some(ref desc) = ni.description {
                ni_builder = ni_builder.description(desc);
            }

            data = data.network_interfaces(ni_builder.build());
        }

        // IAM instance profile
        if let Some(ref profile) = config.iam_instance_profile {
            let mut profile_builder =
                LaunchTemplateIamInstanceProfileSpecificationRequest::builder();
            if let Some(ref arn) = profile.arn {
                profile_builder = profile_builder.arn(arn);
            }
            if let Some(ref name) = profile.name {
                profile_builder = profile_builder.name(name);
            }
            data = data.iam_instance_profile(profile_builder.build());
        }

        // Instance market options (spot)
        if let Some(ref market_options) = config.instance_market_options {
            let mut market_builder = LaunchTemplateInstanceMarketOptionsRequest::builder();

            if market_options.market_type == "spot" {
                market_builder = market_builder.market_type(MarketType::Spot);

                if let Some(ref spot_options) = market_options.spot_options {
                    let mut spot_builder =
                        aws_sdk_ec2::types::LaunchTemplateSpotMarketOptionsRequest::builder();

                    if let Some(ref max_price) = spot_options.max_price {
                        spot_builder = spot_builder.max_price(max_price);
                    }
                    if let Some(ref spot_type) = spot_options.spot_instance_type {
                        let spot_instance_type = match spot_type.as_str() {
                            "persistent" => SpotInstanceType::Persistent,
                            _ => SpotInstanceType::OneTime,
                        };
                        spot_builder = spot_builder.spot_instance_type(spot_instance_type);
                    }
                    if let Some(block_duration) = spot_options.block_duration_minutes {
                        spot_builder = spot_builder.block_duration_minutes(block_duration);
                    }

                    market_builder = market_builder.spot_options(spot_builder.build());
                }
            }

            data = data.instance_market_options(market_builder.build());
        }

        // Placement
        if let Some(ref placement) = config.placement {
            let mut placement_builder = LaunchTemplatePlacementRequest::builder();

            if let Some(ref az) = placement.availability_zone {
                placement_builder = placement_builder.availability_zone(az);
            }
            if let Some(ref group_name) = placement.group_name {
                placement_builder = placement_builder.group_name(group_name);
            }
            if let Some(ref tenancy) = placement.tenancy {
                placement_builder = placement_builder.tenancy(tenancy.parse().map_err(|_| {
                    ProvisioningError::ValidationError(format!("Invalid tenancy: {}", tenancy))
                })?);
            }
            if let Some(ref host_id) = placement.host_id {
                placement_builder = placement_builder.host_id(host_id);
            }
            if let Some(partition) = placement.partition_number {
                placement_builder = placement_builder.partition_number(partition);
            }

            data = data.placement(placement_builder.build());
        }

        // EBS optimized
        if let Some(ebs_optimized) = config.ebs_optimized {
            data = data.ebs_optimized(ebs_optimized);
        }

        // Disable API termination
        if let Some(disable_termination) = config.disable_api_termination {
            data = data.disable_api_termination(disable_termination);
        }

        // Instance initiated shutdown behavior
        if let Some(ref behavior) = config.instance_initiated_shutdown_behavior {
            data = data.instance_initiated_shutdown_behavior(behavior.parse().map_err(|_| {
                ProvisioningError::ValidationError(format!(
                    "Invalid shutdown behavior: {}",
                    behavior
                ))
            })?);
        }

        // Monitoring
        if let Some(monitoring) = config.monitoring {
            data = data.monitoring(
                aws_sdk_ec2::types::LaunchTemplatesMonitoringRequest::builder()
                    .enabled(monitoring)
                    .build(),
            );
        }

        // Tag specifications (for instances created from this template)
        for tag_spec in &config.tag_specifications {
            let resource_type = match tag_spec.resource_type.as_str() {
                "instance" => ResourceType::Instance,
                "volume" => ResourceType::Volume,
                "spot-instances-request" => ResourceType::SpotInstancesRequest,
                "network-interface" => ResourceType::NetworkInterface,
                _ => ResourceType::Instance,
            };

            let tags: Vec<Tag> = tag_spec
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            let spec = LaunchTemplateTagSpecificationRequest::builder()
                .resource_type(resource_type)
                .set_tags(Some(tags))
                .build();

            data = data.tag_specifications(spec);
        }

        Ok(data.build())
    }

    /// Encode user data to base64 if not already encoded
    fn encode_user_data(&self, user_data: &str) -> String {
        if user_data.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c.is_whitespace()
        }) {
            if base64::Engine::decode(&base64::engine::general_purpose::STANDARD, user_data.trim())
                .is_ok()
            {
                return user_data.to_string();
            }
        }
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, user_data)
    }

    /// Extract references from configuration value
    fn extract_references(&self, value: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(s) = value.as_str() {
            if s.contains("${") || s.contains("{{") {
                if let Some(dep) = self.parse_reference(s) {
                    deps.push(dep);
                }
            }
        }

        deps
    }

    /// Parse a reference string
    fn parse_reference(&self, ref_str: &str) -> Option<ResourceDependency> {
        if let Some(start) = ref_str.find("${") {
            if let Some(end) = ref_str[start..].find('}') {
                let inner = &ref_str[start + 2..start + end];
                let parts: Vec<&str> = inner.split('.').collect();
                if parts.len() >= 2 {
                    return Some(ResourceDependency::new(
                        parts[0],
                        parts[1],
                        parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    ));
                }
            }
        }
        None
    }
}

impl Default for AwsLaunchTemplateResource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Resource for AwsLaunchTemplateResource {
    fn resource_type(&self) -> &str {
        "aws_launch_template"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_launch_template".to_string(),
            description: "Provides an EC2 Launch Template resource.".to_string(),
            required_args: vec![],
            optional_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the launch template".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 3 },
                        FieldConstraint::MaxLength { max: 128 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "name_prefix".to_string(),
                    field_type: FieldType::String,
                    description: "Creates a unique name beginning with the specified prefix"
                        .to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Description of the launch template".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "image_id".to_string(),
                    field_type: FieldType::String,
                    description: "The AMI from which to launch the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance_type".to_string(),
                    field_type: FieldType::String,
                    description: "The instance type to use".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "key_name".to_string(),
                    field_type: FieldType::String,
                    description: "The key name to use for the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_security_group_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "A list of security group IDs to associate with".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "user_data".to_string(),
                    field_type: FieldType::String,
                    description: "The user data to provide when launching the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "A map of tags to assign to the launch template".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "The launch template ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the launch template".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_version".to_string(),
                    field_type: FieldType::Integer,
                    description: "The default version of the launch template".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "latest_version".to_string(),
                    field_type: FieldType::Integer,
                    description: "The latest version of the launch template".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "name_prefix".to_string()],
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
        let client = self.create_client(ctx).await?;

        match self.describe_launch_template(&client, id).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize launch template attributes: {}",
                        e
                    ))
                })?;

                Ok(ResourceReadResult::found(id, attributes))
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
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(current_val) => {
                let mut diff = ResourceDiff::no_change();
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                let empty_map = serde_json::Map::new();
                let desired_obj = desired.as_object().unwrap_or(&empty_map);
                let current_obj = current_val.as_object().unwrap_or(&empty_map);

                let force_new = ["name", "name_prefix"];

                for (key, des_val) in desired_obj {
                    let cur_val = current_obj.get(key);

                    match cur_val {
                        Some(cv) if cv != des_val => {
                            diff.modifications
                                .insert(key.clone(), (cv.clone(), des_val.clone()));

                            if force_new.contains(&key.as_str()) {
                                requires_replacement = true;
                                replacement_fields.push(key.clone());
                            }
                        }
                        None => {
                            diff.additions.insert(key.clone(), des_val.clone());
                        }
                        _ => {}
                    }
                }

                // Check for deletions
                let computed_fields = ["id", "arn", "default_version", "latest_version"];
                for key in current_obj.keys() {
                    if !desired_obj.contains_key(key)
                        && !key.starts_with('_')
                        && !computed_fields.contains(&key.as_str())
                    {
                        diff.deletions.push(key.clone());
                    }
                }

                if !diff.additions.is_empty()
                    || !diff.modifications.is_empty()
                    || !diff.deletions.is_empty()
                {
                    if requires_replacement {
                        diff.change_type = ChangeType::Replace;
                        diff.requires_replacement = true;
                        diff.replacement_fields = replacement_fields;
                    } else {
                        diff.change_type = ChangeType::Update;
                    }
                }

                Ok(diff)
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let lt_config = LaunchTemplateConfig::from_value(config)?;
        let client = self.create_client(ctx).await?;

        // Determine template name
        let template_name = lt_config
            .name
            .clone()
            .or_else(|| {
                lt_config.name_prefix.as_ref().map(|prefix| {
                    format!(
                        "{}{}",
                        prefix,
                        uuid::Uuid::new_v4()
                            .to_string()
                            .split('-')
                            .next()
                            .unwrap_or("")
                    )
                })
            })
            .ok_or_else(|| {
                ProvisioningError::ValidationError(
                    "Either name or name_prefix must be specified".to_string(),
                )
            })?;

        info!("Creating launch template: {}", template_name);

        // Build launch template data
        let template_data = self.build_template_data(&lt_config)?;

        // Build tag specifications for the launch template itself
        let mut lt_tags = vec![];
        for (key, value) in &lt_config.tags {
            lt_tags.push(Tag::builder().key(key).value(value).build());
        }
        for (key, value) in &ctx.default_tags {
            if !lt_config.tags.contains_key(key) {
                lt_tags.push(Tag::builder().key(key).value(value).build());
            }
        }

        let mut create_lt = client
            .create_launch_template()
            .launch_template_name(&template_name)
            .launch_template_data(template_data);

        // Add description
        if let Some(ref description) = lt_config.description {
            create_lt = create_lt.version_description(description);
        }

        // Add tags
        if !lt_tags.is_empty() {
            create_lt = create_lt.tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::LaunchTemplate)
                    .set_tags(Some(lt_tags))
                    .build(),
            );
        }

        let resp = create_lt.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create launch template: {}", e))
        })?;

        let lt = resp.launch_template().ok_or_else(|| {
            ProvisioningError::CloudApiError("No launch template returned".to_string())
        })?;

        let template_id = lt.launch_template_id().unwrap_or_default().to_string();
        let region = ctx.region.as_deref().unwrap_or("us-east-1");

        info!(
            "Created launch template: {} ({})",
            template_name, template_id
        );

        let state = LaunchTemplateState {
            id: template_id.clone(),
            arn: self.build_arn(&template_id, region, None),
            name: template_name,
            default_version: lt.default_version_number().unwrap_or(1),
            latest_version: lt.latest_version_number().unwrap_or(1),
            tags: lt_config.tags.clone(),
        };

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(&template_id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output(
                "default_version",
                Value::Number(state.default_version.into()),
            )
            .with_output("latest_version", Value::Number(state.latest_version.into())))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let lt_config = LaunchTemplateConfig::from_value(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating launch template: {}", id);

        // Build new version data
        let template_data = self.build_template_data(&lt_config)?;

        // Create new version
        let mut create_version = client
            .create_launch_template_version()
            .launch_template_id(id)
            .launch_template_data(template_data);

        if let Some(ref description) = lt_config.description {
            create_version = create_version.version_description(description);
        }

        // Source version (use latest)
        let current_state = self
            .describe_launch_template(&client, id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Launch template not found".to_string())
            })?;
        create_version = create_version.source_version(current_state.latest_version.to_string());

        let version_resp = create_version.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!(
                "Failed to create launch template version: {}",
                e
            ))
        })?;

        let new_version = version_resp
            .launch_template_version()
            .and_then(|v| v.version_number())
            .unwrap_or(current_state.latest_version + 1);

        // Update default version if requested
        if lt_config.update_default_version {
            client
                .modify_launch_template()
                .launch_template_id(id)
                .default_version(new_version.to_string())
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to update default version: {}",
                        e
                    ))
                })?;
        }

        // Update tags
        if !lt_config.tags.is_empty() {
            let region = ctx.region.as_deref().unwrap_or("us-east-1");
            let arn = self.build_arn(id, region, None);

            let tags: Vec<Tag> = lt_config
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            client
                .create_tags()
                .resources(&arn)
                .set_tags(Some(tags))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                })?;
        }

        // Get final state
        let state = self
            .describe_launch_template(&client, id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError(
                    "Launch template not found after update".to_string(),
                )
            })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Check if launch template exists
        if self.describe_launch_template(&client, id).await?.is_none() {
            return Ok(ResourceResult::success(id, Value::Null));
        }

        info!("Deleting launch template: {}", id);

        client
            .delete_launch_template()
            .launch_template_id(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete launch template: {}", e))
            })?;

        info!("Deleted launch template: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        let state = self
            .describe_launch_template(&client, id)
            .await?
            .ok_or_else(|| ProvisioningError::ImportError {
                resource_type: "aws_launch_template".to_string(),
                resource_id: id.to_string(),
                message: "Launch template not found".to_string(),
            })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(obj) = config.as_object() {
            // Check image_id for references
            if let Some(image_id) = obj.get("image_id") {
                deps.extend(self.extract_references(image_id));
            }

            // Check vpc_security_group_ids
            if let Some(sgs) = obj.get("vpc_security_group_ids") {
                if let Some(arr) = sgs.as_array() {
                    for sg in arr {
                        deps.extend(self.extract_references(sg));
                    }
                }
            }

            // Check key_name
            if let Some(key) = obj.get("key_name") {
                deps.extend(self.extract_references(key));
            }

            // Check iam_instance_profile
            if let Some(profile) = obj.get("iam_instance_profile") {
                if let Some(arn) = profile.get("arn") {
                    deps.extend(self.extract_references(arn));
                }
                if let Some(name) = profile.get("name") {
                    deps.extend(self.extract_references(name));
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "name_prefix".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate that either name or name_prefix is provided
        let has_name = obj.contains_key("name");
        let has_prefix = obj.contains_key("name_prefix");

        if !has_name && !has_prefix {
            return Err(ProvisioningError::ValidationError(
                "Either name or name_prefix must be specified".to_string(),
            ));
        }

        // Validate name length
        if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
            if name.len() < 3 || name.len() > 128 {
                return Err(ProvisioningError::ValidationError(
                    "name must be between 3 and 128 characters".to_string(),
                ));
            }
        }

        // Validate AMI format if provided
        if let Some(image_id) = obj.get("image_id").and_then(|v| v.as_str()) {
            // Skip validation if it's a reference
            if !image_id.contains("${") && !image_id.contains("{{") && !image_id.starts_with("ami-")
            {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid image_id format: {}. Must start with 'ami-'",
                    image_id
                )));
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
    use serde_json::json;

    #[test]
    fn test_resource_type() {
        let resource = AwsLaunchTemplateResource::new();
        assert_eq!(resource.resource_type(), "aws_launch_template");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsLaunchTemplateResource::new();
        let forces = resource.forces_replacement();

        assert!(forces.contains(&"name".to_string()));
        assert!(forces.contains(&"name_prefix".to_string()));
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "my-template",
            "image_id": "ami-12345678",
            "instance_type": "t3.micro"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_name_and_prefix() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "image_id": "ami-12345678"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_name_prefix() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name_prefix": "web-",
            "image_id": "ami-12345678"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_name_too_short() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "ab"  // Too short
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_ami() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "my-template",
            "image_id": "invalid-ami"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_ami_reference() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "my-template",
            "image_id": "${data.aws_ami.latest.id}"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_launch_template_config_parsing() {
        let config = json!({
            "name": "web-template",
            "description": "Web server template",
            "image_id": "ami-12345678",
            "instance_type": "t3.micro",
            "key_name": "my-key",
            "vpc_security_group_ids": ["sg-12345678"],
            "ebs_optimized": true,
            "monitoring": true,
            "tags": {
                "Name": "web-template",
                "Environment": "production"
            }
        });

        let lt_config = LaunchTemplateConfig::from_value(&config).unwrap();

        assert_eq!(lt_config.name, Some("web-template".to_string()));
        assert_eq!(
            lt_config.description,
            Some("Web server template".to_string())
        );
        assert_eq!(lt_config.image_id, Some("ami-12345678".to_string()));
        assert_eq!(lt_config.instance_type, Some("t3.micro".to_string()));
        assert_eq!(lt_config.ebs_optimized, Some(true));
        assert_eq!(lt_config.monitoring, Some(true));
        assert_eq!(
            lt_config.tags.get("Name"),
            Some(&"web-template".to_string())
        );
    }

    #[test]
    fn test_block_device_mapping_parsing() {
        let config = json!({
            "name": "my-template",
            "block_device_mappings": [
                {
                    "device_name": "/dev/xvda",
                    "ebs": {
                        "volume_size": 50,
                        "volume_type": "gp3",
                        "encrypted": true,
                        "iops": 3000,
                        "throughput": 125
                    }
                }
            ]
        });

        let lt_config = LaunchTemplateConfig::from_value(&config).unwrap();

        assert_eq!(lt_config.block_device_mappings.len(), 1);
        let bdm = &lt_config.block_device_mappings[0];
        assert_eq!(bdm.device_name, "/dev/xvda");
        assert!(bdm.ebs.is_some());

        let ebs = bdm.ebs.as_ref().unwrap();
        assert_eq!(ebs.volume_size, Some(50));
        assert_eq!(ebs.volume_type, Some("gp3".to_string()));
        assert!(ebs.encrypted);
        assert_eq!(ebs.iops, Some(3000));
        assert_eq!(ebs.throughput, Some(125));
    }

    #[test]
    fn test_plan_create() {
        let resource = AwsLaunchTemplateResource::new();

        let desired = json!({
            "name": "my-template"
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 300,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, None, &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_plan_no_change() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "my-template"
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 300,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&config, Some(&config), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_plan_replace_name_change() {
        let resource = AwsLaunchTemplateResource::new();

        let current = json!({
            "name": "old-template"
        });

        let desired = json!({
            "name": "new-template"
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 300,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, Some(&current), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
    }

    #[test]
    fn test_state_serialization() {
        let state = LaunchTemplateState {
            id: "lt-12345678".to_string(),
            arn: "arn:aws:ec2:us-east-1:123456789012:launch-template/lt-12345678".to_string(),
            name: "my-template".to_string(),
            default_version: 1,
            latest_version: 3,
            tags: HashMap::new(),
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "lt-12345678");
        assert_eq!(json["name"], "my-template");
        assert_eq!(json["default_version"], 1);
        assert_eq!(json["latest_version"], 3);
    }

    #[test]
    fn test_build_arn() {
        let resource = AwsLaunchTemplateResource::new();
        let arn = resource.build_arn("lt-12345678", "us-east-1", Some("123456789012"));
        assert_eq!(
            arn,
            "arn:aws:ec2:us-east-1:123456789012:launch-template/lt-12345678"
        );
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsLaunchTemplateResource::new();

        let config = json!({
            "name": "my-template",
            "image_id": "${data.aws_ami.latest.id}",
            "vpc_security_group_ids": ["${aws_security_group.web.id}"]
        });

        let deps = resource.dependencies(&config);

        let has_ami = deps.iter().any(|d| d.resource_type == "data");
        let has_sg = deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "web");

        assert!(has_ami || deps.iter().any(|d| d.resource_name == "aws_ami"));
        assert!(has_sg);
    }
}
