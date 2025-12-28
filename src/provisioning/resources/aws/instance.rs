//! AWS EC2 Instance Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsInstanceResource` which implements the `Resource` trait
//! for managing AWS EC2 instances declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_instance:
//!     web_server:
//!       ami: ami-0abcdef1234567890
//!       instance_type: t3.micro
//!       subnet_id: subnet-12345678
//!       vpc_security_group_ids:
//!         - sg-12345678
//!       key_name: my-key-pair
//!       tags:
//!         Name: web-server
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{
    BlockDeviceMapping, EbsBlockDevice, IamInstanceProfileSpecification,
    InstanceNetworkInterfaceSpecification, InstanceType, Placement, ResourceType, Tag,
    TagSpecification, Tenancy, VolumeType,
};
use aws_sdk_ec2::Client;
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
// Supporting Types
// ============================================================================

/// Root block device configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RootBlockDevice {
    /// Volume size in GiB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_size: Option<i32>,
    /// Volume type: gp2, gp3, io1, io2, st1, sc1, standard
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    /// IOPS for io1/io2/gp3 volumes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    /// Throughput for gp3 volumes (MiB/s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
    /// Whether to delete on instance termination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_on_termination: Option<bool>,
    /// Whether the volume is encrypted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    /// KMS key ID for encryption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
}

impl RootBlockDevice {
    /// Convert to AWS SDK BlockDeviceMapping for /dev/xvda or /dev/sda1
    fn to_block_device_mapping(&self, device_name: &str) -> BlockDeviceMapping {
        let mut ebs = EbsBlockDevice::builder();

        if let Some(size) = self.volume_size {
            ebs = ebs.volume_size(size);
        }
        if let Some(ref vol_type) = self.volume_type {
            let vt = vol_type.parse::<VolumeType>().unwrap_or(VolumeType::Gp3);
            ebs = ebs.volume_type(vt);
        }
        if let Some(iops) = self.iops {
            ebs = ebs.iops(iops);
        }
        if let Some(throughput) = self.throughput {
            ebs = ebs.throughput(throughput);
        }
        if let Some(delete) = self.delete_on_termination {
            ebs = ebs.delete_on_termination(delete);
        }
        if let Some(encrypted) = self.encrypted {
            ebs = ebs.encrypted(encrypted);
        }
        if let Some(ref kms_key) = self.kms_key_id {
            ebs = ebs.kms_key_id(kms_key);
        }

        BlockDeviceMapping::builder()
            .device_name(device_name)
            .ebs(ebs.build())
            .build()
    }
}

/// Instance configuration parsed from provisioning config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// AMI ID (required)
    pub ami: String,
    /// Instance type (default: t3.micro)
    #[serde(default = "default_instance_type")]
    pub instance_type: String,
    /// Subnet ID for VPC placement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
    /// VPC security group IDs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vpc_security_group_ids: Vec<String>,
    /// SSH key pair name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// IAM instance profile name or ARN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iam_instance_profile: Option<String>,
    /// User data script (base64 or plain text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
    /// Whether to associate a public IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associate_public_ip_address: Option<bool>,
    /// Availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    /// Resource tags
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Root block device configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_block_device: Option<RootBlockDevice>,
    /// Whether EBS optimization is enabled
    #[serde(default)]
    pub ebs_optimized: bool,
    /// Whether detailed monitoring is enabled
    #[serde(default)]
    pub monitoring: bool,
    /// Private IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip: Option<String>,
    /// Instance tenancy (default, dedicated, host)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenancy: Option<String>,
}

fn default_instance_type() -> String {
    "t3.micro".to_string()
}

impl InstanceConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!("Invalid instance configuration: {}", e))
        })
    }
}

/// Computed attributes returned after instance operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceAttributes {
    /// Instance ID
    pub id: String,
    /// Instance ARN
    pub arn: String,
    /// Public IP address (if assigned)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_ip: Option<String>,
    /// Private IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip: Option<String>,
    /// Public DNS name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_dns: Option<String>,
    /// Private DNS name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_dns: Option<String>,
    /// Instance state
    pub instance_state: String,
    /// VPC ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    /// Subnet ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
    /// Security group IDs
    #[serde(default)]
    pub security_groups: Vec<String>,
    /// Availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    /// Launch time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_time: Option<String>,
    /// AMI ID used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ami: Option<String>,
    /// Instance type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// Key name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS Instance Resource
// ============================================================================

/// AWS EC2 Instance Resource implementation
#[derive(Debug, Clone)]
pub struct AwsInstanceResource;

impl AwsInstanceResource {
    /// Create a new AWS Instance resource
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

    /// Get instance ARN from instance ID and region
    fn build_arn(&self, instance_id: &str, region: &str, account_id: Option<&str>) -> String {
        let account = account_id.unwrap_or("*");
        format!(
            "arn:aws:ec2:{}:{}:instance/{}",
            region, account, instance_id
        )
    }

    /// Describe an instance by ID
    async fn describe_instance(
        &self,
        client: &Client,
        instance_id: &str,
    ) -> ProvisioningResult<Option<InstanceAttributes>> {
        let resp = client
            .describe_instances()
            .instance_ids(instance_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe instance: {}", e))
            })?;

        for reservation in resp.reservations() {
            for instance in reservation.instances() {
                if instance.instance_id() == Some(instance_id) {
                    return Ok(Some(self.instance_to_attributes(instance)));
                }
            }
        }

        Ok(None)
    }

    /// Convert SDK instance to attributes
    fn instance_to_attributes(
        &self,
        instance: &aws_sdk_ec2::types::Instance,
    ) -> InstanceAttributes {
        let instance_id = instance.instance_id().unwrap_or_default().to_string();
        let state = instance
            .state()
            .and_then(|s| s.name())
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let mut tags = HashMap::new();
        for tag in instance.tags() {
            if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                tags.insert(key.to_string(), value.to_string());
            }
        }

        let security_groups: Vec<String> = instance
            .security_groups()
            .iter()
            .filter_map(|sg| sg.group_id().map(|s| s.to_string()))
            .collect();

        let region = instance
            .placement()
            .and_then(|p| p.availability_zone())
            .map(|az| {
                // Extract region from AZ (e.g., "us-east-1a" -> "us-east-1")
                az.trim_end_matches(|c: char| c.is_alphabetic()).to_string()
            })
            .unwrap_or_else(|| "us-east-1".to_string());

        InstanceAttributes {
            id: instance_id.clone(),
            arn: self.build_arn(&instance_id, &region, None),
            public_ip: instance.public_ip_address().map(|s| s.to_string()),
            private_ip: instance.private_ip_address().map(|s| s.to_string()),
            public_dns: instance.public_dns_name().map(|s| s.to_string()),
            private_dns: instance.private_dns_name().map(|s| s.to_string()),
            instance_state: state,
            vpc_id: instance.vpc_id().map(|s| s.to_string()),
            subnet_id: instance.subnet_id().map(|s| s.to_string()),
            security_groups,
            availability_zone: instance
                .placement()
                .and_then(|p| p.availability_zone())
                .map(|s| s.to_string()),
            launch_time: instance.launch_time().map(|t| t.to_string()),
            ami: instance.image_id().map(|s| s.to_string()),
            instance_type: instance.instance_type().map(|t| t.as_str().to_string()),
            key_name: instance.key_name().map(|s| s.to_string()),
            tags,
        }
    }

    /// Wait for instance to reach a specific state
    async fn wait_for_state(
        &self,
        client: &Client,
        instance_id: &str,
        desired_state: &str,
        timeout: Duration,
    ) -> ProvisioningResult<InstanceAttributes> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(5);

        debug!(
            "Waiting for instance {} to reach state '{}'",
            instance_id, desired_state
        );

        loop {
            if start.elapsed() >= timeout {
                return Err(ProvisioningError::Timeout {
                    operation: format!(
                        "waiting for instance {} to reach {}",
                        instance_id, desired_state
                    ),
                    seconds: timeout.as_secs(),
                });
            }

            if let Some(attrs) = self.describe_instance(client, instance_id).await? {
                if attrs.instance_state == desired_state {
                    return Ok(attrs);
                }

                // Check for terminal failure states
                if attrs.instance_state == "terminated" && desired_state != "terminated" {
                    return Err(ProvisioningError::CloudApiError(format!(
                        "Instance {} was terminated unexpectedly",
                        instance_id
                    )));
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Encode user data to base64 if not already encoded
    fn encode_user_data(&self, user_data: &str) -> String {
        // Check if already base64 encoded (simple heuristic)
        if user_data.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c.is_whitespace()
        }) {
            // Might be base64, try to decode
            if base64::Engine::decode(&base64::engine::general_purpose::STANDARD, user_data.trim())
                .is_ok()
            {
                return user_data.to_string();
            }
        }
        // Not base64, encode it
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, user_data)
    }

    /// Extract references from configuration value
    fn extract_references(&self, value: &Value, field_name: &str) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(s) = value.as_str() {
            // Look for patterns like ${aws_subnet.main.id} or {{ resources.aws_subnet.main.id }}
            if s.contains("${") || s.contains("{{") {
                // Parse Terraform-style reference
                if let Some(start) = s.find("${") {
                    if let Some(end) = s[start..].find('}') {
                        let ref_str = &s[start + 2..start + end];
                        if let Some(dep) = self.parse_reference(ref_str) {
                            deps.push(dep);
                        }
                    }
                }
                // Parse Jinja-style reference
                if let Some(start) = s.find("{{") {
                    if let Some(end) = s[start..].find("}}") {
                        let ref_str = &s[start + 2..start + end];
                        let ref_str = ref_str.trim().trim_start_matches("resources.");
                        if let Some(dep) = self.parse_reference(ref_str) {
                            deps.push(dep);
                        }
                    }
                }
            }
        }

        deps
    }

    /// Parse a reference string like "aws_subnet.main.id"
    fn parse_reference(&self, ref_str: &str) -> Option<ResourceDependency> {
        let parts: Vec<&str> = ref_str.split('.').collect();
        if parts.len() >= 3 {
            Some(ResourceDependency::new(
                parts[0],
                parts[1],
                parts[2..].join("."),
            ))
        } else {
            None
        }
    }

    /// Check if a field change requires replacement
    fn requires_replacement_for_field(&self, field: &str) -> bool {
        matches!(
            field,
            "ami" | "subnet_id" | "availability_zone" | "tenancy" | "user_data"
        )
    }
}

impl Default for AwsInstanceResource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Resource for AwsInstanceResource {
    fn resource_type(&self) -> &str {
        "aws_instance"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_instance".to_string(),
            description: "Provides an EC2 instance resource. This resource creates and manages an EC2 instance.".to_string(),
            required_args: vec![
                SchemaField {
                    name: "ami".to_string(),
                    field_type: FieldType::String,
                    description: "AMI to use for the instance".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Pattern {
                        regex: r"^ami-[a-f0-9]{8,17}$".to_string(),
                    }],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "instance_type".to_string(),
                    field_type: FieldType::String,
                    description: "Instance type to use for the instance".to_string(),
                    default: Some(Value::String("t3.micro".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "subnet_id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC Subnet ID to launch in".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Pattern {
                        regex: r"^subnet-[a-f0-9]{8,17}$".to_string(),
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_security_group_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of security group IDs to associate with".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "key_name".to_string(),
                    field_type: FieldType::String,
                    description: "Key name of the Key Pair to use for the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "iam_instance_profile".to_string(),
                    field_type: FieldType::String,
                    description: "IAM Instance Profile to launch the instance with".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "user_data".to_string(),
                    field_type: FieldType::String,
                    description: "User data to provide when launching the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "associate_public_ip_address".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether to associate a public IP address with an instance in a VPC".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "availability_zone".to_string(),
                    field_type: FieldType::String,
                    description: "AZ to start the instance in".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Map of tags to assign to the resource".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "root_block_device".to_string(),
                    field_type: FieldType::Object(vec![
                        SchemaField {
                            name: "volume_size".to_string(),
                            field_type: FieldType::Integer,
                            description: "Size of the volume in gibibytes (GiB)".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "volume_type".to_string(),
                            field_type: FieldType::String,
                            description: "Type of volume".to_string(),
                            default: Some(Value::String("gp3".to_string())),
                            constraints: vec![FieldConstraint::Enum {
                                values: vec!["gp2".to_string(), "gp3".to_string(), "io1".to_string(), "io2".to_string(), "st1".to_string(), "sc1".to_string(), "standard".to_string()],
                            }],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "encrypted".to_string(),
                            field_type: FieldType::Boolean,
                            description: "Whether to enable volume encryption".to_string(),
                            default: Some(Value::Bool(false)),
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]),
                    description: "Configuration block to customize details about the root block device".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ebs_optimized".to_string(),
                    field_type: FieldType::Boolean,
                    description: "If true, the launched EC2 instance will be EBS-optimized".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "monitoring".to_string(),
                    field_type: FieldType::Boolean,
                    description: "If true, the launched EC2 instance will have detailed monitoring enabled".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Instance ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "ARN of the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "public_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Public IP address assigned to the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Private IP address assigned to the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "public_dns".to_string(),
                    field_type: FieldType::String,
                    description: "Public DNS name assigned to the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_dns".to_string(),
                    field_type: FieldType::String,
                    description: "Private DNS name assigned to the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance_state".to_string(),
                    field_type: FieldType::String,
                    description: "State of the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "ami".to_string(),
                "subnet_id".to_string(),
                "availability_zone".to_string(),
                "tenancy".to_string(),
                "user_data".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 600,
                read: 60,
                update: 600,
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

        match self.describe_instance(&client, id).await? {
            Some(attrs) => {
                // Skip terminated instances
                if attrs.instance_state == "terminated" {
                    return Ok(ResourceReadResult::not_found());
                }

                let attributes = serde_json::to_value(&attrs).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize instance attributes: {}",
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
            None => {
                // Resource doesn't exist, create it
                Ok(ResourceDiff::create(desired.clone()))
            }
            Some(current_val) => {
                // Resource exists, compute diff
                let mut diff = ResourceDiff::no_change();
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                let empty_map = serde_json::Map::new();
                let desired_obj = desired.as_object().unwrap_or(&empty_map);
                let current_obj = current_val.as_object().unwrap_or(&empty_map);

                // Check each field in desired config
                for (key, des_val) in desired_obj {
                    let cur_val = current_obj.get(key);

                    match cur_val {
                        Some(cv) if cv != des_val => {
                            diff.modifications
                                .insert(key.clone(), (cv.clone(), des_val.clone()));

                            if self.requires_replacement_for_field(key) {
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
                for key in current_obj.keys() {
                    if !desired_obj.contains_key(key) && !key.starts_with('_') {
                        // Skip computed fields
                        if !matches!(
                            key.as_str(),
                            "id" | "arn"
                                | "public_ip"
                                | "private_ip"
                                | "public_dns"
                                | "private_dns"
                                | "instance_state"
                                | "vpc_id"
                                | "launch_time"
                        ) {
                            diff.deletions.push(key.clone());
                        }
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
        let instance_config = InstanceConfig::from_value(config)?;
        let client = self.create_client(ctx).await?;

        // Parse instance type
        let instance_type = instance_config
            .instance_type
            .parse::<InstanceType>()
            .map_err(|_| {
                ProvisioningError::ValidationError(format!(
                    "Invalid instance type: {}",
                    instance_config.instance_type
                ))
            })?;

        // Build the RunInstances request
        let mut run_instances = client
            .run_instances()
            .image_id(&instance_config.ami)
            .instance_type(instance_type)
            .min_count(1)
            .max_count(1);

        // Add subnet ID
        if let Some(ref subnet_id) = instance_config.subnet_id {
            run_instances = run_instances.subnet_id(subnet_id);
        }

        // Add security groups
        for sg_id in &instance_config.vpc_security_group_ids {
            run_instances = run_instances.security_group_ids(sg_id);
        }

        // Add key name
        if let Some(ref key_name) = instance_config.key_name {
            run_instances = run_instances.key_name(key_name);
        }

        // Add IAM instance profile
        if let Some(ref profile) = instance_config.iam_instance_profile {
            let profile_spec = if profile.starts_with("arn:") {
                IamInstanceProfileSpecification::builder()
                    .arn(profile)
                    .build()
            } else {
                IamInstanceProfileSpecification::builder()
                    .name(profile)
                    .build()
            };
            run_instances = run_instances.iam_instance_profile(profile_spec);
        }

        // Add user data
        if let Some(ref user_data) = instance_config.user_data {
            let encoded = self.encode_user_data(user_data);
            run_instances = run_instances.user_data(encoded);
        }

        // Add placement configuration
        let mut placement = Placement::builder();
        if let Some(ref az) = instance_config.availability_zone {
            placement = placement.availability_zone(az);
        }
        if let Some(ref tenancy_str) = instance_config.tenancy {
            let tenancy = match tenancy_str.as_str() {
                "dedicated" => Tenancy::Dedicated,
                "host" => Tenancy::Host,
                _ => Tenancy::Default,
            };
            placement = placement.tenancy(tenancy);
        }
        run_instances = run_instances.placement(placement.build());

        // Add EBS optimization and monitoring
        run_instances = run_instances.ebs_optimized(instance_config.ebs_optimized);

        if instance_config.monitoring {
            run_instances = run_instances.monitoring(
                aws_sdk_ec2::types::RunInstancesMonitoringEnabled::builder()
                    .enabled(true)
                    .build(),
            );
        }

        // Add root block device
        if let Some(ref root_block) = instance_config.root_block_device {
            // Determine device name based on AMI type (common defaults)
            let device_name = "/dev/xvda";
            run_instances = run_instances
                .block_device_mappings(root_block.to_block_device_mapping(device_name));
        }

        // Add private IP
        if let Some(ref private_ip) = instance_config.private_ip {
            run_instances = run_instances.private_ip_address(private_ip);
        }

        // Add associate public IP (via network interface if subnet is specified)
        if let Some(associate_public) = instance_config.associate_public_ip_address {
            if instance_config.subnet_id.is_some() {
                // For VPC instances, we need to use network interface
                let mut ni_builder = InstanceNetworkInterfaceSpecification::builder()
                    .device_index(0)
                    .associate_public_ip_address(associate_public);

                if let Some(ref subnet_id) = instance_config.subnet_id {
                    ni_builder = ni_builder.subnet_id(subnet_id);
                }

                for sg_id in &instance_config.vpc_security_group_ids {
                    ni_builder = ni_builder.groups(sg_id);
                }

                if let Some(ref private_ip) = instance_config.private_ip {
                    ni_builder = ni_builder.private_ip_address(private_ip);
                }

                run_instances = run_instances.network_interfaces(ni_builder.build());

                // Remove the top-level security groups and subnet since they're in the NI
                run_instances = run_instances.set_security_group_ids(None);
                run_instances = run_instances.set_subnet_id(None);
            }
        }

        // Add tags
        let mut tags = vec![];
        for (key, value) in &instance_config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }
        // Add default tags from context
        for (key, value) in &ctx.default_tags {
            if !instance_config.tags.contains_key(key) {
                tags.push(Tag::builder().key(key).value(value).build());
            }
        }

        if !tags.is_empty() {
            run_instances = run_instances.tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::Instance)
                    .set_tags(Some(tags))
                    .build(),
            );
        }

        // Execute the request
        let resp = run_instances.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create instance: {}", e))
        })?;

        // Get the instance ID
        let instance = resp.instances().first().ok_or_else(|| {
            ProvisioningError::CloudApiError("No instance returned from RunInstances".to_string())
        })?;

        let instance_id = instance
            .instance_id()
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Instance ID not returned".to_string())
            })?
            .to_string();

        info!("Created EC2 instance: {}", instance_id);

        // Wait for instance to be running
        let timeout = Duration::from_secs(ctx.timeout_seconds);
        let final_attrs = self
            .wait_for_state(&client, &instance_id, "running", timeout)
            .await?;

        let attributes = serde_json::to_value(&final_attrs).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(instance_id, attributes)
            .with_output("id", Value::String(final_attrs.id.clone()))
            .with_output("arn", Value::String(final_attrs.arn.clone()))
            .with_output("public_ip", serde_json::json!(final_attrs.public_ip))
            .with_output("private_ip", serde_json::json!(final_attrs.private_ip)))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Parse configurations
        let old_config = InstanceConfig::from_value(old)?;
        let new_config = InstanceConfig::from_value(new)?;

        // Check for instance type change (requires stop/start)
        if old_config.instance_type != new_config.instance_type {
            info!(
                "Changing instance type from {} to {} (requires stop/start)",
                old_config.instance_type, new_config.instance_type
            );

            // Stop the instance
            client
                .stop_instances()
                .instance_ids(id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to stop instance: {}", e))
                })?;

            // Wait for stopped state
            let timeout = Duration::from_secs(ctx.timeout_seconds);
            self.wait_for_state(&client, id, "stopped", timeout).await?;

            // Modify instance type
            let instance_type = new_config
                .instance_type
                .parse::<InstanceType>()
                .map_err(|_| {
                    ProvisioningError::ValidationError(format!(
                        "Invalid instance type: {}",
                        new_config.instance_type
                    ))
                })?;

            client
                .modify_instance_attribute()
                .instance_id(id)
                .instance_type(
                    aws_sdk_ec2::types::AttributeValue::builder()
                        .value(instance_type.as_str())
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to modify instance type: {}",
                        e
                    ))
                })?;

            // Start the instance
            client
                .start_instances()
                .instance_ids(id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to start instance: {}", e))
                })?;

            // Wait for running state
            self.wait_for_state(&client, id, "running", timeout).await?;
        }

        // Update security groups if changed
        if old_config.vpc_security_group_ids != new_config.vpc_security_group_ids {
            client
                .modify_instance_attribute()
                .instance_id(id)
                .set_groups(Some(new_config.vpc_security_group_ids.clone()))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to modify security groups: {}",
                        e
                    ))
                })?;
        }

        // Update monitoring if changed
        if old_config.monitoring != new_config.monitoring {
            if new_config.monitoring {
                client
                    .monitor_instances()
                    .instance_ids(id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to enable monitoring: {}",
                            e
                        ))
                    })?;
            } else {
                client
                    .unmonitor_instances()
                    .instance_ids(id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to disable monitoring: {}",
                            e
                        ))
                    })?;
            }
        }

        // Update tags if changed
        if old_config.tags != new_config.tags {
            // Delete old tags not in new
            let old_keys: Vec<_> = old_config
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k))
                .cloned()
                .collect();

            if !old_keys.is_empty() {
                let delete_tags: Vec<_> = old_keys
                    .iter()
                    .map(|k| Tag::builder().key(k).build())
                    .collect();
                client
                    .delete_tags()
                    .resources(id)
                    .set_tags(Some(delete_tags))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to delete tags: {}", e))
                    })?;
            }

            // Create/update new tags
            let new_tags: Vec<_> = new_config
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            if !new_tags.is_empty() {
                client
                    .create_tags()
                    .resources(id)
                    .set_tags(Some(new_tags))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to create tags: {}", e))
                    })?;
            }
        }

        // Get updated attributes
        let attrs = self.describe_instance(&client, id).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("Instance not found after update".to_string())
        })?;

        let attributes = serde_json::to_value(&attrs).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        info!("Terminating EC2 instance: {}", id);

        client
            .terminate_instances()
            .instance_ids(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to terminate instance: {}", e))
            })?;

        // Wait for terminated state
        let timeout = Duration::from_secs(ctx.timeout_seconds);
        self.wait_for_state(&client, id, "terminated", timeout)
            .await?;

        info!("Terminated EC2 instance: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        let attrs = self.describe_instance(&client, id).await?.ok_or_else(|| {
            ProvisioningError::ImportError {
                resource_type: "aws_instance".to_string(),
                resource_id: id.to_string(),
                message: "Instance not found".to_string(),
            }
        })?;

        if attrs.instance_state == "terminated" {
            return Err(ProvisioningError::ImportError {
                resource_type: "aws_instance".to_string(),
                resource_id: id.to_string(),
                message: "Cannot import terminated instance".to_string(),
            });
        }

        let attributes = serde_json::to_value(&attrs).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(obj) = config.as_object() {
            // Check subnet_id for references
            if let Some(subnet_id) = obj.get("subnet_id") {
                deps.extend(self.extract_references(subnet_id, "subnet_id"));
            }

            // Check vpc_security_group_ids for references
            if let Some(sg_ids) = obj.get("vpc_security_group_ids") {
                if let Some(arr) = sg_ids.as_array() {
                    for sg_id in arr {
                        deps.extend(self.extract_references(sg_id, "vpc_security_group_ids"));
                    }
                }
            }

            // Check iam_instance_profile for references
            if let Some(profile) = obj.get("iam_instance_profile") {
                deps.extend(self.extract_references(profile, "iam_instance_profile"));
            }

            // Check key_name for references
            if let Some(key) = obj.get("key_name") {
                deps.extend(self.extract_references(key, "key_name"));
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "ami".to_string(),
            "subnet_id".to_string(),
            "availability_zone".to_string(),
            "tenancy".to_string(),
            "user_data".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate required fields
        if !obj.contains_key("ami") {
            return Err(ProvisioningError::ValidationError(
                "ami is required".to_string(),
            ));
        }

        // Validate AMI format
        if let Some(ami) = obj.get("ami").and_then(|v| v.as_str()) {
            if !ami.starts_with("ami-") {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid AMI format: {}. Must start with 'ami-'",
                    ami
                )));
            }
        }

        // Validate subnet_id format if present
        if let Some(subnet) = obj.get("subnet_id").and_then(|v| v.as_str()) {
            // Skip validation if it's a reference
            if !subnet.contains("${") && !subnet.contains("{{") && !subnet.starts_with("subnet-") {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid subnet_id format: {}. Must start with 'subnet-'",
                    subnet
                )));
            }
        }

        // Validate security group IDs format
        if let Some(sgs) = obj.get("vpc_security_group_ids").and_then(|v| v.as_array()) {
            for sg in sgs {
                if let Some(sg_id) = sg.as_str() {
                    // Skip validation if it's a reference
                    if !sg_id.contains("${") && !sg_id.contains("{{") && !sg_id.starts_with("sg-") {
                        return Err(ProvisioningError::ValidationError(format!(
                            "Invalid security group ID format: {}. Must start with 'sg-'",
                            sg_id
                        )));
                    }
                }
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
        let resource = AwsInstanceResource::new();
        assert_eq!(resource.resource_type(), "aws_instance");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsInstanceResource::new();
        let forces = resource.forces_replacement();

        assert!(forces.contains(&"ami".to_string()));
        assert!(forces.contains(&"subnet_id".to_string()));
        assert!(forces.contains(&"availability_zone".to_string()));
        assert!(forces.contains(&"tenancy".to_string()));
        assert!(forces.contains(&"user_data".to_string()));
    }

    #[test]
    fn test_schema_has_required_fields() {
        let resource = AwsInstanceResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_instance");
        assert!(!schema.required_args.is_empty());

        // Check ami is required
        let has_ami = schema.required_args.iter().any(|f| f.name == "ami");
        assert!(has_ami, "ami should be a required field");

        // Check computed attributes
        let computed_names: Vec<_> = schema
            .computed_attrs
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(computed_names.contains(&"id"));
        assert!(computed_names.contains(&"arn"));
        assert!(computed_names.contains(&"public_ip"));
        assert!(computed_names.contains(&"private_ip"));
        assert!(computed_names.contains(&"instance_state"));
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsInstanceResource::new();

        let config = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.micro",
            "subnet_id": "subnet-12345678",
            "vpc_security_group_ids": ["sg-12345678"]
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_ami() {
        let resource = AwsInstanceResource::new();

        let config = json!({
            "instance_type": "t3.micro"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());

        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("ami"));
        }
    }

    #[test]
    fn test_validate_invalid_ami_format() {
        let resource = AwsInstanceResource::new();

        let config = json!({
            "ami": "invalid-ami"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_reference_in_subnet() {
        let resource = AwsInstanceResource::new();

        // References should be allowed (they'll be resolved later)
        let config = json!({
            "ami": "ami-12345678",
            "subnet_id": "${aws_subnet.main.id}"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_instance_config_parsing() {
        let config = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.small",
            "subnet_id": "subnet-12345678",
            "vpc_security_group_ids": ["sg-12345678", "sg-87654321"],
            "key_name": "my-key",
            "tags": {
                "Name": "test-instance",
                "Environment": "test"
            },
            "monitoring": true,
            "ebs_optimized": true
        });

        let instance_config = InstanceConfig::from_value(&config).unwrap();

        assert_eq!(instance_config.ami, "ami-12345678");
        assert_eq!(instance_config.instance_type, "t3.small");
        assert_eq!(
            instance_config.subnet_id,
            Some("subnet-12345678".to_string())
        );
        assert_eq!(instance_config.vpc_security_group_ids.len(), 2);
        assert_eq!(instance_config.key_name, Some("my-key".to_string()));
        assert_eq!(
            instance_config.tags.get("Name"),
            Some(&"test-instance".to_string())
        );
        assert!(instance_config.monitoring);
        assert!(instance_config.ebs_optimized);
    }

    #[test]
    fn test_instance_config_defaults() {
        let config = json!({
            "ami": "ami-12345678"
        });

        let instance_config = InstanceConfig::from_value(&config).unwrap();

        assert_eq!(instance_config.instance_type, "t3.micro");
        assert!(!instance_config.monitoring);
        assert!(!instance_config.ebs_optimized);
        assert!(instance_config.vpc_security_group_ids.is_empty());
        assert!(instance_config.tags.is_empty());
    }

    #[test]
    fn test_root_block_device_parsing() {
        let config = json!({
            "ami": "ami-12345678",
            "root_block_device": {
                "volume_size": 100,
                "volume_type": "gp3",
                "encrypted": true,
                "iops": 3000,
                "throughput": 125
            }
        });

        let instance_config = InstanceConfig::from_value(&config).unwrap();
        let root_block = instance_config.root_block_device.unwrap();

        assert_eq!(root_block.volume_size, Some(100));
        assert_eq!(root_block.volume_type, Some("gp3".to_string()));
        assert_eq!(root_block.encrypted, Some(true));
        assert_eq!(root_block.iops, Some(3000));
        assert_eq!(root_block.throughput, Some(125));
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsInstanceResource::new();

        let config = json!({
            "ami": "ami-12345678",
            "subnet_id": "${aws_subnet.main.id}",
            "vpc_security_group_ids": [
                "${aws_security_group.web.id}",
                "sg-static-12345"
            ]
        });

        let deps = resource.dependencies(&config);

        // Should extract aws_subnet.main and aws_security_group.web
        assert!(!deps.is_empty());

        let has_subnet = deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "main");
        let has_sg = deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "web");

        assert!(has_subnet, "Should detect subnet dependency");
        assert!(has_sg, "Should detect security group dependency");
    }

    #[test]
    fn test_plan_create() {
        let resource = AwsInstanceResource::new();

        let desired = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.micro"
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            // Create a mock context
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
        assert!(diff.additions.contains_key("ami"));
    }

    #[test]
    fn test_plan_no_change() {
        let resource = AwsInstanceResource::new();

        let config = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.micro"
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
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_plan_update_instance_type() {
        let resource = AwsInstanceResource::new();

        let current = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.micro"
        });

        let desired = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.small"
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

        assert_eq!(diff.change_type, ChangeType::Update);
        assert!(diff.modifications.contains_key("instance_type"));
        assert!(!diff.requires_replacement);
    }

    #[test]
    fn test_plan_replace_ami_change() {
        let resource = AwsInstanceResource::new();

        let current = json!({
            "ami": "ami-12345678",
            "instance_type": "t3.micro"
        });

        let desired = json!({
            "ami": "ami-87654321",
            "instance_type": "t3.micro"
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
        assert!(diff.replacement_fields.contains(&"ami".to_string()));
    }

    #[test]
    fn test_build_arn() {
        let resource = AwsInstanceResource::new();
        let arn = resource.build_arn("i-12345678", "us-east-1", Some("123456789012"));
        assert_eq!(
            arn,
            "arn:aws:ec2:us-east-1:123456789012:instance/i-12345678"
        );
    }

    #[test]
    fn test_encode_user_data_plain_text() {
        let resource = AwsInstanceResource::new();
        let user_data = "#!/bin/bash\necho 'Hello World'";
        let encoded = resource.encode_user_data(user_data);

        // Should be base64 encoded
        assert_ne!(encoded, user_data);

        // Verify it decodes correctly
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), user_data);
    }
}
