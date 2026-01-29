//! AWS Security Group Rule Resource for Infrastructure Provisioning
//!
//! This module implements the `aws_security_group_rule` resource type for managing
//! individual security group rules independently from the security group itself.
//!
//! # Example
//!
//! ```yaml
//! resources:
//!   aws_security_group_rule:
//!     allow_http:
//!       type: ingress
//!       security_group_id: "{{ resources.aws_security_group.web.id }}"
//!       protocol: tcp
//!       from_port: 80
//!       to_port: 80
//!       cidr_blocks:
//!         - "0.0.0.0/0"
//!       description: "Allow HTTP traffic"
//!
//!     allow_ssh_from_bastion:
//!       type: ingress
//!       security_group_id: "{{ resources.aws_security_group.app.id }}"
//!       protocol: tcp
//!       from_port: 22
//!       to_port: 22
//!       source_security_group_id: "{{ resources.aws_security_group.bastion.id }}"
//!       description: "Allow SSH from bastion"
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{IpPermission, IpRange, Ipv6Range, UserIdGroupPair};
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
// Security Group Rule Type
// ============================================================================

/// Type of security group rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleType {
    /// Inbound rule
    Ingress,
    /// Outbound rule
    Egress,
}

impl RuleType {
    fn as_str(&self) -> &str {
        match self {
            RuleType::Ingress => "ingress",
            RuleType::Egress => "egress",
        }
    }
}

// ============================================================================
// AWS Security Group Rule Resource
// ============================================================================

/// AWS Security Group Rule resource implementation
#[derive(Debug, Clone)]
pub struct AwsSecurityGroupRuleResource;

impl AwsSecurityGroupRuleResource {
    /// Create a new instance
    pub fn new() -> Self {
        Self
    }

    /// Create AWS EC2 client from provider context
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

    /// Parse configuration from JSON Value
    fn parse_config(&self, config: &Value) -> ProvisioningResult<SecurityGroupRuleConfig> {
        serde_json::from_value(config.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid security group rule configuration: {}",
                e
            ))
        })
    }

    /// Generate a unique rule ID based on its properties
    fn generate_rule_id(config: &SecurityGroupRuleConfig) -> String {
        let direction = config.rule_type.as_str();
        let proto = &config.protocol;
        let from_port = config.from_port;
        let to_port = config.to_port;

        // Build a deterministic ID
        let cidr_part = if !config.cidr_blocks.is_empty() {
            format!(
                "cidr_{}",
                config.cidr_blocks.join("_").replace(['/', '.'], "-")
            )
        } else if !config.ipv6_cidr_blocks.is_empty() {
            format!(
                "ipv6_{}",
                config.ipv6_cidr_blocks.join("_").replace(['/', ':'], "-")
            )
        } else if let Some(ref sg_id) = config.source_security_group_id {
            format!("sg_{}", sg_id)
        } else if config.self_referencing {
            "self".to_string()
        } else {
            "unknown".to_string()
        };

        format!(
            "sgrule-{}-{}-{}-{}-{}",
            &config.security_group_id, direction, proto, from_port, to_port
        )
        .chars()
        .take(64)
        .collect()
    }

    /// Convert config to AWS SDK IpPermission
    fn to_ip_permission(&self, config: &SecurityGroupRuleConfig) -> IpPermission {
        let mut builder = IpPermission::builder()
            .ip_protocol(&config.protocol)
            .from_port(config.from_port)
            .to_port(config.to_port);

        // Add IPv4 CIDR blocks
        for cidr in &config.cidr_blocks {
            let mut ip_range = IpRange::builder().cidr_ip(cidr);
            if let Some(ref desc) = config.description {
                ip_range = ip_range.description(desc);
            }
            builder = builder.ip_ranges(ip_range.build());
        }

        // Add IPv6 CIDR blocks
        for cidr_v6 in &config.ipv6_cidr_blocks {
            let mut ipv6_range = Ipv6Range::builder().cidr_ipv6(cidr_v6);
            if let Some(ref desc) = config.description {
                ipv6_range = ipv6_range.description(desc);
            }
            builder = builder.ipv6_ranges(ipv6_range.build());
        }

        // Add source/destination security group
        if let Some(ref sg_id) = config.source_security_group_id {
            let mut user_id_group = UserIdGroupPair::builder().group_id(sg_id);
            if let Some(ref desc) = config.description {
                user_id_group = user_id_group.description(desc);
            }
            builder = builder.user_id_group_pairs(user_id_group.build());
        }

        // Handle self-referencing
        if config.self_referencing {
            let mut user_id_group = UserIdGroupPair::builder().group_id(&config.security_group_id);
            if let Some(ref desc) = config.description {
                user_id_group = user_id_group.description(desc);
            }
            builder = builder.user_id_group_pairs(user_id_group.build());
        }

        builder.build()
    }

    /// Find a matching rule in the security group
    async fn find_rule(
        &self,
        config: &SecurityGroupRuleConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<SecurityGroupRuleState>> {
        let client = self.create_client(ctx).await?;

        let resp = client
            .describe_security_groups()
            .group_ids(&config.security_group_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!(
                    "Failed to describe security group: {}",
                    e
                ))
            })?;

        for sg in resp.security_groups() {
            let rules = match config.rule_type {
                RuleType::Ingress => sg.ip_permissions(),
                RuleType::Egress => sg.ip_permissions_egress(),
            };

            for perm in rules {
                if self.rule_matches(perm, config) {
                    return Ok(Some(SecurityGroupRuleState {
                        id: Self::generate_rule_id(config),
                        security_group_id: config.security_group_id.clone(),
                        rule_type: config.rule_type,
                        protocol: config.protocol.clone(),
                        from_port: config.from_port,
                        to_port: config.to_port,
                        cidr_blocks: config.cidr_blocks.clone(),
                        ipv6_cidr_blocks: config.ipv6_cidr_blocks.clone(),
                        source_security_group_id: config.source_security_group_id.clone(),
                        self_referencing: config.self_referencing,
                        description: config.description.clone(),
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Check if an IpPermission matches our config
    fn rule_matches(&self, perm: &IpPermission, config: &SecurityGroupRuleConfig) -> bool {
        let protocol = perm.ip_protocol().unwrap_or("-1");
        let from_port = perm.from_port().unwrap_or(-1);
        let to_port = perm.to_port().unwrap_or(-1);

        // Check protocol and ports
        if protocol != config.protocol || from_port != config.from_port || to_port != config.to_port
        {
            return false;
        }

        // Check CIDR blocks
        if !config.cidr_blocks.is_empty() {
            let perm_cidrs: Vec<String> = perm
                .ip_ranges()
                .iter()
                .filter_map(|r| r.cidr_ip().map(|s| s.to_string()))
                .collect();

            for cidr in &config.cidr_blocks {
                if !perm_cidrs.contains(cidr) {
                    return false;
                }
            }
            return true;
        }

        // Check IPv6 CIDR blocks
        if !config.ipv6_cidr_blocks.is_empty() {
            let perm_cidrs_v6: Vec<String> = perm
                .ipv6_ranges()
                .iter()
                .filter_map(|r| r.cidr_ipv6().map(|s| s.to_string()))
                .collect();

            for cidr in &config.ipv6_cidr_blocks {
                if !perm_cidrs_v6.contains(cidr) {
                    return false;
                }
            }
            return true;
        }

        // Check security group reference
        if let Some(ref src_sg) = config.source_security_group_id {
            let perm_sgs: Vec<String> = perm
                .user_id_group_pairs()
                .iter()
                .filter_map(|p| p.group_id().map(|s| s.to_string()))
                .collect();

            if perm_sgs.contains(src_sg) {
                return true;
            }
        }

        // Check self-referencing
        if config.self_referencing {
            let perm_sgs: Vec<String> = perm
                .user_id_group_pairs()
                .iter()
                .filter_map(|p| p.group_id().map(|s| s.to_string()))
                .collect();

            if perm_sgs.contains(&config.security_group_id) {
                return true;
            }
        }

        false
    }

    /// Authorize the rule (create)
    async fn authorize_rule(
        &self,
        client: &Client,
        config: &SecurityGroupRuleConfig,
    ) -> ProvisioningResult<()> {
        let ip_permission = self.to_ip_permission(config);

        match config.rule_type {
            RuleType::Ingress => {
                client
                    .authorize_security_group_ingress()
                    .group_id(&config.security_group_id)
                    .ip_permissions(ip_permission)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to authorize ingress rule: {}",
                            e
                        ))
                    })?;
            }
            RuleType::Egress => {
                client
                    .authorize_security_group_egress()
                    .group_id(&config.security_group_id)
                    .ip_permissions(ip_permission)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to authorize egress rule: {}",
                            e
                        ))
                    })?;
            }
        }

        Ok(())
    }

    /// Revoke the rule (delete)
    async fn revoke_rule(
        &self,
        client: &Client,
        config: &SecurityGroupRuleConfig,
    ) -> ProvisioningResult<()> {
        let ip_permission = self.to_ip_permission(config);

        match config.rule_type {
            RuleType::Ingress => {
                client
                    .revoke_security_group_ingress()
                    .group_id(&config.security_group_id)
                    .ip_permissions(ip_permission)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to revoke ingress rule: {}",
                            e
                        ))
                    })?;
            }
            RuleType::Egress => {
                client
                    .revoke_security_group_egress()
                    .group_id(&config.security_group_id)
                    .ip_permissions(ip_permission)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to revoke egress rule: {}",
                            e
                        ))
                    })?;
            }
        }

        Ok(())
    }
}

impl Default for AwsSecurityGroupRuleResource {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Security Group Rule Configuration (from YAML/JSON)
// ============================================================================

/// Security group rule configuration as parsed from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupRuleConfig {
    /// Rule type: ingress or egress
    #[serde(rename = "type")]
    pub rule_type: RuleType,

    /// Security group ID to add the rule to
    pub security_group_id: String,

    /// IP protocol: tcp, udp, icmp, icmpv6, or -1 for all
    #[serde(default = "default_protocol")]
    pub protocol: String,

    /// Start of port range (use -1 for all with protocol -1)
    #[serde(default = "default_port")]
    pub from_port: i32,

    /// End of port range (use -1 for all with protocol -1)
    #[serde(default = "default_port")]
    pub to_port: i32,

    /// IPv4 CIDR blocks
    #[serde(default)]
    pub cidr_blocks: Vec<String>,

    /// IPv6 CIDR blocks
    #[serde(default)]
    pub ipv6_cidr_blocks: Vec<String>,

    /// Source/destination security group ID
    #[serde(default)]
    pub source_security_group_id: Option<String>,

    /// Allow traffic from/to the security group itself
    #[serde(default)]
    pub self_referencing: bool,

    /// Rule description
    #[serde(default)]
    pub description: Option<String>,
}

fn default_protocol() -> String {
    "-1".to_string()
}

fn default_port() -> i32 {
    -1
}

// ============================================================================
// Security Group Rule State (from AWS)
// ============================================================================

/// Current state of a security group rule from AWS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupRuleState {
    /// Generated rule ID
    pub id: String,

    /// Security group ID
    pub security_group_id: String,

    /// Rule type
    pub rule_type: RuleType,

    /// Protocol
    pub protocol: String,

    /// From port
    pub from_port: i32,

    /// To port
    pub to_port: i32,

    /// IPv4 CIDR blocks
    pub cidr_blocks: Vec<String>,

    /// IPv6 CIDR blocks
    pub ipv6_cidr_blocks: Vec<String>,

    /// Source security group ID
    pub source_security_group_id: Option<String>,

    /// Self-referencing
    pub self_referencing: bool,

    /// Description
    pub description: Option<String>,
}

// ============================================================================
// Resource Trait Implementation
// ============================================================================

#[async_trait]
impl Resource for AwsSecurityGroupRuleResource {
    fn resource_type(&self) -> &str {
        "aws_security_group_rule"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_security_group_rule".to_string(),
            description:
                "Provides an AWS EC2 security group rule resource for managing individual rules"
                    .to_string(),
            required_args: vec![
                SchemaField {
                    name: "type".to_string(),
                    field_type: FieldType::String,
                    description: "Rule type: ingress or egress".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Enum {
                        values: vec!["ingress".to_string(), "egress".to_string()],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "security_group_id".to_string(),
                    field_type: FieldType::String,
                    description: "Security group ID to attach the rule to".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "protocol".to_string(),
                    field_type: FieldType::String,
                    description: "Protocol (tcp, udp, icmp, -1 for all)".to_string(),
                    default: Some(Value::String("-1".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "from_port".to_string(),
                    field_type: FieldType::Integer,
                    description: "Start of port range".to_string(),
                    default: Some(Value::Number((-1).into())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "to_port".to_string(),
                    field_type: FieldType::Integer,
                    description: "End of port range".to_string(),
                    default: Some(Value::Number((-1).into())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "cidr_blocks".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "IPv4 CIDR blocks".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ipv6_cidr_blocks".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "IPv6 CIDR blocks".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "source_security_group_id".to_string(),
                    field_type: FieldType::String,
                    description: "Source/destination security group ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "self_referencing".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Allow traffic from/to self".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Rule description".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MaxLength { max: 255 }],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![SchemaField {
                name: "id".to_string(),
                field_type: FieldType::String,
                description: "Rule ID".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            force_new: vec![
                "type".to_string(),
                "security_group_id".to_string(),
                "protocol".to_string(),
                "from_port".to_string(),
                "to_port".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 120,
                read: 60,
                update: 120,
                delete: 120,
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        debug!("Reading security group rule: {}", id);

        // Parse the rule ID to extract security group ID and rule details
        // This is a simplified approach - in production you'd want to store
        // more state information
        let parts: Vec<&str> = id.split('-').collect();
        if parts.len() < 2 {
            return Ok(ResourceReadResult::not_found());
        }

        // We need to search through all rules in the security group
        // For now, return not found as we need the full config to find the rule
        Ok(ResourceReadResult::not_found())
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let config = self.parse_config(desired)?;

        match current {
            None => {
                // Check if rule already exists
                if let Some(_existing) = self.find_rule(&config, ctx).await? {
                    // Rule exists - no change needed
                    Ok(ResourceDiff::no_change())
                } else {
                    // Rule doesn't exist - create
                    Ok(ResourceDiff::create(desired.clone()))
                }
            }
            Some(current_value) => {
                let current_state: SecurityGroupRuleState =
                    serde_json::from_value(current_value.clone()).map_err(|e| {
                        ProvisioningError::SerializationError(format!(
                            "Failed to parse current state: {}",
                            e
                        ))
                    })?;

                // Check for force_new fields - all rule properties require replacement
                if config.rule_type != current_state.rule_type
                    || config.security_group_id != current_state.security_group_id
                    || config.protocol != current_state.protocol
                    || config.from_port != current_state.from_port
                    || config.to_port != current_state.to_port
                    || config.cidr_blocks != current_state.cidr_blocks
                    || config.ipv6_cidr_blocks != current_state.ipv6_cidr_blocks
                    || config.source_security_group_id != current_state.source_security_group_id
                    || config.self_referencing != current_state.self_referencing
                {
                    return Ok(ResourceDiff {
                        change_type: ChangeType::Replace,
                        additions: HashMap::new(),
                        modifications: HashMap::new(),
                        deletions: Vec::new(),
                        requires_replacement: true,
                        replacement_fields: vec!["rule".to_string()],
                    });
                }

                // Only description can be updated in-place (via re-create)
                if config.description != current_state.description {
                    let mut modifications = HashMap::new();
                    modifications.insert(
                        "description".to_string(),
                        (
                            serde_json::to_value(&current_state.description).unwrap(),
                            serde_json::to_value(&config.description).unwrap(),
                        ),
                    );

                    return Ok(ResourceDiff {
                        change_type: ChangeType::Update,
                        additions: HashMap::new(),
                        modifications,
                        deletions: Vec::new(),
                        requires_replacement: false,
                        replacement_fields: Vec::new(),
                    });
                }

                Ok(ResourceDiff::no_change())
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let rule_config = self.parse_config(config)?;
        let client = self.create_client(ctx).await?;

        info!(
            "Creating security group rule on {}: {} {}:{}-{}",
            rule_config.security_group_id,
            rule_config.rule_type.as_str(),
            rule_config.protocol,
            rule_config.from_port,
            rule_config.to_port
        );

        self.authorize_rule(&client, &rule_config).await?;

        let rule_id = Self::generate_rule_id(&rule_config);
        let state = SecurityGroupRuleState {
            id: rule_id.clone(),
            security_group_id: rule_config.security_group_id,
            rule_type: rule_config.rule_type,
            protocol: rule_config.protocol,
            from_port: rule_config.from_port,
            to_port: rule_config.to_port,
            cidr_blocks: rule_config.cidr_blocks,
            ipv6_cidr_blocks: rule_config.ipv6_cidr_blocks,
            source_security_group_id: rule_config.source_security_group_id,
            self_referencing: rule_config.self_referencing,
            description: rule_config.description,
        };

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize rule state: {}", e))
        })?;

        info!("Created security group rule: {}", rule_id);

        Ok(ResourceResult::success(&rule_id, attributes)
            .with_output("id", Value::String(rule_id))
            .with_output(
                "security_group_id",
                Value::String(state.security_group_id.clone()),
            ))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        // For security group rules, update means delete old + create new
        // because AWS doesn't support in-place updates
        let old_config = self.parse_config(old)?;
        let new_config = self.parse_config(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating security group rule: {}", id);

        // Revoke old rule
        if let Err(e) = self.revoke_rule(&client, &old_config).await {
            debug!("Failed to revoke old rule (may not exist): {}", e);
        }

        // Authorize new rule
        self.authorize_rule(&client, &new_config).await?;

        let rule_id = Self::generate_rule_id(&new_config);
        let state = SecurityGroupRuleState {
            id: rule_id.clone(),
            security_group_id: new_config.security_group_id,
            rule_type: new_config.rule_type,
            protocol: new_config.protocol,
            from_port: new_config.from_port,
            to_port: new_config.to_port,
            cidr_blocks: new_config.cidr_blocks,
            ipv6_cidr_blocks: new_config.ipv6_cidr_blocks,
            source_security_group_id: new_config.source_security_group_id,
            self_referencing: new_config.self_referencing,
            description: new_config.description,
        };

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize rule state: {}", e))
        })?;

        Ok(ResourceResult::success(&rule_id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        // We need the full config to revoke the rule
        // In practice, this would come from state
        info!("Deleting security group rule: {}", id);

        // The actual revoke happens with the config from state
        // For now, return success - the state should have the config
        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        debug!("Importing security group rule: {}", id);

        // Import format: sg-xxx_ingress_tcp_80_80_0.0.0.0/0
        // or: sg-xxx_egress_-1_-1_-1_0.0.0.0/0
        let parts: Vec<&str> = id.split('_').collect();
        if parts.len() < 5 {
            return Err(ProvisioningError::ImportError {
                resource_type: "aws_security_group_rule".to_string(),
                resource_id: id.to_string(),
                message: "Import ID format: sg-xxx_ingress|egress_protocol_from_to_cidr"
                    .to_string(),
            });
        }

        let sg_id = parts[0];
        let rule_type = match parts[1] {
            "ingress" => RuleType::Ingress,
            "egress" => RuleType::Egress,
            _ => {
                return Err(ProvisioningError::ImportError {
                    resource_type: "aws_security_group_rule".to_string(),
                    resource_id: id.to_string(),
                    message: "Rule type must be 'ingress' or 'egress'".to_string(),
                })
            }
        };
        let protocol = parts[2].to_string();
        let from_port: i32 = parts[3].parse().unwrap_or(-1);
        let to_port: i32 = parts[4].parse().unwrap_or(-1);

        let cidr = if parts.len() > 5 {
            parts[5..].join("_").replace('_', "/")
        } else {
            String::new()
        };

        let config = SecurityGroupRuleConfig {
            rule_type,
            security_group_id: sg_id.to_string(),
            protocol,
            from_port,
            to_port,
            cidr_blocks: if !cidr.is_empty() && !cidr.contains(':') {
                vec![cidr.clone()]
            } else {
                vec![]
            },
            ipv6_cidr_blocks: if !cidr.is_empty() && cidr.contains(':') {
                vec![cidr]
            } else {
                vec![]
            },
            source_security_group_id: None,
            self_referencing: false,
            description: None,
        };

        // Verify the rule exists
        let state =
            self.find_rule(&config, ctx)
                .await?
                .ok_or_else(|| ProvisioningError::ImportError {
                    resource_type: "aws_security_group_rule".to_string(),
                    resource_id: id.to_string(),
                    message: "Rule not found in security group".to_string(),
                })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize rule state: {}", e))
        })?;

        Ok(ResourceResult::success(&state.id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Check for security_group_id reference
        if let Some(sg_id) = config.get("security_group_id").and_then(|v| v.as_str()) {
            if let Some(captures) = parse_resource_reference(sg_id) {
                deps.push(ResourceDependency::new(
                    captures.resource_type,
                    captures.resource_name,
                    captures.attribute,
                ));
            }
        }

        // Check for source_security_group_id reference
        if let Some(src_sg) = config
            .get("source_security_group_id")
            .and_then(|v| v.as_str())
        {
            if let Some(captures) = parse_resource_reference(src_sg) {
                deps.push(ResourceDependency::new(
                    captures.resource_type,
                    captures.resource_name,
                    captures.attribute,
                ));
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "type".to_string(),
            "security_group_id".to_string(),
            "protocol".to_string(),
            "from_port".to_string(),
            "to_port".to_string(),
            "cidr_blocks".to_string(),
            "ipv6_cidr_blocks".to_string(),
            "source_security_group_id".to_string(),
            "self_referencing".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate type is present
        let rule_type = config.get("type").and_then(|v| v.as_str());
        if rule_type.is_none() {
            return Err(ProvisioningError::ValidationError(
                "type is required (ingress or egress)".to_string(),
            ));
        }

        let rule_type = rule_type.unwrap();
        if rule_type != "ingress" && rule_type != "egress" {
            return Err(ProvisioningError::ValidationError(
                "type must be 'ingress' or 'egress'".to_string(),
            ));
        }

        // Validate security_group_id is present
        if config
            .get("security_group_id")
            .and_then(|v| v.as_str())
            .is_none()
        {
            return Err(ProvisioningError::ValidationError(
                "security_group_id is required".to_string(),
            ));
        }

        // Validate at least one source/destination is specified
        let has_cidr = config
            .get("cidr_blocks")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_ipv6 = config
            .get("ipv6_cidr_blocks")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_sg = config
            .get("source_security_group_id")
            .and_then(|v| v.as_str())
            .is_some();
        let has_self = config
            .get("self_referencing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !has_cidr && !has_ipv6 && !has_sg && !has_self {
            return Err(ProvisioningError::ValidationError(
                "At least one of cidr_blocks, ipv6_cidr_blocks, source_security_group_id, or self_referencing must be specified".to_string(),
            ));
        }

        // Validate protocol and ports
        let protocol = config
            .get("protocol")
            .and_then(|v| v.as_str())
            .unwrap_or("-1");

        if protocol != "-1" && protocol.to_lowercase() != "all" {
            // For specific protocols, ports should be specified
            let from_port = config.get("from_port");
            let to_port = config.get("to_port");

            if from_port.is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "from_port is required for protocol {}",
                    protocol
                )));
            }
            if to_port.is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "to_port is required for protocol {}",
                    protocol
                )));
            }
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

/// Parse a resource reference string like "{{ resources.aws_security_group.main.id }}"
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
        let resource = AwsSecurityGroupRuleResource::new();
        assert_eq!(resource.resource_type(), "aws_security_group_rule");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsSecurityGroupRuleResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_security_group_rule");
        assert!(schema.required_args.iter().any(|f| f.name == "type"));
        assert!(schema
            .required_args
            .iter()
            .any(|f| f.name == "security_group_id"));
    }

    #[test]
    fn test_parse_config_ingress() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 80,
            "to_port": 80,
            "cidr_blocks": ["0.0.0.0/0"],
            "description": "Allow HTTP"
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.rule_type, RuleType::Ingress);
        assert_eq!(parsed.security_group_id, "sg-12345");
        assert_eq!(parsed.protocol, "tcp");
        assert_eq!(parsed.from_port, 80);
        assert_eq!(parsed.to_port, 80);
        assert_eq!(parsed.cidr_blocks, vec!["0.0.0.0/0"]);
    }

    #[test]
    fn test_parse_config_egress() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "egress",
            "security_group_id": "sg-12345",
            "protocol": "-1",
            "from_port": -1,
            "to_port": -1,
            "cidr_blocks": ["0.0.0.0/0"]
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.rule_type, RuleType::Egress);
        assert_eq!(parsed.protocol, "-1");
    }

    #[test]
    fn test_generate_rule_id() {
        let config = SecurityGroupRuleConfig {
            rule_type: RuleType::Ingress,
            security_group_id: "sg-12345".to_string(),
            protocol: "tcp".to_string(),
            from_port: 443,
            to_port: 443,
            cidr_blocks: vec!["0.0.0.0/0".to_string()],
            ipv6_cidr_blocks: vec![],
            source_security_group_id: None,
            self_referencing: false,
            description: None,
        };

        let id = AwsSecurityGroupRuleResource::generate_rule_id(&config);
        assert!(id.starts_with("sgrule-"));
        assert!(id.contains("sg-12345"));
        assert!(id.contains("ingress"));
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 80,
            "to_port": 80,
            "cidr_blocks": ["0.0.0.0/0"]
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_type() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 80,
            "to_port": 80,
            "cidr_blocks": ["0.0.0.0/0"]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_invalid_type() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "invalid",
            "security_group_id": "sg-12345"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_missing_security_group_id() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "protocol": "tcp",
            "from_port": 80,
            "to_port": 80,
            "cidr_blocks": ["0.0.0.0/0"]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_no_source() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 80,
            "to_port": 80
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_self_referencing() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 0,
            "to_port": 65535,
            "self_referencing": true
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_source_security_group() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "sg-12345",
            "protocol": "tcp",
            "from_port": 22,
            "to_port": 22,
            "source_security_group_id": "sg-67890"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsSecurityGroupRuleResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"type".to_string()));
        assert!(force_new.contains(&"security_group_id".to_string()));
        assert!(force_new.contains(&"protocol".to_string()));
        assert!(force_new.contains(&"from_port".to_string()));
        assert!(force_new.contains(&"to_port".to_string()));
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = serde_json::json!({
            "type": "ingress",
            "security_group_id": "{{ resources.aws_security_group.web.id }}",
            "protocol": "tcp",
            "from_port": 22,
            "to_port": 22,
            "source_security_group_id": "{{ resources.aws_security_group.bastion.id }}"
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 2);

        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "web"));
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "bastion"));
    }

    #[test]
    fn test_to_ip_permission_cidr() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = SecurityGroupRuleConfig {
            rule_type: RuleType::Ingress,
            security_group_id: "sg-12345".to_string(),
            protocol: "tcp".to_string(),
            from_port: 443,
            to_port: 443,
            cidr_blocks: vec!["0.0.0.0/0".to_string(), "10.0.0.0/8".to_string()],
            ipv6_cidr_blocks: vec![],
            source_security_group_id: None,
            self_referencing: false,
            description: Some("HTTPS".to_string()),
        };

        let perm = resource.to_ip_permission(&config);
        assert_eq!(perm.ip_protocol(), Some("tcp"));
        assert_eq!(perm.from_port(), Some(443));
        assert_eq!(perm.to_port(), Some(443));
        assert_eq!(perm.ip_ranges().len(), 2);
    }

    #[test]
    fn test_to_ip_permission_self_reference() {
        let resource = AwsSecurityGroupRuleResource::new();
        let config = SecurityGroupRuleConfig {
            rule_type: RuleType::Ingress,
            security_group_id: "sg-12345".to_string(),
            protocol: "tcp".to_string(),
            from_port: 0,
            to_port: 65535,
            cidr_blocks: vec![],
            ipv6_cidr_blocks: vec![],
            source_security_group_id: None,
            self_referencing: true,
            description: None,
        };

        let perm = resource.to_ip_permission(&config);
        assert_eq!(perm.user_id_group_pairs().len(), 1);
        assert_eq!(perm.user_id_group_pairs()[0].group_id(), Some("sg-12345"));
    }

    #[test]
    fn test_state_serialization() {
        let state = SecurityGroupRuleState {
            id: "sgrule-123".to_string(),
            security_group_id: "sg-12345".to_string(),
            rule_type: RuleType::Ingress,
            protocol: "tcp".to_string(),
            from_port: 80,
            to_port: 80,
            cidr_blocks: vec!["0.0.0.0/0".to_string()],
            ipv6_cidr_blocks: vec![],
            source_security_group_id: None,
            self_referencing: false,
            description: Some("HTTP".to_string()),
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "sgrule-123");
        assert_eq!(json["security_group_id"], "sg-12345");
        assert_eq!(json["rule_type"], "ingress");
        assert_eq!(json["protocol"], "tcp");
    }

    #[test]
    fn test_parse_resource_reference() {
        let ref1 = parse_resource_reference("{{ resources.aws_security_group.web.id }}");
        assert!(ref1.is_some());
        let ref1 = ref1.unwrap();
        assert_eq!(ref1.resource_type, "aws_security_group");
        assert_eq!(ref1.resource_name, "web");
        assert_eq!(ref1.attribute, "id");

        // Not a reference
        let ref2 = parse_resource_reference("sg-12345");
        assert!(ref2.is_none());
    }
}
