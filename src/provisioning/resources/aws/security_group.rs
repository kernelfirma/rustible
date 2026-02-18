//! AWS Security Group Resource for Infrastructure Provisioning
//!
//! This module implements the `aws_security_group` resource type following
//! the Resource trait pattern for declarative infrastructure management.
//!
//! # Example
//!
//! ```yaml
//! resources:
//!   aws_security_group:
//!     web_sg:
//!       name: web-security-group
//!       description: Allow HTTP/HTTPS traffic
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//!       ingress:
//!         - protocol: tcp
//!           from_port: 80
//!           to_port: 80
//!           cidr_blocks:
//!             - "0.0.0.0/0"
//!         - protocol: tcp
//!           from_port: 443
//!           to_port: 443
//!           cidr_blocks:
//!             - "0.0.0.0/0"
//!       egress:
//!         - protocol: "-1"
//!           from_port: 0
//!           to_port: 0
//!           cidr_blocks:
//!             - "0.0.0.0/0"
//!       tags:
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{
    Filter, IpPermission, IpRange, Ipv6Range, ResourceType, Tag, TagSpecification, UserIdGroupPair,
};
use aws_sdk_ec2::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Security Group Rule Configuration
// ============================================================================

/// Configuration for a single security group rule
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityGroupRuleConfig {
    /// IP protocol: tcp, udp, icmp, or -1 for all
    pub protocol: String,

    /// Start of port range (use -1 for all with protocol -1)
    pub from_port: i32,

    /// End of port range (use -1 for all with protocol -1)
    pub to_port: i32,

    /// IPv4 CIDR blocks
    #[serde(default)]
    pub cidr_blocks: Vec<String>,

    /// IPv6 CIDR blocks
    #[serde(default)]
    pub ipv6_cidr_blocks: Vec<String>,

    /// Source/destination security group IDs
    #[serde(default)]
    pub security_groups: Vec<String>,

    /// Allow traffic from/to the security group itself
    #[serde(default)]
    pub self_referencing: bool,

    /// Rule description
    #[serde(default)]
    pub description: Option<String>,
}

impl SecurityGroupRuleConfig {
    /// Convert to AWS SDK IpPermission
    fn to_ip_permission(&self, self_group_id: Option<&str>) -> IpPermission {
        let mut builder = IpPermission::builder()
            .ip_protocol(&self.protocol)
            .from_port(self.from_port)
            .to_port(self.to_port);

        // Add IPv4 CIDR blocks
        for cidr in &self.cidr_blocks {
            let mut ip_range = IpRange::builder().cidr_ip(cidr);
            if let Some(ref desc) = self.description {
                ip_range = ip_range.description(desc);
            }
            builder = builder.ip_ranges(ip_range.build());
        }

        // Add IPv6 CIDR blocks
        for cidr_v6 in &self.ipv6_cidr_blocks {
            let mut ipv6_range = Ipv6Range::builder().cidr_ipv6(cidr_v6);
            if let Some(ref desc) = self.description {
                ipv6_range = ipv6_range.description(desc);
            }
            builder = builder.ipv6_ranges(ipv6_range.build());
        }

        // Add security group references
        for sg_id in &self.security_groups {
            let mut user_id_group = UserIdGroupPair::builder().group_id(sg_id);
            if let Some(ref desc) = self.description {
                user_id_group = user_id_group.description(desc);
            }
            builder = builder.user_id_group_pairs(user_id_group.build());
        }

        // Handle self-referencing
        if self.self_referencing {
            if let Some(group_id) = self_group_id {
                let mut user_id_group = UserIdGroupPair::builder().group_id(group_id);
                if let Some(ref desc) = self.description {
                    user_id_group = user_id_group.description(desc);
                }
                builder = builder.user_id_group_pairs(user_id_group.build());
            }
        }

        builder.build()
    }

    /// Create a unique key for this rule (for diff comparison)
    fn rule_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            self.protocol,
            self.from_port,
            self.to_port,
            self.cidr_blocks.join(","),
            self.ipv6_cidr_blocks.join(","),
            self.security_groups.join(","),
            self.self_referencing
        )
    }
}

// ============================================================================
// AWS Security Group Resource
// ============================================================================

/// AWS Security Group resource implementation
#[derive(Debug, Clone)]
pub struct AwsSecurityGroupResource;

impl AwsSecurityGroupResource {
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

    /// Find security group by ID
    async fn find_by_id(
        &self,
        group_id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<SecurityGroupState>> {
        let client = self.create_client(ctx).await?;

        let resp = client
            .describe_security_groups()
            .group_ids(group_id)
            .send()
            .await
            .map_err(|e| {
                if e.to_string().contains("InvalidGroup.NotFound") {
                    return ProvisioningError::CloudApiError(format!(
                        "Security group not found: {}",
                        group_id
                    ));
                }
                ProvisioningError::CloudApiError(format!(
                    "Failed to describe security group: {}",
                    e
                ))
            });

        // Handle NotFound gracefully
        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.to_string().contains("not found") => return Ok(None),
            Err(e) => return Err(e),
        };

        if let Some(sg) = resp.security_groups().iter().next() {
            return Ok(Some(self.parse_security_group(sg)));
        }

        Ok(None)
    }

    /// Find security group by name and VPC
    async fn find_by_name(
        &self,
        name: &str,
        vpc_id: Option<&str>,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<SecurityGroupState>> {
        let client = self.create_client(ctx).await?;

        let mut req = client
            .describe_security_groups()
            .filters(Filter::builder().name("group-name").values(name).build());

        if let Some(vpc) = vpc_id {
            req = req.filters(Filter::builder().name("vpc-id").values(vpc).build());
        }

        let resp = req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to describe security groups: {}", e))
        })?;

        if let Some(sg) = resp.security_groups().iter().next() {
            return Ok(Some(self.parse_security_group(sg)));
        }

        Ok(None)
    }

    /// Parse AWS SecurityGroup into our state
    fn parse_security_group(&self, sg: &aws_sdk_ec2::types::SecurityGroup) -> SecurityGroupState {
        let mut tags = HashMap::new();
        for tag in sg.tags() {
            if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                tags.insert(key.to_string(), value.to_string());
            }
        }

        // Parse ingress rules
        let ingress = sg
            .ip_permissions()
            .iter()
            .flat_map(|perm| self.parse_ip_permission(perm))
            .collect();

        // Parse egress rules
        let egress = sg
            .ip_permissions_egress()
            .iter()
            .flat_map(|perm| self.parse_ip_permission(perm))
            .collect();

        SecurityGroupState {
            id: sg.group_id().unwrap_or_default().to_string(),
            arn: format!(
                "arn:aws:ec2:{}:{}:security-group/{}",
                "", // Region would come from context
                sg.owner_id().unwrap_or_default(),
                sg.group_id().unwrap_or_default()
            ),
            name: sg.group_name().unwrap_or_default().to_string(),
            description: sg.description().unwrap_or_default().to_string(),
            vpc_id: sg.vpc_id().map(|s| s.to_string()),
            owner_id: sg.owner_id().unwrap_or_default().to_string(),
            ingress,
            egress,
            tags,
        }
    }

    /// Parse IpPermission into SecurityGroupRuleConfig(s)
    fn parse_ip_permission(&self, perm: &IpPermission) -> Vec<SecurityGroupRuleConfig> {
        let protocol = perm.ip_protocol().unwrap_or("-1").to_string();
        let from_port = perm.from_port().unwrap_or(-1);
        let to_port = perm.to_port().unwrap_or(-1);

        let mut rules = Vec::new();

        // IPv4 CIDR blocks
        let cidr_blocks: Vec<String> = perm
            .ip_ranges()
            .iter()
            .filter_map(|r| r.cidr_ip().map(|s| s.to_string()))
            .collect();

        // IPv6 CIDR blocks
        let ipv6_cidr_blocks: Vec<String> = perm
            .ipv6_ranges()
            .iter()
            .filter_map(|r| r.cidr_ipv6().map(|s| s.to_string()))
            .collect();

        // Security group references
        let security_groups: Vec<String> = perm
            .user_id_group_pairs()
            .iter()
            .filter_map(|p| p.group_id().map(|s| s.to_string()))
            .collect();

        // Get description from first range if available
        let description = perm
            .ip_ranges()
            .first()
            .and_then(|r| r.description().map(|s| s.to_string()))
            .or_else(|| {
                perm.ipv6_ranges()
                    .first()
                    .and_then(|r| r.description().map(|s| s.to_string()))
            });

        // Create a single consolidated rule
        if !cidr_blocks.is_empty() || !ipv6_cidr_blocks.is_empty() || !security_groups.is_empty() {
            rules.push(SecurityGroupRuleConfig {
                protocol: protocol.clone(),
                from_port,
                to_port,
                cidr_blocks,
                ipv6_cidr_blocks,
                security_groups,
                self_referencing: false,
                description,
            });
        }

        rules
    }

    /// Parse configuration from JSON Value
    fn parse_config(&self, config: &Value) -> ProvisioningResult<SecurityGroupConfig> {
        serde_json::from_value(config.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!(
                "Invalid security group configuration: {}",
                e
            ))
        })
    }

    /// Compute diff between current and desired rules
    fn diff_rules(
        &self,
        current: &[SecurityGroupRuleConfig],
        desired: &[SecurityGroupRuleConfig],
    ) -> (Vec<SecurityGroupRuleConfig>, Vec<SecurityGroupRuleConfig>) {
        let current_keys: std::collections::HashSet<String> =
            current.iter().map(|r| r.rule_key()).collect();
        let desired_keys: std::collections::HashSet<String> =
            desired.iter().map(|r| r.rule_key()).collect();

        // Rules to add (in desired but not current)
        let to_add: Vec<SecurityGroupRuleConfig> = desired
            .iter()
            .filter(|r| !current_keys.contains(&r.rule_key()))
            .cloned()
            .collect();

        // Rules to remove (in current but not desired)
        let to_remove: Vec<SecurityGroupRuleConfig> = current
            .iter()
            .filter(|r| !desired_keys.contains(&r.rule_key()))
            .cloned()
            .collect();

        (to_add, to_remove)
    }

    /// Authorize ingress rules
    async fn authorize_ingress(
        &self,
        client: &Client,
        group_id: &str,
        rules: &[SecurityGroupRuleConfig],
    ) -> ProvisioningResult<()> {
        if rules.is_empty() {
            return Ok(());
        }

        let ip_permissions: Vec<IpPermission> = rules
            .iter()
            .map(|r| r.to_ip_permission(Some(group_id)))
            .collect();

        client
            .authorize_security_group_ingress()
            .group_id(group_id)
            .set_ip_permissions(Some(ip_permissions))
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!(
                    "Failed to authorize ingress rules: {}",
                    e
                ))
            })?;

        debug!("Authorized {} ingress rules for {}", rules.len(), group_id);
        Ok(())
    }

    /// Authorize egress rules
    async fn authorize_egress(
        &self,
        client: &Client,
        group_id: &str,
        rules: &[SecurityGroupRuleConfig],
    ) -> ProvisioningResult<()> {
        if rules.is_empty() {
            return Ok(());
        }

        let ip_permissions: Vec<IpPermission> = rules
            .iter()
            .map(|r| r.to_ip_permission(Some(group_id)))
            .collect();

        client
            .authorize_security_group_egress()
            .group_id(group_id)
            .set_ip_permissions(Some(ip_permissions))
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to authorize egress rules: {}", e))
            })?;

        debug!("Authorized {} egress rules for {}", rules.len(), group_id);
        Ok(())
    }

    /// Revoke ingress rules
    async fn revoke_ingress(
        &self,
        client: &Client,
        group_id: &str,
        rules: &[SecurityGroupRuleConfig],
    ) -> ProvisioningResult<()> {
        if rules.is_empty() {
            return Ok(());
        }

        let ip_permissions: Vec<IpPermission> = rules
            .iter()
            .map(|r| r.to_ip_permission(Some(group_id)))
            .collect();

        client
            .revoke_security_group_ingress()
            .group_id(group_id)
            .set_ip_permissions(Some(ip_permissions))
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to revoke ingress rules: {}", e))
            })?;

        debug!("Revoked {} ingress rules for {}", rules.len(), group_id);
        Ok(())
    }

    /// Revoke egress rules
    async fn revoke_egress(
        &self,
        client: &Client,
        group_id: &str,
        rules: &[SecurityGroupRuleConfig],
    ) -> ProvisioningResult<()> {
        if rules.is_empty() {
            return Ok(());
        }

        let ip_permissions: Vec<IpPermission> = rules
            .iter()
            .map(|r| r.to_ip_permission(Some(group_id)))
            .collect();

        client
            .revoke_security_group_egress()
            .group_id(group_id)
            .set_ip_permissions(Some(ip_permissions))
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to revoke egress rules: {}", e))
            })?;

        debug!("Revoked {} egress rules for {}", rules.len(), group_id);
        Ok(())
    }
}

impl Default for AwsSecurityGroupResource {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Security Group Configuration (from YAML/JSON)
// ============================================================================

/// Security group configuration as parsed from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupConfig {
    /// Security group name (required)
    pub name: String,

    /// Description of the security group
    #[serde(default = "default_description")]
    pub description: String,

    /// VPC ID (required for VPC security groups)
    #[serde(default)]
    pub vpc_id: Option<String>,

    /// Ingress (inbound) rules
    #[serde(default)]
    pub ingress: Vec<SecurityGroupRuleConfig>,

    /// Egress (outbound) rules
    #[serde(default)]
    pub egress: Vec<SecurityGroupRuleConfig>,

    /// Resource tags
    #[serde(default)]
    pub tags: HashMap<String, String>,

    /// Revoke all rules on delete (helps avoid dependency issues)
    #[serde(default)]
    pub revoke_rules_on_delete: bool,
}

fn default_description() -> String {
    "Managed by Rustible".to_string()
}

// ============================================================================
// Security Group State (from AWS)
// ============================================================================

/// Current state of a security group from AWS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupState {
    /// Security group ID
    pub id: String,

    /// Security group ARN
    pub arn: String,

    /// Security group name
    pub name: String,

    /// Description
    pub description: String,

    /// VPC ID
    pub vpc_id: Option<String>,

    /// Owner account ID
    pub owner_id: String,

    /// Ingress rules
    pub ingress: Vec<SecurityGroupRuleConfig>,

    /// Egress rules
    pub egress: Vec<SecurityGroupRuleConfig>,

    /// Tags
    pub tags: HashMap<String, String>,
}

// ============================================================================
// Resource Trait Implementation
// ============================================================================

#[async_trait]
impl Resource for AwsSecurityGroupResource {
    fn resource_type(&self) -> &str {
        "aws_security_group"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_security_group".to_string(),
            description:
                "Provides an AWS EC2 security group resource for controlling network traffic"
                    .to_string(),
            required_args: vec![SchemaField {
                name: "name".to_string(),
                field_type: FieldType::String,
                description: "Name of the security group".to_string(),
                default: None,
                constraints: vec![
                    FieldConstraint::MinLength { min: 1 },
                    FieldConstraint::MaxLength { max: 255 },
                ],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Description of the security group".to_string(),
                    default: Some(Value::String("Managed by Rustible".to_string())),
                    constraints: vec![FieldConstraint::MaxLength { max: 255 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ID for the security group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ingress".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Object(vec![
                        SchemaField {
                            name: "protocol".to_string(),
                            field_type: FieldType::String,
                            description: "Protocol (tcp, udp, icmp, -1 for all)".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "from_port".to_string(),
                            field_type: FieldType::Integer,
                            description: "Start of port range".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "to_port".to_string(),
                            field_type: FieldType::Integer,
                            description: "End of port range".to_string(),
                            default: None,
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
                    ]))),
                    description: "Ingress (inbound) rules".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "egress".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Object(vec![]))),
                    description: "Egress (outbound) rules".to_string(),
                    default: Some(Value::Array(vec![])),
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
                SchemaField {
                    name: "revoke_rules_on_delete".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Revoke all rules before deletion".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Security group ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "Security group ARN".to_string(),
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
            force_new: vec!["name".to_string(), "vpc_id".to_string()],
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
        debug!("Reading security group: {}", id);

        match self.find_by_id(id, ctx).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize security group state: {}",
                        e
                    ))
                })?;

                Ok(ResourceReadResult::found(&state.id, attributes))
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
                let current_state: SecurityGroupState =
                    serde_json::from_value(current_value.clone()).map_err(|e| {
                        ProvisioningError::SerializationError(format!(
                            "Failed to parse current state: {}",
                            e
                        ))
                    })?;

                // Check for force_new fields
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                if config.name != current_state.name {
                    requires_replacement = true;
                    replacement_fields.push("name".to_string());
                }

                if config.vpc_id != current_state.vpc_id {
                    requires_replacement = true;
                    replacement_fields.push("vpc_id".to_string());
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

                // Check for rule changes
                let (ingress_to_add, ingress_to_remove) =
                    self.diff_rules(&current_state.ingress, &config.ingress);
                let (egress_to_add, egress_to_remove) =
                    self.diff_rules(&current_state.egress, &config.egress);

                // Check for tag changes
                let tags_changed = config.tags != current_state.tags;

                if ingress_to_add.is_empty()
                    && ingress_to_remove.is_empty()
                    && egress_to_add.is_empty()
                    && egress_to_remove.is_empty()
                    && !tags_changed
                {
                    return Ok(ResourceDiff::no_change());
                }

                let mut modifications = HashMap::new();

                if !ingress_to_add.is_empty() || !ingress_to_remove.is_empty() {
                    modifications.insert(
                        "ingress".to_string(),
                        (
                            serde_json::to_value(&current_state.ingress).unwrap(),
                            serde_json::to_value(&config.ingress).unwrap(),
                        ),
                    );
                }

                if !egress_to_add.is_empty() || !egress_to_remove.is_empty() {
                    modifications.insert(
                        "egress".to_string(),
                        (
                            serde_json::to_value(&current_state.egress).unwrap(),
                            serde_json::to_value(&config.egress).unwrap(),
                        ),
                    );
                }

                if tags_changed {
                    modifications.insert(
                        "tags".to_string(),
                        (
                            serde_json::to_value(&current_state.tags).unwrap(),
                            serde_json::to_value(&config.tags).unwrap(),
                        ),
                    );
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
        let sg_config = self.parse_config(config)?;
        let client = self.create_client(ctx).await?;

        info!("Creating security group: {}", sg_config.name);

        // Build tags
        let mut tags = vec![Tag::builder().key("Name").value(&sg_config.name).build()];

        for (key, value) in &sg_config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }

        // Apply default tags from provider context
        for (key, value) in &ctx.default_tags {
            if !sg_config.tags.contains_key(key) {
                tags.push(Tag::builder().key(key).value(value).build());
            }
        }

        // Create the security group
        let mut req = client
            .create_security_group()
            .group_name(&sg_config.name)
            .description(&sg_config.description)
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::SecurityGroup)
                    .set_tags(Some(tags))
                    .build(),
            );

        if let Some(ref vpc_id) = sg_config.vpc_id {
            req = req.vpc_id(vpc_id);
        }

        let resp = req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create security group: {}", e))
        })?;

        let group_id = resp
            .group_id()
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("No group ID in create response".to_string())
            })?
            .to_string();

        info!(
            "Created security group {} with ID {}",
            sg_config.name, group_id
        );

        // Authorize ingress rules
        if !sg_config.ingress.is_empty() {
            self.authorize_ingress(&client, &group_id, &sg_config.ingress)
                .await?;
        }

        // For VPC security groups, we need to remove the default egress rule
        // before adding our custom ones (if any specified)
        if sg_config.vpc_id.is_some() && !sg_config.egress.is_empty() {
            // Remove default "allow all" egress rule
            let default_egress = vec![SecurityGroupRuleConfig {
                protocol: "-1".to_string(),
                from_port: -1,
                to_port: -1,
                cidr_blocks: vec!["0.0.0.0/0".to_string()],
                ipv6_cidr_blocks: vec![],
                security_groups: vec![],
                self_referencing: false,
                description: None,
            }];

            // Ignore errors - rule may not exist
            let _ = self
                .revoke_egress(&client, &group_id, &default_egress)
                .await;
        }

        // Authorize egress rules
        if !sg_config.egress.is_empty() {
            self.authorize_egress(&client, &group_id, &sg_config.egress)
                .await?;
        }

        // Read back the created security group
        let state = self.find_by_id(&group_id, ctx).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("Security group not found after creation".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize security group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(&group_id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("owner_id", Value::String(state.owner_id.clone())))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let old_state: SecurityGroupState = serde_json::from_value(old.clone()).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to parse old state: {}", e))
        })?;

        let new_config = self.parse_config(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating security group: {}", id);

        // Compute rule differences
        let (ingress_to_add, ingress_to_remove) =
            self.diff_rules(&old_state.ingress, &new_config.ingress);
        let (egress_to_add, egress_to_remove) =
            self.diff_rules(&old_state.egress, &new_config.egress);

        // Revoke removed rules first
        if !ingress_to_remove.is_empty() {
            self.revoke_ingress(&client, id, &ingress_to_remove).await?;
        }

        if !egress_to_remove.is_empty() {
            self.revoke_egress(&client, id, &egress_to_remove).await?;
        }

        // Authorize new rules
        if !ingress_to_add.is_empty() {
            self.authorize_ingress(&client, id, &ingress_to_add).await?;
        }

        if !egress_to_add.is_empty() {
            self.authorize_egress(&client, id, &egress_to_add).await?;
        }

        // Update tags if changed
        if new_config.tags != old_state.tags {
            // Collect old tag keys to delete
            let old_keys: Vec<String> = old_state
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k) && *k != "Name")
                .cloned()
                .collect();

            if !old_keys.is_empty() {
                let delete_tags: Vec<Tag> = old_keys
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

            // Create/update tags
            let new_tags: Vec<Tag> = new_config
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

        // Read back updated state
        let state = self.find_by_id(id, ctx).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("Security group not found after update".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize security group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("owner_id", Value::String(state.owner_id.clone())))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        info!("Deleting security group: {}", id);

        // Optionally revoke all rules first (helps with dependencies)
        if let Ok(Some(state)) = self.find_by_id(id, ctx).await {
            // Check if revoke_rules_on_delete was set (we'd need to track this in state)
            // For now, we'll try to revoke rules to handle dependency cases

            // Revoke all ingress rules
            if !state.ingress.is_empty() {
                if let Err(e) = self.revoke_ingress(&client, id, &state.ingress).await {
                    warn!("Failed to revoke ingress rules before delete: {}", e);
                }
            }

            // Revoke all egress rules (except default for VPC)
            if !state.egress.is_empty() {
                if let Err(e) = self.revoke_egress(&client, id, &state.egress).await {
                    warn!("Failed to revoke egress rules before delete: {}", e);
                }
            }
        }

        // Delete the security group
        client
            .delete_security_group()
            .group_id(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete security group: {}", e))
            })?;

        info!("Deleted security group: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        debug!("Importing security group: {}", id);

        let state =
            self.find_by_id(id, ctx)
                .await?
                .ok_or_else(|| ProvisioningError::ImportError {
                    resource_type: "aws_security_group".to_string(),
                    resource_id: id.to_string(),
                    message: "Security group not found".to_string(),
                })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize security group state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("owner_id", Value::String(state.owner_id.clone())))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Check for vpc_id reference
        if let Some(vpc_id) = config.get("vpc_id").and_then(|v| v.as_str()) {
            // Check if it's a reference like "{{ resources.aws_vpc.main.id }}"
            if vpc_id.contains("resources.aws_vpc.") {
                // Parse the reference
                if let Some(captures) = parse_resource_reference(vpc_id) {
                    deps.push(ResourceDependency::new(
                        captures.resource_type,
                        captures.resource_name,
                        captures.attribute,
                    ));
                }
            }
        }

        // Check for security group references in rules
        for rule_type in &["ingress", "egress"] {
            if let Some(rules) = config.get(*rule_type).and_then(|v| v.as_array()) {
                for rule in rules {
                    if let Some(sgs) = rule.get("security_groups").and_then(|v| v.as_array()) {
                        for sg_ref in sgs {
                            if let Some(sg_str) = sg_ref.as_str() {
                                if let Some(captures) = parse_resource_reference(sg_str) {
                                    deps.push(ResourceDependency::new(
                                        captures.resource_type,
                                        captures.resource_name,
                                        captures.attribute,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "vpc_id".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate name is present
        let name = config.get("name").and_then(|v| v.as_str());
        if name.is_none() || name.unwrap().is_empty() {
            return Err(ProvisioningError::ValidationError(
                "name is required".to_string(),
            ));
        }

        // Validate ingress rules if present
        if let Some(ingress) = config.get("ingress").and_then(|v| v.as_array()) {
            for (i, rule) in ingress.iter().enumerate() {
                self.validate_rule(rule, &format!("ingress[{}]", i))?;
            }
        }

        // Validate egress rules if present
        if let Some(egress) = config.get("egress").and_then(|v| v.as_array()) {
            for (i, rule) in egress.iter().enumerate() {
                self.validate_rule(rule, &format!("egress[{}]", i))?;
            }
        }

        Ok(())
    }
}

impl AwsSecurityGroupResource {
    /// Validate a single rule configuration
    fn validate_rule(&self, rule: &Value, path: &str) -> ProvisioningResult<()> {
        // Protocol is required
        let protocol = rule.get("protocol").and_then(|v| v.as_str());
        if protocol.is_none() {
            return Err(ProvisioningError::ValidationError(format!(
                "{}.protocol is required",
                path
            )));
        }

        let protocol = protocol.unwrap();
        if !["tcp", "udp", "icmp", "icmpv6", "-1", "all"]
            .contains(&protocol.to_lowercase().as_str())
        {
            return Err(ProvisioningError::ValidationError(format!(
                "{}.protocol must be one of: tcp, udp, icmp, icmpv6, -1, all",
                path
            )));
        }

        // For non-all protocols, ports are required
        if protocol != "-1" && protocol.to_lowercase() != "all" {
            if rule.get("from_port").is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{}.from_port is required for protocol {}",
                    path, protocol
                )));
            }
            if rule.get("to_port").is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{}.to_port is required for protocol {}",
                    path, protocol
                )));
            }
        }

        // At least one source/destination must be specified
        let has_cidr = rule
            .get("cidr_blocks")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_ipv6 = rule
            .get("ipv6_cidr_blocks")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_sg = rule
            .get("security_groups")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_self = rule
            .get("self_referencing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !has_cidr && !has_ipv6 && !has_sg && !has_self {
            return Err(ProvisioningError::ValidationError(format!(
                "{} must specify at least one of: cidr_blocks, ipv6_cidr_blocks, security_groups, or self_referencing",
                path
            )));
        }

        // Validate CIDR blocks format
        if let Some(cidrs) = rule.get("cidr_blocks").and_then(|v| v.as_array()) {
            for cidr in cidrs {
                if let Some(cidr_str) = cidr.as_str() {
                    if !is_valid_ipv4_cidr(cidr_str) {
                        return Err(ProvisioningError::ValidationError(format!(
                            "{}.cidr_blocks contains invalid IPv4 CIDR: {}",
                            path, cidr_str
                        )));
                    }
                }
            }
        }

        if let Some(cidrs) = rule.get("ipv6_cidr_blocks").and_then(|v| v.as_array()) {
            for cidr in cidrs {
                if let Some(cidr_str) = cidr.as_str() {
                    if !is_valid_ipv6_cidr(cidr_str) {
                        return Err(ProvisioningError::ValidationError(format!(
                            "{}.ipv6_cidr_blocks contains invalid IPv6 CIDR: {}",
                            path, cidr_str
                        )));
                    }
                }
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

/// Parse a resource reference string like "{{ resources.aws_vpc.main.id }}"
fn parse_resource_reference(reference: &str) -> Option<ResourceReference> {
    // Match pattern: {{ resources.<type>.<name>.<attr> }}
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

/// Basic IPv4 CIDR validation
fn is_valid_ipv4_cidr(cidr: &str) -> bool {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    // Validate IP address
    let ip_parts: Vec<&str> = parts[0].split('.').collect();
    if ip_parts.len() != 4 {
        return false;
    }

    for part in &ip_parts {
        match part.parse::<u8>() {
            Ok(_) => {}
            Err(_) => return false,
        }
    }

    // Validate prefix length
    match parts[1].parse::<u8>() {
        Ok(prefix) if prefix <= 32 => true,
        _ => false,
    }
}

/// Basic IPv6 CIDR validation
fn is_valid_ipv6_cidr(cidr: &str) -> bool {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    // Very basic IPv6 check - contains colons and valid prefix
    if !parts[0].contains(':') {
        return false;
    }

    match parts[1].parse::<u8>() {
        Ok(prefix) if prefix <= 128 => true,
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_type() {
        let resource = AwsSecurityGroupResource::new();
        assert_eq!(resource.resource_type(), "aws_security_group");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsSecurityGroupResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_security_group");
        assert!(!schema.required_args.is_empty());
        assert!(schema.required_args.iter().any(|f| f.name == "name"));
        assert!(schema.force_new.contains(&"name".to_string()));
        assert!(schema.force_new.contains(&"vpc_id".to_string()));
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsSecurityGroupResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"name".to_string()));
        assert!(force_new.contains(&"vpc_id".to_string()));
    }

    #[test]
    fn test_parse_config() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "description": "Test security group",
            "vpc_id": "vpc-12345",
            "ingress": [
                {
                    "protocol": "tcp",
                    "from_port": 80,
                    "to_port": 80,
                    "cidr_blocks": ["0.0.0.0/0"]
                }
            ],
            "egress": [],
            "tags": {
                "Environment": "test"
            }
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.name, "test-sg");
        assert_eq!(parsed.description, "Test security group");
        assert_eq!(parsed.vpc_id, Some("vpc-12345".to_string()));
        assert_eq!(parsed.ingress.len(), 1);
        assert_eq!(parsed.ingress[0].protocol, "tcp");
        assert_eq!(parsed.ingress[0].from_port, 80);
        assert_eq!(parsed.tags.get("Environment"), Some(&"test".to_string()));
    }

    #[test]
    fn test_parse_config_defaults() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "minimal-sg"
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.name, "minimal-sg");
        assert_eq!(parsed.description, "Managed by Rustible");
        assert!(parsed.vpc_id.is_none());
        assert!(parsed.ingress.is_empty());
        assert!(parsed.egress.is_empty());
        assert!(!parsed.revoke_rules_on_delete);
    }

    #[test]
    fn test_rule_key() {
        let rule = SecurityGroupRuleConfig {
            protocol: "tcp".to_string(),
            from_port: 443,
            to_port: 443,
            cidr_blocks: vec!["10.0.0.0/8".to_string()],
            ipv6_cidr_blocks: vec![],
            security_groups: vec![],
            self_referencing: false,
            description: Some("HTTPS".to_string()),
        };

        let key = rule.rule_key();
        assert!(key.contains("tcp"));
        assert!(key.contains("443"));
        assert!(key.contains("10.0.0.0/8"));
    }

    #[test]
    fn test_diff_rules() {
        let resource = AwsSecurityGroupResource::new();

        let current = vec![
            SecurityGroupRuleConfig {
                protocol: "tcp".to_string(),
                from_port: 80,
                to_port: 80,
                cidr_blocks: vec!["0.0.0.0/0".to_string()],
                ipv6_cidr_blocks: vec![],
                security_groups: vec![],
                self_referencing: false,
                description: None,
            },
            SecurityGroupRuleConfig {
                protocol: "tcp".to_string(),
                from_port: 22,
                to_port: 22,
                cidr_blocks: vec!["10.0.0.0/8".to_string()],
                ipv6_cidr_blocks: vec![],
                security_groups: vec![],
                self_referencing: false,
                description: None,
            },
        ];

        let desired = vec![
            SecurityGroupRuleConfig {
                protocol: "tcp".to_string(),
                from_port: 80,
                to_port: 80,
                cidr_blocks: vec!["0.0.0.0/0".to_string()],
                ipv6_cidr_blocks: vec![],
                security_groups: vec![],
                self_referencing: false,
                description: None,
            },
            SecurityGroupRuleConfig {
                protocol: "tcp".to_string(),
                from_port: 443,
                to_port: 443,
                cidr_blocks: vec!["0.0.0.0/0".to_string()],
                ipv6_cidr_blocks: vec![],
                security_groups: vec![],
                self_referencing: false,
                description: None,
            },
        ];

        let (to_add, to_remove) = resource.diff_rules(&current, &desired);

        assert_eq!(to_add.len(), 1);
        assert_eq!(to_add[0].from_port, 443);

        assert_eq!(to_remove.len(), 1);
        assert_eq!(to_remove[0].from_port, 22);
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "ingress": [
                {
                    "protocol": "tcp",
                    "from_port": 80,
                    "to_port": 80,
                    "cidr_blocks": ["0.0.0.0/0"]
                }
            ]
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_name() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "description": "No name"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_missing_protocol() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "ingress": [
                {
                    "from_port": 80,
                    "to_port": 80,
                    "cidr_blocks": ["0.0.0.0/0"]
                }
            ]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_no_source() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "ingress": [
                {
                    "protocol": "tcp",
                    "from_port": 80,
                    "to_port": 80
                }
            ]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_invalid_cidr() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "ingress": [
                {
                    "protocol": "tcp",
                    "from_port": 80,
                    "to_port": 80,
                    "cidr_blocks": ["invalid-cidr"]
                }
            ]
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_is_valid_ipv4_cidr() {
        assert!(is_valid_ipv4_cidr("0.0.0.0/0"));
        assert!(is_valid_ipv4_cidr("10.0.0.0/8"));
        assert!(is_valid_ipv4_cidr("192.168.1.0/24"));
        assert!(is_valid_ipv4_cidr("172.16.0.0/12"));

        assert!(!is_valid_ipv4_cidr("invalid"));
        assert!(!is_valid_ipv4_cidr("10.0.0.0"));
        assert!(!is_valid_ipv4_cidr("10.0.0.0/33"));
        assert!(!is_valid_ipv4_cidr("256.0.0.0/8"));
    }

    #[test]
    fn test_is_valid_ipv6_cidr() {
        assert!(is_valid_ipv6_cidr("::/0"));
        assert!(is_valid_ipv6_cidr("2001:db8::/32"));
        assert!(is_valid_ipv6_cidr("fe80::/10"));

        assert!(!is_valid_ipv6_cidr("invalid"));
        assert!(!is_valid_ipv6_cidr("2001:db8::/129"));
    }

    #[test]
    fn test_parse_resource_reference() {
        let ref1 = parse_resource_reference("{{ resources.aws_vpc.main.id }}");
        assert!(ref1.is_some());
        let ref1 = ref1.unwrap();
        assert_eq!(ref1.resource_type, "aws_vpc");
        assert_eq!(ref1.resource_name, "main");
        assert_eq!(ref1.attribute, "id");

        let ref2 = parse_resource_reference("{{ resources.aws_security_group.web.id }}");
        assert!(ref2.is_some());

        // Not a reference
        let ref3 = parse_resource_reference("vpc-12345");
        assert!(ref3.is_none());

        // Invalid reference
        let ref4 = parse_resource_reference("{{ invalid }}");
        assert!(ref4.is_none());
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsSecurityGroupResource::new();
        let config = serde_json::json!({
            "name": "test-sg",
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "ingress": [
                {
                    "protocol": "tcp",
                    "from_port": 443,
                    "to_port": 443,
                    "security_groups": ["{{ resources.aws_security_group.bastion.id }}"]
                }
            ]
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 2);

        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_vpc" && d.resource_name == "main"));
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "bastion"));
    }

    #[test]
    fn test_to_ip_permission() {
        let rule = SecurityGroupRuleConfig {
            protocol: "tcp".to_string(),
            from_port: 443,
            to_port: 443,
            cidr_blocks: vec!["0.0.0.0/0".to_string(), "10.0.0.0/8".to_string()],
            ipv6_cidr_blocks: vec!["::/0".to_string()],
            security_groups: vec![],
            self_referencing: false,
            description: Some("HTTPS".to_string()),
        };

        let perm = rule.to_ip_permission(None);
        assert_eq!(perm.ip_protocol(), Some("tcp"));
        assert_eq!(perm.from_port(), Some(443));
        assert_eq!(perm.to_port(), Some(443));
        assert_eq!(perm.ip_ranges().len(), 2);
        assert_eq!(perm.ipv6_ranges().len(), 1);
    }

    #[test]
    fn test_to_ip_permission_self_reference() {
        let rule = SecurityGroupRuleConfig {
            protocol: "tcp".to_string(),
            from_port: 0,
            to_port: 65535,
            cidr_blocks: vec![],
            ipv6_cidr_blocks: vec![],
            security_groups: vec![],
            self_referencing: true,
            description: Some("Self reference".to_string()),
        };

        let perm = rule.to_ip_permission(Some("sg-12345"));
        assert_eq!(perm.user_id_group_pairs().len(), 1);
        assert_eq!(perm.user_id_group_pairs()[0].group_id(), Some("sg-12345"));
    }

    #[test]
    fn test_security_group_state_serialization() {
        let state = SecurityGroupState {
            id: "sg-12345".to_string(),
            arn: "arn:aws:ec2:us-east-1:123456789:security-group/sg-12345".to_string(),
            name: "test-sg".to_string(),
            description: "Test".to_string(),
            vpc_id: Some("vpc-12345".to_string()),
            owner_id: "123456789".to_string(),
            ingress: vec![],
            egress: vec![],
            tags: HashMap::new(),
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "sg-12345");
        assert_eq!(json["name"], "test-sg");
        assert_eq!(json["vpc_id"], "vpc-12345");
    }
}
