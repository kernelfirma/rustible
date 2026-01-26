//! AWS Load Balancer Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsLoadBalancerResource` which implements the `Resource` trait
//! for managing AWS Application and Network Load Balancers declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_lb:
//!     web_alb:
//!       name: web-alb
//!       load_balancer_type: application
//!       internal: false
//!       security_groups:
//!         - sg-12345678
//!       subnets:
//!         - subnet-12345678
//!         - subnet-87654321
//!       enable_deletion_protection: false
//!       tags:
//!         Name: web-alb
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_elasticloadbalancingv2::types::{
    IpAddressType, LoadBalancerSchemeEnum, LoadBalancerStateEnum, LoadBalancerTypeEnum,
    Tag as ElbTag,
};
use aws_sdk_elasticloadbalancingv2::Client;
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

/// Access log configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessLogsConfig {
    /// S3 bucket name
    pub bucket: String,
    /// Enable access logs
    #[serde(default)]
    pub enabled: bool,
    /// S3 bucket prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

/// Subnet mapping for NLB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetMapping {
    /// Subnet ID
    pub subnet_id: String,
    /// Allocation ID for Elastic IP (NLB only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    /// Private IPv4 address (NLB only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ipv4_address: Option<String>,
    /// IPv6 address (NLB only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_address: Option<String>,
}

/// Load balancer configuration parsed from provisioning config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadBalancerConfig {
    /// Load balancer name (required)
    pub name: String,
    /// Load balancer type: application, network, gateway
    #[serde(default = "default_lb_type")]
    pub load_balancer_type: String,
    /// Whether the load balancer is internal
    #[serde(default)]
    pub internal: bool,
    /// Security group IDs (ALB only)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security_groups: Vec<String>,
    /// Subnet IDs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnets: Vec<String>,
    /// Subnet mappings (for NLB with EIPs)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnet_mapping: Vec<SubnetMapping>,
    /// IP address type: ipv4, dualstack
    #[serde(default = "default_ip_type")]
    pub ip_address_type: String,
    /// Customer-owned IP pool (Outpost only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_owned_ipv4_pool: Option<String>,
    /// Enable deletion protection
    #[serde(default)]
    pub enable_deletion_protection: bool,
    /// Enable cross-zone load balancing (NLB)
    #[serde(default = "default_true")]
    pub enable_cross_zone_load_balancing: bool,
    /// Enable HTTP/2 (ALB only)
    #[serde(default = "default_true")]
    pub enable_http2: bool,
    /// Enable WAF fail-open (ALB only)
    #[serde(default)]
    pub enable_waf_fail_open: bool,
    /// Idle timeout in seconds (ALB only)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: i32,
    /// Desync mitigation mode: defensive, strictest, monitor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desync_mitigation_mode: Option<String>,
    /// Drop invalid header fields (ALB only)
    #[serde(default)]
    pub drop_invalid_header_fields: bool,
    /// Access logs configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_logs: Option<AccessLogsConfig>,
    /// Resource tags
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

fn default_lb_type() -> String {
    "application".to_string()
}

fn default_ip_type() -> String {
    "ipv4".to_string()
}

fn default_true() -> bool {
    true
}

fn default_idle_timeout() -> i32 {
    60
}

impl LoadBalancerConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid load balancer configuration: {}",
                e
            ))
        })
    }
}

/// Computed attributes returned after load balancer operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoadBalancerState {
    /// Load balancer ARN (the primary ID)
    pub id: String,
    /// Load balancer ARN
    pub arn: String,
    /// Load balancer ARN suffix (for CloudWatch)
    pub arn_suffix: String,
    /// DNS name
    pub dns_name: String,
    /// Canonical hosted zone ID (for Route53)
    pub zone_id: String,
    /// Load balancer name
    pub name: String,
    /// Load balancer type
    pub load_balancer_type: String,
    /// VPC ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    /// Internal or internet-facing
    pub internal: bool,
    /// Security group IDs
    #[serde(default)]
    pub security_groups: Vec<String>,
    /// Subnet IDs
    #[serde(default)]
    pub subnets: Vec<String>,
    /// IP address type
    pub ip_address_type: String,
    /// Current state
    pub state: String,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS Load Balancer Resource
// ============================================================================

/// AWS Load Balancer Resource implementation
#[derive(Debug, Clone)]
pub struct AwsLoadBalancerResource;

impl AwsLoadBalancerResource {
    /// Create a new AWS Load Balancer resource
    pub fn new() -> Self {
        Self
    }

    /// Create AWS ELBv2 client from provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_elasticloadbalancingv2::config::Region::new(
                    region.clone(),
                ))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Extract ARN suffix for CloudWatch metrics
    fn extract_arn_suffix(&self, arn: &str) -> String {
        // ARN format: arn:aws:elasticloadbalancing:region:account:loadbalancer/type/name/id
        // Extract: type/name/id
        if let Some(lb_part) = arn.split("loadbalancer/").nth(1) {
            lb_part.to_string()
        } else {
            String::new()
        }
    }

    /// Describe load balancer by ARN or name
    async fn describe_load_balancer(
        &self,
        client: &Client,
        identifier: &str,
    ) -> ProvisioningResult<Option<LoadBalancerState>> {
        let resp = if identifier.starts_with("arn:") {
            client
                .describe_load_balancers()
                .load_balancer_arns(identifier)
                .send()
                .await
        } else {
            client
                .describe_load_balancers()
                .names(identifier)
                .send()
                .await
        };

        match resp {
            Ok(output) => {
                if let Some(lb) = output.load_balancers().first() {
                    Ok(Some(self.lb_to_state(lb)))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("LoadBalancerNotFound") || err_str.contains("not found") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to describe load balancer: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Get tags for load balancer
    async fn get_tags(
        &self,
        client: &Client,
        arn: &str,
    ) -> ProvisioningResult<HashMap<String, String>> {
        let resp = client
            .describe_tags()
            .resource_arns(arn)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to get tags: {}", e))
            })?;

        let mut tags = HashMap::new();
        for tag_desc in resp.tag_descriptions() {
            for tag in tag_desc.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }
        }

        Ok(tags)
    }

    /// Convert SDK load balancer to state struct
    fn lb_to_state(
        &self,
        lb: &aws_sdk_elasticloadbalancingv2::types::LoadBalancer,
    ) -> LoadBalancerState {
        let arn = lb.load_balancer_arn().unwrap_or_default().to_string();
        let name = lb.load_balancer_name().unwrap_or_default().to_string();

        let lb_type = lb
            .r#type()
            .map(|t| t.as_str().to_string())
            .unwrap_or_else(|| "application".to_string());

        let scheme = lb.scheme();
        let internal = scheme == Some(&LoadBalancerSchemeEnum::Internal);

        let security_groups: Vec<String> = lb
            .security_groups()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let subnets: Vec<String> = lb
            .availability_zones()
            .iter()
            .filter_map(|az| az.subnet_id().map(|s| s.to_string()))
            .collect();

        let ip_type = lb
            .ip_address_type()
            .map(|t| t.as_str().to_string())
            .unwrap_or_else(|| "ipv4".to_string());

        let state = lb
            .state()
            .and_then(|s| s.code())
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        LoadBalancerState {
            id: arn.clone(),
            arn: arn.clone(),
            arn_suffix: self.extract_arn_suffix(&arn),
            dns_name: lb.dns_name().unwrap_or_default().to_string(),
            zone_id: lb.canonical_hosted_zone_id().unwrap_or_default().to_string(),
            name,
            load_balancer_type: lb_type,
            vpc_id: lb.vpc_id().map(|s| s.to_string()),
            internal,
            security_groups,
            subnets,
            ip_address_type: ip_type,
            state,
            tags: HashMap::new(), // Tags fetched separately
        }
    }

    /// Wait for load balancer to reach a specific state
    async fn wait_for_state(
        &self,
        client: &Client,
        arn: &str,
        desired_state: LoadBalancerStateEnum,
        timeout: Duration,
    ) -> ProvisioningResult<LoadBalancerState> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(10);

        debug!(
            "Waiting for load balancer to reach state {:?}",
            desired_state
        );

        loop {
            if start.elapsed() >= timeout {
                return Err(ProvisioningError::Timeout {
                    operation: format!("waiting for load balancer to reach {:?}", desired_state),
                    seconds: timeout.as_secs(),
                });
            }

            if let Some(state) = self.describe_load_balancer(client, arn).await? {
                let current_state = match state.state.as_str() {
                    "active" => LoadBalancerStateEnum::Active,
                    "provisioning" => LoadBalancerStateEnum::Provisioning,
                    "active_impaired" => LoadBalancerStateEnum::ActiveImpaired,
                    "failed" => LoadBalancerStateEnum::Failed,
                    _ => LoadBalancerStateEnum::Provisioning,
                };

                if current_state == desired_state {
                    return Ok(state);
                }

                if current_state == LoadBalancerStateEnum::Failed {
                    return Err(ProvisioningError::CloudApiError(
                        "Load balancer creation failed".to_string(),
                    ));
                }

                debug!("Load balancer state: {}, waiting for {:?}", state.state, desired_state);
            }

            tokio::time::sleep(poll_interval).await;
        }
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

        if let Some(start) = ref_str.find("{{") {
            if let Some(end) = ref_str[start..].find("}}") {
                let inner = ref_str[start + 2..start + end].trim();
                let inner = inner.trim_start_matches("resources.");
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

    /// Build tags for ELBv2 API
    fn build_tags(&self, tags: &HashMap<String, String>) -> Vec<ElbTag> {
        tags.iter()
            .map(|(k, v)| ElbTag::builder().key(k).value(v).build())
            .collect()
    }
}

impl Default for AwsLoadBalancerResource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Resource for AwsLoadBalancerResource {
    fn resource_type(&self) -> &str {
        "aws_lb"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_lb".to_string(),
            description: "Provides a Load Balancer resource (Application, Network, or Gateway)."
                .to_string(),
            required_args: vec![SchemaField {
                name: "name".to_string(),
                field_type: FieldType::String,
                description: "The name of the LB".to_string(),
                default: None,
                constraints: vec![
                    FieldConstraint::MinLength { min: 1 },
                    FieldConstraint::MaxLength { max: 32 },
                    FieldConstraint::Pattern {
                        regex: r"^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?$".to_string(),
                    },
                ],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "load_balancer_type".to_string(),
                    field_type: FieldType::String,
                    description: "The type of load balancer to create".to_string(),
                    default: Some(Value::String("application".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "application".to_string(),
                            "network".to_string(),
                            "gateway".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "internal".to_string(),
                    field_type: FieldType::Boolean,
                    description: "If true, the LB will be internal".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "security_groups".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "A list of security group IDs to assign to the LB".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "subnets".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "A list of subnet IDs to attach to the LB".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ip_address_type".to_string(),
                    field_type: FieldType::String,
                    description: "The type of IP addresses used by the subnets".to_string(),
                    default: Some(Value::String("ipv4".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec!["ipv4".to_string(), "dualstack".to_string()],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_deletion_protection".to_string(),
                    field_type: FieldType::Boolean,
                    description: "If true, deletion of the LB will be disabled".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_cross_zone_load_balancing".to_string(),
                    field_type: FieldType::Boolean,
                    description: "If true, cross-zone load balancing is enabled (NLB)"
                        .to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "enable_http2".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Indicates whether HTTP/2 is enabled (ALB)".to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "idle_timeout".to_string(),
                    field_type: FieldType::Integer,
                    description: "The time in seconds that the connection is idle (ALB)"
                        .to_string(),
                    default: Some(Value::Number(60.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 1 },
                        FieldConstraint::MaxValue { value: 4000 },
                    ],
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
                    description: "The ARN of the load balancer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the load balancer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn_suffix".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN suffix for use with CloudWatch Metrics".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "dns_name".to_string(),
                    field_type: FieldType::String,
                    description: "The DNS name of the load balancer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "zone_id".to_string(),
                    field_type: FieldType::String,
                    description: "The canonical hosted zone ID of the load balancer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "The VPC ID of the load balancer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "name".to_string(),
                "load_balancer_type".to_string(),
                "internal".to_string(),
            ],
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

        match self.describe_load_balancer(&client, id).await? {
            Some(mut state) => {
                // Fetch tags
                state.tags = self.get_tags(&client, &state.arn).await?;

                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize load balancer attributes: {}",
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

                let force_new = ["name", "load_balancer_type", "internal"];

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
                let computed_fields = [
                    "id", "arn", "arn_suffix", "dns_name", "zone_id", "vpc_id", "state",
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
        let lb_config = LoadBalancerConfig::from_value(config)?;
        let client = self.create_client(ctx).await?;

        info!("Creating load balancer: {}", lb_config.name);

        let lb_type = match lb_config.load_balancer_type.as_str() {
            "application" => LoadBalancerTypeEnum::Application,
            "network" => LoadBalancerTypeEnum::Network,
            "gateway" => LoadBalancerTypeEnum::Gateway,
            _ => LoadBalancerTypeEnum::Application,
        };

        let scheme = if lb_config.internal {
            LoadBalancerSchemeEnum::Internal
        } else {
            LoadBalancerSchemeEnum::InternetFacing
        };

        let ip_type = match lb_config.ip_address_type.as_str() {
            "dualstack" => IpAddressType::Dualstack,
            _ => IpAddressType::Ipv4,
        };

        let mut create_lb = client
            .create_load_balancer()
            .name(&lb_config.name)
            .r#type(lb_type)
            .scheme(scheme)
            .ip_address_type(ip_type);

        // Add subnets
        if !lb_config.subnet_mapping.is_empty() {
            for mapping in &lb_config.subnet_mapping {
                let mut sm = aws_sdk_elasticloadbalancingv2::types::SubnetMapping::builder()
                    .subnet_id(&mapping.subnet_id);

                if let Some(ref alloc_id) = mapping.allocation_id {
                    sm = sm.allocation_id(alloc_id);
                }
                if let Some(ref private_ip) = mapping.private_ipv4_address {
                    sm = sm.private_ipv4_address(private_ip);
                }
                if let Some(ref ipv6) = mapping.ipv6_address {
                    sm = sm.ipv6_address(ipv6);
                }

                create_lb = create_lb.subnet_mappings(sm.build());
            }
        } else {
            for subnet in &lb_config.subnets {
                create_lb = create_lb.subnets(subnet);
            }
        }

        // Add security groups (ALB only)
        if lb_config.load_balancer_type == "application" {
            for sg in &lb_config.security_groups {
                create_lb = create_lb.security_groups(sg);
            }
        }

        // Add tags
        let mut all_tags = ctx.default_tags.clone();
        all_tags.extend(lb_config.tags.clone());
        if !all_tags.is_empty() {
            let tags = self.build_tags(&all_tags);
            create_lb = create_lb.set_tags(Some(tags));
        }

        let resp = create_lb.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create load balancer: {}", e))
        })?;

        let lb = resp
            .load_balancers()
            .first()
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("No load balancer returned".to_string())
            })?;

        let arn = lb.load_balancer_arn().unwrap_or_default().to_string();

        info!("Created load balancer: {} ({})", lb_config.name, arn);

        // Set attributes
        let mut modify_attrs = client
            .modify_load_balancer_attributes()
            .load_balancer_arn(&arn);

        // Deletion protection
        modify_attrs = modify_attrs.attributes(
            aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                .key("deletion_protection.enabled")
                .value(lb_config.enable_deletion_protection.to_string())
                .build(),
        );

        // Type-specific attributes
        if lb_config.load_balancer_type == "application" {
            modify_attrs = modify_attrs
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("idle_timeout.timeout_seconds")
                        .value(lb_config.idle_timeout.to_string())
                        .build(),
                )
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("routing.http2.enabled")
                        .value(lb_config.enable_http2.to_string())
                        .build(),
                )
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("routing.http.drop_invalid_header_fields.enabled")
                        .value(lb_config.drop_invalid_header_fields.to_string())
                        .build(),
                )
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("waf.fail_open.enabled")
                        .value(lb_config.enable_waf_fail_open.to_string())
                        .build(),
                );

            if let Some(ref desync_mode) = lb_config.desync_mitigation_mode {
                modify_attrs = modify_attrs.attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("routing.http.desync_mitigation_mode")
                        .value(desync_mode)
                        .build(),
                );
            }
        } else if lb_config.load_balancer_type == "network" {
            modify_attrs = modify_attrs.attributes(
                aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                    .key("load_balancing.cross_zone.enabled")
                    .value(lb_config.enable_cross_zone_load_balancing.to_string())
                    .build(),
            );
        }

        // Access logs
        if let Some(ref access_logs) = lb_config.access_logs {
            modify_attrs = modify_attrs
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("access_logs.s3.enabled")
                        .value(access_logs.enabled.to_string())
                        .build(),
                )
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("access_logs.s3.bucket")
                        .value(&access_logs.bucket)
                        .build(),
                );

            if let Some(ref prefix) = access_logs.prefix {
                modify_attrs = modify_attrs.attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("access_logs.s3.prefix")
                        .value(prefix)
                        .build(),
                );
            }
        }

        modify_attrs.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!(
                "Failed to set load balancer attributes: {}",
                e
            ))
        })?;

        // Wait for active state
        let timeout = Duration::from_secs(ctx.timeout_seconds);
        let mut state = self
            .wait_for_state(&client, &arn, LoadBalancerStateEnum::Active, timeout)
            .await?;

        // Fetch tags
        state.tags = self.get_tags(&client, &arn).await?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(&arn, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("dns_name", Value::String(state.dns_name.clone()))
            .with_output("zone_id", Value::String(state.zone_id.clone())))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let lb_config = LoadBalancerConfig::from_value(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating load balancer: {}", id);

        // Update security groups (ALB only)
        if lb_config.load_balancer_type == "application" && !lb_config.security_groups.is_empty() {
            client
                .set_security_groups()
                .load_balancer_arn(id)
                .set_security_groups(Some(lb_config.security_groups.clone()))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to update security groups: {}",
                        e
                    ))
                })?;
        }

        // Update subnets
        if !lb_config.subnets.is_empty() {
            client
                .set_subnets()
                .load_balancer_arn(id)
                .set_subnets(Some(lb_config.subnets.clone()))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update subnets: {}", e))
                })?;
        }

        // Update attributes
        let mut modify_attrs = client
            .modify_load_balancer_attributes()
            .load_balancer_arn(id);

        modify_attrs = modify_attrs.attributes(
            aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                .key("deletion_protection.enabled")
                .value(lb_config.enable_deletion_protection.to_string())
                .build(),
        );

        if lb_config.load_balancer_type == "application" {
            modify_attrs = modify_attrs
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("idle_timeout.timeout_seconds")
                        .value(lb_config.idle_timeout.to_string())
                        .build(),
                )
                .attributes(
                    aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                        .key("routing.http2.enabled")
                        .value(lb_config.enable_http2.to_string())
                        .build(),
                );
        }

        modify_attrs.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to modify attributes: {}", e))
        })?;

        // Update tags
        let mut all_tags = ctx.default_tags.clone();
        all_tags.extend(lb_config.tags.clone());

        if !all_tags.is_empty() {
            let tags = self.build_tags(&all_tags);
            client
                .add_tags()
                .resource_arns(id)
                .set_tags(Some(tags))
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                })?;
        }

        // Get final state
        let mut state = self
            .describe_load_balancer(&client, id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Load balancer not found after update".to_string())
            })?;

        state.tags = self.get_tags(&client, id).await?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Check if load balancer exists
        if self.describe_load_balancer(&client, id).await?.is_none() {
            return Ok(ResourceResult::success(id, Value::Null));
        }

        info!("Deleting load balancer: {}", id);

        // Disable deletion protection first
        client
            .modify_load_balancer_attributes()
            .load_balancer_arn(id)
            .attributes(
                aws_sdk_elasticloadbalancingv2::types::LoadBalancerAttribute::builder()
                    .key("deletion_protection.enabled")
                    .value("false")
                    .build(),
            )
            .send()
            .await
            .ok(); // Ignore errors

        // Delete the load balancer
        client
            .delete_load_balancer()
            .load_balancer_arn(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete load balancer: {}", e))
            })?;

        // Wait for deletion (poll until not found)
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(ctx.timeout_seconds);
        let poll_interval = Duration::from_secs(10);

        loop {
            if start.elapsed() >= timeout {
                return Err(ProvisioningError::Timeout {
                    operation: "waiting for load balancer deletion".to_string(),
                    seconds: timeout.as_secs(),
                });
            }

            if self.describe_load_balancer(&client, id).await?.is_none() {
                break;
            }

            tokio::time::sleep(poll_interval).await;
        }

        info!("Deleted load balancer: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        let mut state = self
            .describe_load_balancer(&client, id)
            .await?
            .ok_or_else(|| ProvisioningError::ImportError {
                resource_type: "aws_lb".to_string(),
                resource_id: id.to_string(),
                message: "Load balancer not found".to_string(),
            })?;

        state.tags = self.get_tags(&client, &state.arn).await?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(obj) = config.as_object() {
            // Check security_groups for references
            if let Some(sgs) = obj.get("security_groups") {
                if let Some(arr) = sgs.as_array() {
                    for sg in arr {
                        deps.extend(self.extract_references(sg));
                    }
                }
            }

            // Check subnets for references
            if let Some(subnets) = obj.get("subnets") {
                if let Some(arr) = subnets.as_array() {
                    for subnet in arr {
                        deps.extend(self.extract_references(subnet));
                    }
                }
            }

            // Check access_logs bucket for references
            if let Some(access_logs) = obj.get("access_logs") {
                if let Some(bucket) = access_logs.get("bucket") {
                    deps.extend(self.extract_references(bucket));
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "name".to_string(),
            "load_balancer_type".to_string(),
            "internal".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate required fields
        if !obj.contains_key("name") {
            return Err(ProvisioningError::ValidationError(
                "name is required".to_string(),
            ));
        }

        // Validate name
        if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
            if name.is_empty() || name.len() > 32 {
                return Err(ProvisioningError::ValidationError(
                    "name must be between 1 and 32 characters".to_string(),
                ));
            }
            if !name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphanumeric())
                .unwrap_or(false)
            {
                return Err(ProvisioningError::ValidationError(
                    "name must start with an alphanumeric character".to_string(),
                ));
            }
        }

        // Validate load_balancer_type
        if let Some(lb_type) = obj.get("load_balancer_type").and_then(|v| v.as_str()) {
            let valid_types = ["application", "network", "gateway"];
            if !valid_types.contains(&lb_type) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid load_balancer_type: {}. Must be one of: {}",
                    lb_type,
                    valid_types.join(", ")
                )));
            }
        }

        // Validate ip_address_type
        if let Some(ip_type) = obj.get("ip_address_type").and_then(|v| v.as_str()) {
            let valid_types = ["ipv4", "dualstack"];
            if !valid_types.contains(&ip_type) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid ip_address_type: {}. Must be one of: {}",
                    ip_type,
                    valid_types.join(", ")
                )));
            }
        }

        // Validate idle_timeout
        if let Some(idle_timeout) = obj.get("idle_timeout") {
            if let Some(timeout) = idle_timeout.as_i64() {
                if timeout < 1 || timeout > 4000 {
                    return Err(ProvisioningError::ValidationError(
                        "idle_timeout must be between 1 and 4000 seconds".to_string(),
                    ));
                }
            }
        }

        // Validate subnets requirement
        let has_subnets = obj
            .get("subnets")
            .map(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .unwrap_or(false);
        let has_subnet_mapping = obj
            .get("subnet_mapping")
            .map(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .unwrap_or(false);

        if !has_subnets && !has_subnet_mapping {
            return Err(ProvisioningError::ValidationError(
                "Either subnets or subnet_mapping must be specified".to_string(),
            ));
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
        let resource = AwsLoadBalancerResource::new();
        assert_eq!(resource.resource_type(), "aws_lb");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsLoadBalancerResource::new();
        let forces = resource.forces_replacement();

        assert!(forces.contains(&"name".to_string()));
        assert!(forces.contains(&"load_balancer_type".to_string()));
        assert!(forces.contains(&"internal".to_string()));
    }

    #[test]
    fn test_schema_has_required_fields() {
        let resource = AwsLoadBalancerResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_lb");
        assert!(!schema.required_args.is_empty());

        let has_name = schema.required_args.iter().any(|f| f.name == "name");
        assert!(has_name);
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-alb",
            "load_balancer_type": "application",
            "subnets": ["subnet-12345678", "subnet-87654321"]
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_name() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "subnets": ["subnet-12345678"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_name_too_long() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "this-name-is-way-too-long-for-alb",
            "subnets": ["subnet-12345678"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_lb_type() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-lb",
            "load_balancer_type": "invalid",
            "subnets": ["subnet-12345678"]
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_missing_subnets() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-lb"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_idle_timeout() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-lb",
            "subnets": ["subnet-12345678"],
            "idle_timeout": 5000  // Max is 4000
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_lb_config_parsing() {
        let config = json!({
            "name": "web-alb",
            "load_balancer_type": "application",
            "internal": false,
            "security_groups": ["sg-12345678"],
            "subnets": ["subnet-12345678", "subnet-87654321"],
            "enable_deletion_protection": true,
            "idle_timeout": 120,
            "tags": {
                "Name": "web-alb",
                "Environment": "production"
            }
        });

        let lb_config = LoadBalancerConfig::from_value(&config).unwrap();

        assert_eq!(lb_config.name, "web-alb");
        assert_eq!(lb_config.load_balancer_type, "application");
        assert!(!lb_config.internal);
        assert_eq!(lb_config.security_groups.len(), 1);
        assert_eq!(lb_config.subnets.len(), 2);
        assert!(lb_config.enable_deletion_protection);
        assert_eq!(lb_config.idle_timeout, 120);
        assert_eq!(lb_config.tags.get("Name"), Some(&"web-alb".to_string()));
    }

    #[test]
    fn test_lb_config_defaults() {
        let config = json!({
            "name": "my-lb",
            "subnets": ["subnet-12345678"]
        });

        let lb_config = LoadBalancerConfig::from_value(&config).unwrap();

        assert_eq!(lb_config.load_balancer_type, "application");
        assert_eq!(lb_config.ip_address_type, "ipv4");
        assert!(!lb_config.internal);
        assert!(!lb_config.enable_deletion_protection);
        assert!(lb_config.enable_cross_zone_load_balancing);
        assert!(lb_config.enable_http2);
        assert_eq!(lb_config.idle_timeout, 60);
    }

    #[test]
    fn test_extract_arn_suffix() {
        let resource = AwsLoadBalancerResource::new();

        let arn = "arn:aws:elasticloadbalancing:us-east-1:123456789012:loadbalancer/app/my-alb/1234567890abcdef";
        let suffix = resource.extract_arn_suffix(arn);

        assert_eq!(suffix, "app/my-alb/1234567890abcdef");
    }

    #[test]
    fn test_plan_create() {
        let resource = AwsLoadBalancerResource::new();

        let desired = json!({
            "name": "my-alb",
            "subnets": ["subnet-12345678"]
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 600,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, None, &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_plan_no_change() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-alb",
            "subnets": ["subnet-12345678"]
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 600,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&config, Some(&config), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_plan_replace_name_change() {
        let resource = AwsLoadBalancerResource::new();

        let current = json!({
            "name": "old-alb",
            "subnets": ["subnet-12345678"]
        });

        let desired = json!({
            "name": "new-alb",
            "subnets": ["subnet-12345678"]
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 600,
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
        let state = LoadBalancerState {
            id: "arn:aws:elasticloadbalancing:us-east-1:123456789012:loadbalancer/app/my-alb/1234567890abcdef".to_string(),
            arn: "arn:aws:elasticloadbalancing:us-east-1:123456789012:loadbalancer/app/my-alb/1234567890abcdef".to_string(),
            arn_suffix: "app/my-alb/1234567890abcdef".to_string(),
            dns_name: "my-alb-123456789.us-east-1.elb.amazonaws.com".to_string(),
            zone_id: "Z35SXDOTRQ7X7K".to_string(),
            name: "my-alb".to_string(),
            load_balancer_type: "application".to_string(),
            vpc_id: Some("vpc-12345678".to_string()),
            internal: false,
            security_groups: vec!["sg-12345678".to_string()],
            subnets: vec!["subnet-12345678".to_string()],
            ip_address_type: "ipv4".to_string(),
            state: "active".to_string(),
            tags: HashMap::new(),
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["name"], "my-alb");
        assert_eq!(json["load_balancer_type"], "application");
        assert_eq!(json["state"], "active");
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsLoadBalancerResource::new();

        let config = json!({
            "name": "my-alb",
            "security_groups": ["${aws_security_group.web.id}"],
            "subnets": [
                "${aws_subnet.public_a.id}",
                "${aws_subnet.public_b.id}"
            ]
        });

        let deps = resource.dependencies(&config);

        let has_sg = deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "web");
        let has_subnet_a = deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "public_a");
        let has_subnet_b = deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "public_b");

        assert!(has_sg, "Should detect security group dependency");
        assert!(has_subnet_a, "Should detect subnet_a dependency");
        assert!(has_subnet_b, "Should detect subnet_b dependency");
    }
}
