//! AWS Auto Scaling Group Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsAutoScalingGroupResource` which implements the `Resource` trait
//! for managing AWS Auto Scaling Groups declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_autoscaling_group:
//!     web_asg:
//!       name: web-servers
//!       min_size: 1
//!       max_size: 10
//!       desired_capacity: 2
//!       launch_template:
//!         id: ${aws_launch_template.web.id}
//!         version: "$Latest"
//!       vpc_zone_identifier:
//!         - ${aws_subnet.private_a.id}
//!         - ${aws_subnet.private_b.id}
//!       target_group_arns:
//!         - ${aws_lb_target_group.web.arn}
//!       health_check_type: ELB
//!       health_check_grace_period: 300
//!       termination_policies:
//!         - OldestInstance
//!         - Default
//!       tags:
//!         Name: web-server
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_autoscaling::types::{LaunchTemplateSpecification, Tag as AsgTag, TagDescription};
use aws_sdk_autoscaling::Client;
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

/// Launch template specification for the Auto Scaling Group
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaunchTemplateSpec {
    /// Launch template ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Launch template name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Launch template version ($Latest, $Default, or specific version number)
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "$Default".to_string()
}

/// Mixed instances policy for the Auto Scaling Group
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MixedInstancesPolicy {
    /// Launch template specification
    pub launch_template: LaunchTemplateSpec,
    /// Override instance types
    #[serde(default)]
    pub overrides: Vec<InstanceTypeOverride>,
    /// On-demand allocation strategy: prioritized, lowest-price
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_allocation_strategy: Option<String>,
    /// Base capacity fulfilled by on-demand instances
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_base_capacity: Option<i32>,
    /// Percentage above base capacity fulfilled by on-demand
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_percentage_above_base_capacity: Option<i32>,
    /// Spot allocation strategy: lowest-price, capacity-optimized, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_allocation_strategy: Option<String>,
    /// Number of Spot pools per availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_instance_pools: Option<i32>,
    /// Maximum price for Spot instances
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_max_price: Option<String>,
}

/// Instance type override for mixed instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceTypeOverride {
    /// Instance type
    pub instance_type: String,
    /// Weighted capacity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighted_capacity: Option<String>,
}

/// Instance refresh configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceRefresh {
    /// Refresh strategy (Rolling)
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// Minimum healthy percentage during refresh
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_healthy_percentage: Option<i32>,
    /// Instance warmup seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_warmup: Option<i32>,
    /// Skip matching
    #[serde(default)]
    pub skip_matching: bool,
    /// Auto rollback
    #[serde(default = "default_true")]
    pub auto_rollback: bool,
    /// Checkpoint delay in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_delay: Option<i32>,
    /// Checkpoint percentages
    #[serde(default)]
    pub checkpoint_percentages: Vec<i32>,
}

fn default_strategy() -> String {
    "Rolling".to_string()
}

fn default_true() -> bool {
    true
}

/// Warm pool configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WarmPool {
    /// Pool state: Stopped, Running, Hibernated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_state: Option<String>,
    /// Minimum pool size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i32>,
    /// Maximum group prepared capacity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_group_prepared_capacity: Option<i32>,
    /// Instance reuse policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reuse_on_scale_in: Option<bool>,
}

/// ASG tag with propagate_at_launch option
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsgTagConfig {
    /// Tag key
    pub key: String,
    /// Tag value
    pub value: String,
    /// Propagate tag to launched instances
    #[serde(default = "default_true")]
    pub propagate_at_launch: bool,
}

// ============================================================================
// Resource Configuration
// ============================================================================

/// Configuration for AWS Auto Scaling Group
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoScalingGroupConfig {
    /// Name of the Auto Scaling Group (forces replacement if changed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Name prefix for the Auto Scaling Group (forces replacement if changed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_prefix: Option<String>,
    /// Minimum size of the group
    pub min_size: i32,
    /// Maximum size of the group
    pub max_size: i32,
    /// Desired capacity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_capacity: Option<i32>,
    /// Launch template specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template: Option<LaunchTemplateSpec>,
    /// Launch configuration name (deprecated, use launch_template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_configuration: Option<String>,
    /// Mixed instances policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixed_instances_policy: Option<MixedInstancesPolicy>,
    /// VPC subnets for launching instances
    #[serde(default)]
    pub vpc_zone_identifier: Vec<String>,
    /// Availability zones (if not using VPC)
    #[serde(default)]
    pub availability_zones: Vec<String>,
    /// Health check type: EC2 or ELB
    #[serde(default = "default_health_check_type")]
    pub health_check_type: String,
    /// Grace period for health checks in seconds
    #[serde(default = "default_health_check_grace_period")]
    pub health_check_grace_period: i32,
    /// Target group ARNs for ALB/NLB
    #[serde(default)]
    pub target_group_arns: Vec<String>,
    /// Classic load balancer names
    #[serde(default)]
    pub load_balancers: Vec<String>,
    /// Default cooldown period in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cooldown: Option<i32>,
    /// Termination policies
    #[serde(default)]
    pub termination_policies: Vec<String>,
    /// Whether capacity rebalancing is enabled
    #[serde(default)]
    pub capacity_rebalance: bool,
    /// Placement group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_group: Option<String>,
    /// Service-linked role ARN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_linked_role_arn: Option<String>,
    /// Maximum instance lifetime in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_lifetime: Option<i32>,
    /// Enabled metrics for CloudWatch
    #[serde(default)]
    pub enabled_metrics: Vec<String>,
    /// Metrics granularity (1Minute)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_granularity: Option<String>,
    /// Suspended processes
    #[serde(default)]
    pub suspended_processes: Vec<String>,
    /// Wait for capacity timeout (wait for instances to be healthy)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for_capacity_timeout: Option<String>,
    /// Minimum number of instances in service during activities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_elb_capacity: Option<i32>,
    /// Wait for ELB capacity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for_elb_capacity: Option<i32>,
    /// Instance refresh configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_refresh: Option<InstanceRefresh>,
    /// Warm pool configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warm_pool: Option<WarmPool>,
    /// Force delete during destroy (don't wait for instances)
    #[serde(default)]
    pub force_delete: bool,
    /// Force delete warm pool
    #[serde(default)]
    pub force_delete_warm_pool: bool,
    /// Protect instances from scale-in
    #[serde(default)]
    pub protect_from_scale_in: bool,
    /// Tags for the Auto Scaling Group
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Tags with propagate_at_launch option
    #[serde(default)]
    pub tag: Vec<AsgTagConfig>,
}

fn default_health_check_type() -> String {
    "EC2".to_string()
}

fn default_health_check_grace_period() -> i32 {
    300
}

impl AutoScalingGroupConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid Auto Scaling Group configuration: {}",
                e
            ))
        })
    }
}

// ============================================================================
// Resource State
// ============================================================================

/// State of an AWS Auto Scaling Group
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoScalingGroupState {
    /// ID (same as name for ASG)
    pub id: String,
    /// ARN of the Auto Scaling Group
    pub arn: String,
    /// Name of the Auto Scaling Group
    pub name: String,
    /// Minimum size
    pub min_size: i32,
    /// Maximum size
    pub max_size: i32,
    /// Desired capacity
    pub desired_capacity: i32,
    /// Default cooldown
    pub default_cooldown: i32,
    /// Launch template in use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template: Option<LaunchTemplateSpec>,
    /// Launch configuration name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_configuration: Option<String>,
    /// Availability zones
    pub availability_zones: Vec<String>,
    /// VPC zone identifier (comma-separated subnet IDs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_zone_identifier: Option<String>,
    /// Health check type
    pub health_check_type: String,
    /// Health check grace period
    pub health_check_grace_period: i32,
    /// Target group ARNs
    pub target_group_arns: Vec<String>,
    /// Load balancer names
    pub load_balancers: Vec<String>,
    /// Termination policies
    pub termination_policies: Vec<String>,
    /// Capacity rebalance enabled
    pub capacity_rebalance: bool,
    /// Placement group
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_group: Option<String>,
    /// Service-linked role ARN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_linked_role_arn: Option<String>,
    /// Max instance lifetime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_lifetime: Option<i32>,
    /// Protected from scale in
    pub protect_from_scale_in: bool,
    /// ASG status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Created time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_time: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

// ============================================================================
// Resource Implementation
// ============================================================================

/// AWS Auto Scaling Group Resource
#[derive(Debug, Clone, Default)]
pub struct AwsAutoScalingGroupResource;

impl AwsAutoScalingGroupResource {
    /// Create a new Auto Scaling Group resource handler
    pub fn new() -> Self {
        Self
    }

    /// Create an Auto Scaling client
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config_loader = aws_config::defaults(BehaviorVersion::latest());
        let config = if let Some(r) = &ctx.region {
            config_loader
                .region(aws_config::Region::new(r.to_string()))
                .load()
                .await
        } else {
            config_loader.load().await
        };

        Ok(Client::new(&config))
    }

    /// Describe an Auto Scaling Group by name
    async fn describe_group(
        &self,
        client: &Client,
        name: &str,
    ) -> ProvisioningResult<Option<AutoScalingGroupState>> {
        let result = client
            .describe_auto_scaling_groups()
            .auto_scaling_group_names(name)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!(
                    "Failed to describe Auto Scaling Group '{}': {}",
                    name, e
                ))
            })?;

        let asg = match result.auto_scaling_groups.into_iter().flatten().next() {
            Some(g) => g,
            None => return Ok(None),
        };

        let launch_template = asg.launch_template().map(|lt| LaunchTemplateSpec {
            id: lt.launch_template_id().map(|s| s.to_string()),
            name: lt.launch_template_name().map(|s| s.to_string()),
            version: lt.version().unwrap_or("$Default").to_string(),
        });

        let asg_name = asg
            .auto_scaling_group_name()
            .unwrap_or_default()
            .to_string();
        // Convert health check type enum to string using Debug format
        // The enum variants are Ec2, Elb, VpcLattice - convert to uppercase
        let health_type = asg
            .health_check_type()
            .map(|t| {
                let s = format!("{:?}", t);
                // Handle common cases
                match s.as_str() {
                    "Ec2" => "EC2".to_string(),
                    "Elb" => "ELB".to_string(),
                    "VpcLattice" => "VPC_LATTICE".to_string(),
                    _ => s.to_uppercase(),
                }
            })
            .unwrap_or_else(|| "EC2".to_string());

        let state = AutoScalingGroupState {
            id: asg_name.clone(),
            arn: asg.auto_scaling_group_arn().unwrap_or_default().to_string(),
            name: asg_name,
            min_size: asg.min_size().unwrap_or(0),
            max_size: asg.max_size().unwrap_or(0),
            desired_capacity: asg.desired_capacity().unwrap_or(0),
            default_cooldown: asg.default_cooldown().unwrap_or(300),
            launch_template,
            launch_configuration: asg.launch_configuration_name().map(|s| s.to_string()),
            availability_zones: asg
                .availability_zones()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            vpc_zone_identifier: asg.vpc_zone_identifier().map(|s| s.to_string()),
            health_check_type: health_type,
            health_check_grace_period: asg.health_check_grace_period().unwrap_or(300),
            target_group_arns: asg
                .target_group_arns()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            load_balancers: asg
                .load_balancer_names()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            termination_policies: asg
                .termination_policies()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            capacity_rebalance: asg.capacity_rebalance().unwrap_or(false),
            placement_group: asg.placement_group().map(|s| s.to_string()),
            service_linked_role_arn: asg.service_linked_role_arn().map(|s| s.to_string()),
            max_instance_lifetime: asg.max_instance_lifetime(),
            protect_from_scale_in: asg.new_instances_protected_from_scale_in().unwrap_or(false),
            status: asg.status().map(|s| s.to_string()),
            created_time: asg.created_time().map(|t| t.to_string()),
            tags: self.tags_to_hashmap(asg.tags()),
        };

        Ok(Some(state))
    }

    /// Extract resource references from configuration
    fn extract_references(&self, config: &Value) -> Vec<String> {
        let mut refs = Vec::new();

        // Launch template references
        if let Some(lt) = config.get("launch_template") {
            if let Some(id) = lt.get("id").and_then(|v| v.as_str()) {
                if id.starts_with("${") {
                    refs.push(id.to_string());
                }
            }
            if let Some(name) = lt.get("name").and_then(|v| v.as_str()) {
                if name.starts_with("${") {
                    refs.push(name.to_string());
                }
            }
        }

        // VPC zone identifier (subnets)
        if let Some(subnets) = config.get("vpc_zone_identifier").and_then(|v| v.as_array()) {
            for subnet in subnets {
                if let Some(s) = subnet.as_str() {
                    if s.starts_with("${") {
                        refs.push(s.to_string());
                    }
                }
            }
        }

        // Target group ARNs
        if let Some(tgs) = config.get("target_group_arns").and_then(|v| v.as_array()) {
            for tg in tgs {
                if let Some(s) = tg.as_str() {
                    if s.starts_with("${") {
                        refs.push(s.to_string());
                    }
                }
            }
        }

        // Load balancers
        if let Some(lbs) = config.get("load_balancers").and_then(|v| v.as_array()) {
            for lb in lbs {
                if let Some(s) = lb.as_str() {
                    if s.starts_with("${") {
                        refs.push(s.to_string());
                    }
                }
            }
        }

        // Service-linked role
        if let Some(role) = config
            .get("service_linked_role_arn")
            .and_then(|v| v.as_str())
        {
            if role.starts_with("${") {
                refs.push(role.to_string());
            }
        }

        // Placement group
        if let Some(pg) = config.get("placement_group").and_then(|v| v.as_str()) {
            if pg.starts_with("${") {
                refs.push(pg.to_string());
            }
        }

        refs
    }

    /// Parse a resource reference to extract type and name
    fn parse_reference(&self, reference: &str) -> Option<(String, String, String)> {
        // Format: ${resource_type.resource_name.attribute}
        let trimmed = reference.trim_start_matches("${").trim_end_matches('}');
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() >= 3 {
            Some((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2..].join("."),
            ))
        } else {
            None
        }
    }

    /// Build tags for API calls
    fn build_tags(&self, config: &AutoScalingGroupConfig, asg_name: &str) -> Vec<AsgTag> {
        let mut tags = Vec::new();

        // Add tags from the tags map
        for (key, value) in &config.tags {
            tags.push(
                AsgTag::builder()
                    .key(key)
                    .value(value)
                    .propagate_at_launch(true)
                    .resource_id(asg_name)
                    .resource_type("auto-scaling-group")
                    .build(),
            );
        }

        // Add tags from the tag list
        for tag_config in &config.tag {
            tags.push(
                AsgTag::builder()
                    .key(&tag_config.key)
                    .value(&tag_config.value)
                    .propagate_at_launch(tag_config.propagate_at_launch)
                    .resource_id(asg_name)
                    .resource_type("auto-scaling-group")
                    .build(),
            );
        }

        tags
    }

    /// Convert TagDescription to HashMap
    fn tags_to_hashmap(&self, tags: &[TagDescription]) -> HashMap<String, String> {
        tags.iter()
            .filter_map(|t| {
                let key = t.key()?;
                let value = t.value()?;
                Some((key.to_string(), value.to_string()))
            })
            .collect()
    }

    /// Generate a unique name with prefix
    fn generate_name(&self, prefix: &str) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("{}{:x}", prefix, timestamp)
    }
}

#[async_trait]
impl Resource for AwsAutoScalingGroupResource {
    fn resource_type(&self) -> &str {
        "aws_autoscaling_group"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_autoscaling_group".to_string(),
            description: "Provides an Auto Scaling Group resource.".to_string(),
            required_args: vec![
                SchemaField {
                    name: "min_size".to_string(),
                    field_type: FieldType::Integer,
                    description: "Minimum size of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinValue { value: 0 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "max_size".to_string(),
                    field_type: FieldType::Integer,
                    description: "Maximum size of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinValue { value: 0 }],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 1 },
                        FieldConstraint::MaxLength { max: 255 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "name_prefix".to_string(),
                    field_type: FieldType::String,
                    description: "Creates a unique name beginning with the specified prefix"
                        .to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MaxLength { max: 255 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "desired_capacity".to_string(),
                    field_type: FieldType::Integer,
                    description: "Desired capacity of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinValue { value: 0 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "launch_template".to_string(),
                    field_type: FieldType::Object(vec![]),
                    description: "Nested block containing launch template settings".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "launch_configuration".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the launch configuration to use".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_zone_identifier".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of subnet IDs to launch resources in".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "availability_zones".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of availability zones to launch resources in".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "health_check_type".to_string(),
                    field_type: FieldType::String,
                    description: "EC2 or ELB".to_string(),
                    default: Some(Value::String("EC2".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec!["EC2".to_string(), "ELB".to_string()],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "health_check_grace_period".to_string(),
                    field_type: FieldType::Integer,
                    description: "Time after instance launch before checking health".to_string(),
                    default: Some(Value::Number(300.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 0 },
                        FieldConstraint::MaxValue { value: 7200 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "target_group_arns".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Set of target group ARNs".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "load_balancers".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of Classic Load Balancer names".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "default_cooldown".to_string(),
                    field_type: FieldType::Integer,
                    description: "Time between a scaling activity and next scaling".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 0 },
                        FieldConstraint::MaxValue { value: 86400 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "termination_policies".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of termination policies".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "capacity_rebalance".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable capacity rebalancing".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "placement_group".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the placement group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "max_instance_lifetime".to_string(),
                    field_type: FieldType::Integer,
                    description: "Maximum amount of time an instance can be in service".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 86400 },
                        FieldConstraint::MaxValue { value: 31536000 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "force_delete".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Allows deleting the ASG without waiting".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "protect_from_scale_in".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Protect instances from scale-in".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to assign to the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "The Auto Scaling Group name (same as name)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    description: "Status of the Auto Scaling Group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "name_prefix".to_string()],
            timeouts: ResourceTimeouts {
                create: 600,
                read: 60,
                update: 600,
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

        match self.describe_group(&client, id).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize Auto Scaling Group state: {}",
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

                // Check for deletions (ignore computed fields)
                let computed_fields = [
                    "id",
                    "arn",
                    "status",
                    "created_time",
                    "availability_zones",
                    "vpc_zone_identifier",
                ];
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
        let client = self.create_client(ctx).await?;
        let parsed_config = AutoScalingGroupConfig::from_value(config)?;

        // Determine the name
        let asg_name = parsed_config.name.clone().unwrap_or_else(|| {
            parsed_config
                .name_prefix
                .as_ref()
                .map(|p| self.generate_name(p))
                .unwrap_or_else(|| self.generate_name("asg-"))
        });

        info!("Creating Auto Scaling Group: {}", asg_name);

        // Build the create request
        let mut request = client
            .create_auto_scaling_group()
            .auto_scaling_group_name(&asg_name)
            .min_size(parsed_config.min_size)
            .max_size(parsed_config.max_size);

        // Desired capacity
        if let Some(dc) = parsed_config.desired_capacity {
            request = request.desired_capacity(dc);
        }

        // Launch template
        if let Some(lt) = &parsed_config.launch_template {
            let mut lt_spec = LaunchTemplateSpecification::builder().version(&lt.version);

            if let Some(id) = &lt.id {
                lt_spec = lt_spec.launch_template_id(id);
            }
            if let Some(name) = &lt.name {
                lt_spec = lt_spec.launch_template_name(name);
            }

            request = request.launch_template(lt_spec.build());
        }

        // Launch configuration (deprecated but supported)
        if let Some(lc) = &parsed_config.launch_configuration {
            request = request.launch_configuration_name(lc);
        }

        // VPC zone identifier
        if !parsed_config.vpc_zone_identifier.is_empty() {
            request = request.vpc_zone_identifier(parsed_config.vpc_zone_identifier.join(","));
        }

        // Availability zones
        if !parsed_config.availability_zones.is_empty() {
            request =
                request.set_availability_zones(Some(parsed_config.availability_zones.clone()));
        }

        // Health check
        request = request
            .health_check_type(&parsed_config.health_check_type)
            .health_check_grace_period(parsed_config.health_check_grace_period);

        // Target group ARNs
        if !parsed_config.target_group_arns.is_empty() {
            request = request.set_target_group_arns(Some(parsed_config.target_group_arns.clone()));
        }

        // Load balancers
        if !parsed_config.load_balancers.is_empty() {
            request = request.set_load_balancer_names(Some(parsed_config.load_balancers.clone()));
        }

        // Default cooldown
        if let Some(cooldown) = parsed_config.default_cooldown {
            request = request.default_cooldown(cooldown);
        }

        // Termination policies
        if !parsed_config.termination_policies.is_empty() {
            request =
                request.set_termination_policies(Some(parsed_config.termination_policies.clone()));
        }

        // Capacity rebalance
        request = request.capacity_rebalance(parsed_config.capacity_rebalance);

        // Placement group
        if let Some(pg) = &parsed_config.placement_group {
            request = request.placement_group(pg);
        }

        // Service-linked role
        if let Some(role) = &parsed_config.service_linked_role_arn {
            request = request.service_linked_role_arn(role);
        }

        // Max instance lifetime
        if let Some(mil) = parsed_config.max_instance_lifetime {
            request = request.max_instance_lifetime(mil);
        }

        // Scale-in protection
        request =
            request.new_instances_protected_from_scale_in(parsed_config.protect_from_scale_in);

        // Tags
        let tags = self.build_tags(&parsed_config, &asg_name);
        if !tags.is_empty() {
            request = request.set_tags(Some(tags));
        }

        // Send the create request
        request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create Auto Scaling Group: {}", e))
        })?;

        // Enable metrics if specified
        if !parsed_config.enabled_metrics.is_empty() {
            let granularity = parsed_config
                .metrics_granularity
                .clone()
                .unwrap_or_else(|| "1Minute".to_string());

            client
                .enable_metrics_collection()
                .auto_scaling_group_name(&asg_name)
                .set_metrics(Some(parsed_config.enabled_metrics.clone()))
                .granularity(&granularity)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to enable metrics: {}", e))
                })?;
        }

        // Suspend processes if specified
        if !parsed_config.suspended_processes.is_empty() {
            client
                .suspend_processes()
                .auto_scaling_group_name(&asg_name)
                .set_scaling_processes(Some(parsed_config.suspended_processes.clone()))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to suspend processes: {}", e))
                })?;
        }

        // Read back the created group
        let created_state = self
            .describe_group(&client, &asg_name)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError(format!(
                    "Auto Scaling Group '{}' not found after creation",
                    asg_name
                ))
            })?;

        let state_value = serde_json::to_value(&created_state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize Auto Scaling Group state: {}",
                e
            ))
        })?;

        info!("Created Auto Scaling Group: {}", asg_name);

        Ok(ResourceResult::success(&asg_name, state_value))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;
        let parsed_config = AutoScalingGroupConfig::from_value(new)?;

        info!("Updating Auto Scaling Group: {}", id);

        // Build the update request
        let mut request = client
            .update_auto_scaling_group()
            .auto_scaling_group_name(id)
            .min_size(parsed_config.min_size)
            .max_size(parsed_config.max_size);

        // Desired capacity
        if let Some(dc) = parsed_config.desired_capacity {
            request = request.desired_capacity(dc);
        }

        // Launch template
        if let Some(lt) = &parsed_config.launch_template {
            let mut lt_spec = LaunchTemplateSpecification::builder().version(&lt.version);

            if let Some(lt_id) = &lt.id {
                lt_spec = lt_spec.launch_template_id(lt_id);
            }
            if let Some(lt_name) = &lt.name {
                lt_spec = lt_spec.launch_template_name(lt_name);
            }

            request = request.launch_template(lt_spec.build());
        }

        // VPC zone identifier
        if !parsed_config.vpc_zone_identifier.is_empty() {
            request = request.vpc_zone_identifier(parsed_config.vpc_zone_identifier.join(","));
        }

        // Health check
        request = request
            .health_check_type(&parsed_config.health_check_type)
            .health_check_grace_period(parsed_config.health_check_grace_period);

        // Default cooldown
        if let Some(cooldown) = parsed_config.default_cooldown {
            request = request.default_cooldown(cooldown);
        }

        // Termination policies
        if !parsed_config.termination_policies.is_empty() {
            request =
                request.set_termination_policies(Some(parsed_config.termination_policies.clone()));
        }

        // Capacity rebalance
        request = request.capacity_rebalance(parsed_config.capacity_rebalance);

        // Placement group
        if let Some(pg) = &parsed_config.placement_group {
            request = request.placement_group(pg);
        }

        // Max instance lifetime
        if let Some(mil) = parsed_config.max_instance_lifetime {
            request = request.max_instance_lifetime(mil);
        }

        // Scale-in protection
        request =
            request.new_instances_protected_from_scale_in(parsed_config.protect_from_scale_in);

        // Send the update request
        request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to update Auto Scaling Group: {}", e))
        })?;

        // Update target groups (need separate API calls)
        // First, get current state
        let current = self.describe_group(&client, id).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError(format!("Auto Scaling Group '{}' not found", id))
        })?;

        let current_target_groups = &current.target_group_arns;

        // Detach removed target groups
        for tg in current_target_groups {
            if !parsed_config.target_group_arns.contains(tg) {
                client
                    .detach_load_balancer_target_groups()
                    .auto_scaling_group_name(id)
                    .target_group_arns(tg)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to detach target group: {}",
                            e
                        ))
                    })?;
            }
        }

        // Attach new target groups
        for tg in &parsed_config.target_group_arns {
            if !current_target_groups.contains(tg) {
                client
                    .attach_load_balancer_target_groups()
                    .auto_scaling_group_name(id)
                    .target_group_arns(tg)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to attach target group: {}",
                            e
                        ))
                    })?;
            }
        }

        // Update tags - delete old tags first, then create new ones
        let current_tags = &current.tags;

        // Delete tags that are no longer present
        let tags_to_delete: Vec<AsgTag> = current_tags
            .keys()
            .filter(|k| !parsed_config.tags.contains_key(*k))
            .map(|k| {
                AsgTag::builder()
                    .key(k)
                    .resource_id(id)
                    .resource_type("auto-scaling-group")
                    .build()
            })
            .collect();

        if !tags_to_delete.is_empty() {
            client
                .delete_tags()
                .set_tags(Some(tags_to_delete))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to delete tags: {}", e))
                })?;
        }

        // Create/update tags
        let new_tags = self.build_tags(&parsed_config, id);
        if !new_tags.is_empty() {
            client
                .create_or_update_tags()
                .set_tags(Some(new_tags))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                })?;
        }

        // Read back the updated group
        let updated_state = self.describe_group(&client, id).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError(format!(
                "Auto Scaling Group '{}' not found after update",
                id
            ))
        })?;

        let state_value = serde_json::to_value(&updated_state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize Auto Scaling Group state: {}",
                e
            ))
        })?;

        info!("Updated Auto Scaling Group: {}", id);

        Ok(ResourceResult::success(id, state_value))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Check if ASG exists
        if self.describe_group(&client, id).await?.is_none() {
            return Ok(ResourceResult::success(id, Value::Null));
        }

        info!("Destroying Auto Scaling Group: {}", id);

        // First, set min/max/desired to 0 to terminate instances
        client
            .update_auto_scaling_group()
            .auto_scaling_group_name(id)
            .min_size(0)
            .max_size(0)
            .desired_capacity(0)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!(
                    "Failed to scale down Auto Scaling Group: {}",
                    e
                ))
            })?;

        // Delete the group (with force delete)
        client
            .delete_auto_scaling_group()
            .auto_scaling_group_name(id)
            .force_delete(true)
            .send()
            .await
            .map_err(|e| {
                // Check if already deleted
                let err_str = e.to_string();
                if err_str.contains("not found") || err_str.contains("does not exist") {
                    return Ok(());
                }
                Err(ProvisioningError::CloudApiError(format!(
                    "Failed to delete Auto Scaling Group: {}",
                    e
                )))
            })
            .ok();

        info!("Destroyed Auto Scaling Group: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let result = self.read(id, ctx).await?;
        if result.exists {
            info!("Imported Auto Scaling Group: {}", id);
            Ok(ResourceResult::success(id, result.attributes))
        } else {
            Err(ProvisioningError::resource_not_found(
                "aws",
                "autoscaling_group",
            ))
        }
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let refs = self.extract_references(config);
        refs.iter()
            .filter_map(|r| {
                self.parse_reference(r)
                    .map(|(res_type, res_name, attr)| ResourceDependency {
                        resource_type: res_type,
                        resource_name: res_name,
                        attribute: attr,
                        hard: true,
                    })
            })
            .collect()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "name_prefix".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate min_size
        let min_size = obj
            .get("min_size")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("min_size is required".to_string())
            })?;

        if min_size < 0 {
            return Err(ProvisioningError::ValidationError(
                "min_size must be >= 0".to_string(),
            ));
        }

        // Validate max_size
        let max_size = obj
            .get("max_size")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("max_size is required".to_string())
            })?;

        if max_size < 0 {
            return Err(ProvisioningError::ValidationError(
                "max_size must be >= 0".to_string(),
            ));
        }

        // Validate min/max relationship
        if min_size > max_size {
            return Err(ProvisioningError::ValidationError(
                "min_size cannot be greater than max_size".to_string(),
            ));
        }

        // Validate desired_capacity if provided
        if let Some(dc) = obj.get("desired_capacity").and_then(|v| v.as_i64()) {
            if dc < min_size {
                return Err(ProvisioningError::ValidationError(
                    "desired_capacity cannot be less than min_size".to_string(),
                ));
            }
            if dc > max_size {
                return Err(ProvisioningError::ValidationError(
                    "desired_capacity cannot be greater than max_size".to_string(),
                ));
            }
        }

        // Validate that launch_template, launch_configuration, or mixed_instances_policy exists
        let has_lt = obj.contains_key("launch_template");
        let has_lc = obj.contains_key("launch_configuration");
        let has_mip = obj.contains_key("mixed_instances_policy");

        if !has_lt && !has_lc && !has_mip {
            return Err(ProvisioningError::ValidationError(
                "One of launch_template, launch_configuration, or mixed_instances_policy is required"
                    .to_string(),
            ));
        }

        // Validate launch_template if provided
        if let Some(lt) = obj.get("launch_template") {
            if let Some(lt_obj) = lt.as_object() {
                if !lt_obj.contains_key("id") && !lt_obj.contains_key("name") {
                    return Err(ProvisioningError::ValidationError(
                        "launch_template requires either id or name".to_string(),
                    ));
                }
            }
        }

        // Validate vpc_zone_identifier or availability_zones
        let has_subnets = obj
            .get("vpc_zone_identifier")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_azs = obj
            .get("availability_zones")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);

        if !has_subnets && !has_azs {
            return Err(ProvisioningError::ValidationError(
                "Either vpc_zone_identifier or availability_zones must be specified".to_string(),
            ));
        }

        // Validate health_check_type
        if let Some(hct) = obj.get("health_check_type").and_then(|v| v.as_str()) {
            if !["EC2", "ELB"].contains(&hct) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid health_check_type '{}', must be EC2 or ELB",
                    hct
                )));
            }
        }

        // Validate health_check_grace_period
        if let Some(hcgp) = obj
            .get("health_check_grace_period")
            .and_then(|v| v.as_i64())
        {
            if hcgp < 0 || hcgp > 7200 {
                return Err(ProvisioningError::ValidationError(
                    "health_check_grace_period must be between 0 and 7200".to_string(),
                ));
            }
        }

        // Validate max_instance_lifetime
        if let Some(mil) = obj.get("max_instance_lifetime").and_then(|v| v.as_i64()) {
            if mil != 0 && (mil < 86400 || mil > 31536000) {
                return Err(ProvisioningError::ValidationError(
                    "max_instance_lifetime must be 0 or between 86400 and 31536000".to_string(),
                ));
            }
        }

        // Validate termination_policies
        if let Some(policies) = obj.get("termination_policies").and_then(|v| v.as_array()) {
            let valid_policies = [
                "Default",
                "AllocationStrategy",
                "OldestInstance",
                "NewestInstance",
                "OldestLaunchConfiguration",
                "OldestLaunchTemplate",
                "ClosestToNextInstanceHour",
            ];
            for policy in policies {
                if let Some(p) = policy.as_str() {
                    if !valid_policies.contains(&p) {
                        return Err(ProvisioningError::ValidationError(format!(
                            "Invalid termination_policy '{}'",
                            p
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
    use crate::provisioning::traits::{DebugCredentials, RetryConfig};
    use serde_json::json;
    use std::sync::Arc;

    fn make_test_ctx() -> ProviderContext {
        ProviderContext {
            provider: "aws".to_string(),
            region: Some("us-east-1".to_string()),
            config: Value::Null,
            credentials: Arc::new(DebugCredentials::new("aws")),
            timeout_seconds: 600,
            retry_config: RetryConfig::default(),
            default_tags: HashMap::new(),
        }
    }

    #[test]
    fn test_resource_type() {
        let resource = AwsAutoScalingGroupResource::new();
        assert_eq!(resource.resource_type(), "aws_autoscaling_group");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_asg_config_parsing() {
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "desired_capacity": 2,
            "launch_template": {
                "id": "lt-12345678",
                "version": "$Latest"
            },
            "vpc_zone_identifier": ["subnet-1", "subnet-2"],
            "health_check_type": "ELB",
            "health_check_grace_period": 300
        });

        let parsed: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();
        assert_eq!(parsed.name, Some("test-asg".to_string()));
        assert_eq!(parsed.min_size, 1);
        assert_eq!(parsed.max_size, 10);
        assert_eq!(parsed.desired_capacity, Some(2));
        assert!(parsed.launch_template.is_some());
        assert_eq!(parsed.vpc_zone_identifier.len(), 2);
        assert_eq!(parsed.health_check_type, "ELB");
    }

    #[test]
    fn test_state_serialization() {
        let state = AutoScalingGroupState {
            id: "test-asg".to_string(),
            arn: "arn:aws:autoscaling:us-east-1:123456789012:autoScalingGroup:test".to_string(),
            name: "test-asg".to_string(),
            min_size: 1,
            max_size: 10,
            desired_capacity: 2,
            default_cooldown: 300,
            launch_template: Some(LaunchTemplateSpec {
                id: Some("lt-12345678".to_string()),
                name: None,
                version: "$Latest".to_string(),
            }),
            launch_configuration: None,
            availability_zones: vec!["us-east-1a".to_string(), "us-east-1b".to_string()],
            vpc_zone_identifier: Some("subnet-1,subnet-2".to_string()),
            health_check_type: "ELB".to_string(),
            health_check_grace_period: 300,
            target_group_arns: vec!["arn:aws:elasticloadbalancing:...".to_string()],
            load_balancers: vec![],
            termination_policies: vec!["Default".to_string()],
            capacity_rebalance: false,
            placement_group: None,
            service_linked_role_arn: None,
            max_instance_lifetime: None,
            protect_from_scale_in: false,
            status: None,
            created_time: None,
            tags: HashMap::new(),
        };

        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["name"], "test-asg");
        assert_eq!(value["min_size"], 1);
        assert_eq!(value["max_size"], 10);
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsAutoScalingGroupResource::new();
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "${aws_launch_template.web.id}",
                "version": "$Latest"
            },
            "vpc_zone_identifier": [
                "${aws_subnet.private_a.id}",
                "${aws_subnet.private_b.id}"
            ],
            "target_group_arns": [
                "${aws_lb_target_group.web.arn}"
            ]
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 4);

        let lt_dep = deps
            .iter()
            .find(|d| d.resource_type == "aws_launch_template");
        assert!(lt_dep.is_some());

        let subnet_deps: Vec<_> = deps
            .iter()
            .filter(|d| d.resource_type == "aws_subnet")
            .collect();
        assert_eq!(subnet_deps.len(), 2);

        let tg_dep = deps
            .iter()
            .find(|d| d.resource_type == "aws_lb_target_group");
        assert!(tg_dep.is_some());
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsAutoScalingGroupResource::new();
        let force_new = resource.forces_replacement();
        assert!(force_new.contains(&"name".to_string()));
        assert!(force_new.contains(&"name_prefix".to_string()));
    }

    #[tokio::test]
    async fn test_plan_create() {
        let resource = AwsAutoScalingGroupResource::new();
        let ctx = make_test_ctx();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678",
                "version": "$Latest"
            },
            "vpc_zone_identifier": ["subnet-1"]
        });

        let diff = resource.plan(&config, None, &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Create));
    }

    #[tokio::test]
    async fn test_plan_no_change() {
        let resource = AwsAutoScalingGroupResource::new();
        let ctx = make_test_ctx();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10
        });

        let state = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10
        });

        let diff = resource.plan(&config, Some(&state), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::NoOp));
    }

    #[tokio::test]
    async fn test_plan_update() {
        let resource = AwsAutoScalingGroupResource::new();
        let ctx = make_test_ctx();

        let config = json!({
            "name": "test-asg",
            "min_size": 2,
            "max_size": 20
        });

        let state = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10
        });

        let diff = resource.plan(&config, Some(&state), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Update));
        assert!(diff.modifications.contains_key("min_size"));
        assert!(diff.modifications.contains_key("max_size"));
    }

    #[tokio::test]
    async fn test_plan_replace_name_change() {
        let resource = AwsAutoScalingGroupResource::new();
        let ctx = make_test_ctx();

        let config = json!({
            "name": "new-asg-name",
            "min_size": 1,
            "max_size": 10
        });

        let state = json!({
            "name": "old-asg-name",
            "min_size": 1,
            "max_size": 10
        });

        let diff = resource.plan(&config, Some(&state), &ctx).await.unwrap();
        assert!(matches!(diff.change_type, ChangeType::Replace));
        assert!(diff.requires_replacement);
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "desired_capacity": 5,
            "launch_template": {
                "id": "lt-12345678",
                "version": "$Latest"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "health_check_type": "ELB",
            "health_check_grace_period": 300
        });

        let result = resource.validate(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_min_max() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 10,
            "max_size": 5,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("min_size cannot be greater than max_size"));
    }

    #[test]
    fn test_validate_invalid_desired_capacity() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 2,
            "max_size": 10,
            "desired_capacity": 1,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("desired_capacity cannot be less than min_size"));
    }

    #[test]
    fn test_validate_missing_launch_template() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "vpc_zone_identifier": ["subnet-1"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("launch_template") || err.contains("launch_configuration"));
    }

    #[test]
    fn test_validate_missing_subnets() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            }
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("vpc_zone_identifier") || err.contains("availability_zones"));
    }

    #[test]
    fn test_validate_invalid_health_check_type() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "health_check_type": "INVALID"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid health_check_type"));
    }

    #[test]
    fn test_validate_invalid_termination_policy() {
        let resource = AwsAutoScalingGroupResource::new();

        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "termination_policies": ["InvalidPolicy"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid termination_policy"));
    }

    #[test]
    fn test_mixed_instances_policy_parsing() {
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "mixed_instances_policy": {
                "launch_template": {
                    "id": "lt-12345678",
                    "version": "$Latest"
                },
                "overrides": [
                    {"instance_type": "t3.micro"},
                    {"instance_type": "t3.small", "weighted_capacity": "2"}
                ],
                "on_demand_base_capacity": 1,
                "on_demand_percentage_above_base_capacity": 25,
                "spot_allocation_strategy": "capacity-optimized"
            },
            "vpc_zone_identifier": ["subnet-1"]
        });

        let parsed: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();
        let mip = parsed.mixed_instances_policy.unwrap();
        assert_eq!(mip.overrides.len(), 2);
        assert_eq!(mip.on_demand_base_capacity, Some(1));
        assert_eq!(
            mip.spot_allocation_strategy,
            Some("capacity-optimized".to_string())
        );
    }

    #[test]
    fn test_instance_refresh_parsing() {
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "instance_refresh": {
                "strategy": "Rolling",
                "min_healthy_percentage": 90,
                "instance_warmup": 300,
                "auto_rollback": true
            }
        });

        let parsed: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();
        let ir = parsed.instance_refresh.unwrap();
        assert_eq!(ir.strategy, "Rolling");
        assert_eq!(ir.min_healthy_percentage, Some(90));
        assert!(ir.auto_rollback);
    }

    #[test]
    fn test_warm_pool_parsing() {
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "warm_pool": {
                "pool_state": "Stopped",
                "min_size": 2,
                "max_group_prepared_capacity": 5
            }
        });

        let parsed: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();
        let wp = parsed.warm_pool.unwrap();
        assert_eq!(wp.pool_state, Some("Stopped".to_string()));
        assert_eq!(wp.min_size, Some(2));
    }

    #[test]
    fn test_tag_config_parsing() {
        let config = json!({
            "name": "test-asg",
            "min_size": 1,
            "max_size": 10,
            "launch_template": {
                "id": "lt-12345678"
            },
            "vpc_zone_identifier": ["subnet-1"],
            "tag": [
                {"key": "Name", "value": "web-server", "propagate_at_launch": true},
                {"key": "Environment", "value": "prod", "propagate_at_launch": false}
            ]
        });

        let parsed: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();
        assert_eq!(parsed.tag.len(), 2);
        assert!(parsed.tag[0].propagate_at_launch);
        assert!(!parsed.tag[1].propagate_at_launch);
    }
}
