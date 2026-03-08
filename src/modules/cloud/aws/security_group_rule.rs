//! AWS native module for standalone EC2 security group rules.
//!
//! This module manages individual ingress and egress rules directly from
//! playbooks using the AWS SDK for Rust.

use std::collections::{BTreeSet, HashMap};
use std::net::{Ipv4Addr, Ipv6Addr};

use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{IpPermission, IpRange, Ipv6Range, SecurityGroup, UserIdGroupPair};
use aws_sdk_ec2::Client;
use serde::Serialize;

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};

fn serialize_output_data<T: Serialize + ?Sized>(
    field: &str,
    value: &T,
) -> ModuleResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| {
        ModuleError::ExecutionFailed(format!(
            "Failed to serialize '{}' output data: {}",
            field, e
        ))
    })
}

fn join_scoped_module_thread(
    result: std::thread::Result<ModuleResult<ModuleOutput>>,
    module_name: &str,
) -> ModuleResult<ModuleOutput> {
    result.map_err(|_| {
        ModuleError::ExecutionFailed(format!(
            "{} worker thread panicked during execution",
            module_name
        ))
    })?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DesiredState {
    #[default]
    Present,
    Absent,
}

impl DesiredState {
    fn from_optional_str(value: Option<String>) -> ModuleResult<Self> {
        match value
            .unwrap_or_else(|| "present".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "present" => Ok(Self::Present),
            "absent" => Ok(Self::Absent),
            other => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum RuleDirection {
    Ingress,
    Egress,
}

impl RuleDirection {
    fn from_str(value: &str) -> ModuleResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ingress" | "in" => Ok(Self::Ingress),
            "egress" | "out" => Ok(Self::Egress),
            other => Err(ModuleError::InvalidParameter(format!(
                "Invalid type '{}'. Valid values: ingress, egress",
                other
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ingress => "ingress",
            Self::Egress => "egress",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SecurityGroupRuleInfo {
    rule_id: String,
    group_id: String,
    rule_type: String,
    protocol: String,
    from_port: i32,
    to_port: i32,
    cidr_blocks: Vec<String>,
    ipv6_cidr_blocks: Vec<String>,
    source_security_group_ids: Vec<String>,
    self_referencing: bool,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct SecurityGroupRuleConfig {
    group_id: String,
    direction: RuleDirection,
    state: DesiredState,
    protocol: String,
    from_port: i32,
    to_port: i32,
    cidr_blocks: Vec<String>,
    ipv6_cidr_blocks: Vec<String>,
    source_security_group_id: Option<String>,
    self_referencing: bool,
    description: Option<String>,
    region: Option<String>,
}

impl SecurityGroupRuleConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let group_id = params
            .get_string("group_id")?
            .or(params.get_string("security_group_id")?)
            .ok_or_else(|| ModuleError::MissingParameter("group_id".to_string()))?;
        let direction = RuleDirection::from_str(&params.get_string_required("type")?)?;
        let state = DesiredState::from_optional_str(params.get_string("state")?)?;
        let protocol = params
            .get_string("protocol")?
            .unwrap_or_else(|| "-1".to_string())
            .to_ascii_lowercase();
        let from_port = params.get_i64("from_port")?.unwrap_or(-1);
        let to_port = params.get_i64("to_port")?.unwrap_or(from_port);

        let from_port = i32::try_from(from_port).map_err(|_| {
            ModuleError::InvalidParameter("from_port must fit in a signed 32-bit integer".into())
        })?;
        let to_port = i32::try_from(to_port).map_err(|_| {
            ModuleError::InvalidParameter("to_port must fit in a signed 32-bit integer".into())
        })?;

        Ok(Self {
            group_id,
            direction,
            state,
            protocol,
            from_port,
            to_port,
            cidr_blocks: params.get_vec_string("cidr_blocks")?.unwrap_or_default(),
            ipv6_cidr_blocks: params
                .get_vec_string("ipv6_cidr_blocks")?
                .unwrap_or_default(),
            source_security_group_id: params
                .get_string("source_security_group_id")?
                .or(params.get_string("source_group_id")?),
            self_referencing: params.get_bool_or("self_referencing", false),
            description: params.get_string("description")?,
            region: params.get_string("region")?,
        })
    }

    fn validate(&self) -> ModuleResult<()> {
        if self.group_id.trim().is_empty() {
            return Err(ModuleError::InvalidParameter(
                "group_id cannot be empty".to_string(),
            ));
        }

        if !self.group_id.starts_with("sg-") {
            return Err(ModuleError::InvalidParameter(
                "group_id must look like an AWS security group ID (sg-...)".to_string(),
            ));
        }

        let valid_protocol = matches!(
            self.protocol.as_str(),
            "-1" | "tcp" | "udp" | "icmp" | "icmpv6"
        );
        if !valid_protocol {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid protocol '{}'. Valid values: -1, tcp, udp, icmp, icmpv6",
                self.protocol
            )));
        }

        if self
            .description
            .as_deref()
            .is_some_and(|value| value.len() > 255)
        {
            return Err(ModuleError::InvalidParameter(
                "description cannot exceed 255 characters".to_string(),
            ));
        }

        if self.cidr_blocks.is_empty()
            && self.ipv6_cidr_blocks.is_empty()
            && self.source_security_group_id.is_none()
            && !self.self_referencing
        {
            return Err(ModuleError::InvalidParameter(
                "At least one of cidr_blocks, ipv6_cidr_blocks, source_security_group_id, or self_referencing must be specified"
                    .to_string(),
            ));
        }

        for cidr in &self.cidr_blocks {
            validate_ipv4_cidr(cidr)?;
        }
        for cidr in &self.ipv6_cidr_blocks {
            validate_ipv6_cidr(cidr)?;
        }

        if let Some(source_security_group_id) = &self.source_security_group_id {
            if !source_security_group_id.starts_with("sg-") {
                return Err(ModuleError::InvalidParameter(
                    "source_security_group_id must look like an AWS security group ID (sg-...)"
                        .to_string(),
                ));
            }
        }

        validate_ports(self.protocol.as_str(), self.from_port, self.to_port)
    }

    fn desired_group_targets(&self) -> BTreeSet<String> {
        let mut targets = BTreeSet::new();
        if let Some(source_security_group_id) = &self.source_security_group_id {
            targets.insert(source_security_group_id.clone());
        }
        if self.self_referencing {
            targets.insert(self.group_id.clone());
        }
        targets
    }

    fn rule_id(&self) -> String {
        let mut sources = Vec::new();
        sources.extend(self.cidr_blocks.iter().cloned());
        sources.extend(self.ipv6_cidr_blocks.iter().cloned());
        sources.extend(self.desired_group_targets());

        let joined = sources.join(",");
        let raw = format!(
            "{}:{}:{}:{}:{}:{}",
            self.group_id,
            self.direction.as_str(),
            self.protocol,
            self.from_port,
            self.to_port,
            joined
        );

        raw.chars()
            .map(|ch| match ch {
                '/' | ':' | ',' | ' ' => '-',
                other => other,
            })
            .take(128)
            .collect()
    }

    fn info(&self) -> SecurityGroupRuleInfo {
        let desired_group_targets = self.desired_group_targets();
        SecurityGroupRuleInfo {
            rule_id: self.rule_id(),
            group_id: self.group_id.clone(),
            rule_type: self.direction.as_str().to_string(),
            protocol: self.protocol.clone(),
            from_port: self.from_port,
            to_port: self.to_port,
            cidr_blocks: self.cidr_blocks.clone(),
            ipv6_cidr_blocks: self.ipv6_cidr_blocks.clone(),
            source_security_group_ids: desired_group_targets.iter().cloned().collect(),
            self_referencing: self.self_referencing,
            description: self.description.clone(),
        }
    }
}

fn validate_ipv4_cidr(value: &str) -> ModuleResult<()> {
    let (ip, prefix) = value
        .split_once('/')
        .ok_or_else(|| ModuleError::InvalidParameter(format!("Invalid IPv4 CIDR '{}'", value)))?;
    ip.parse::<Ipv4Addr>()
        .map_err(|_| ModuleError::InvalidParameter(format!("Invalid IPv4 CIDR '{}'", value)))?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| ModuleError::InvalidParameter(format!("Invalid IPv4 CIDR '{}'", value)))?;
    if prefix > 32 {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid IPv4 CIDR '{}'",
            value
        )));
    }
    Ok(())
}

fn validate_ipv6_cidr(value: &str) -> ModuleResult<()> {
    let (ip, prefix) = value
        .split_once('/')
        .ok_or_else(|| ModuleError::InvalidParameter(format!("Invalid IPv6 CIDR '{}'", value)))?;
    ip.parse::<Ipv6Addr>()
        .map_err(|_| ModuleError::InvalidParameter(format!("Invalid IPv6 CIDR '{}'", value)))?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| ModuleError::InvalidParameter(format!("Invalid IPv6 CIDR '{}'", value)))?;
    if prefix > 128 {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid IPv6 CIDR '{}'",
            value
        )));
    }
    Ok(())
}

fn validate_ports(protocol: &str, from_port: i32, to_port: i32) -> ModuleResult<()> {
    match protocol {
        "-1" => {
            if from_port != -1 || to_port != -1 {
                return Err(ModuleError::InvalidParameter(
                    "from_port and to_port must both be -1 when protocol is -1".to_string(),
                ));
            }
        }
        "tcp" | "udp" => {
            if !(0..=65535).contains(&from_port) || !(0..=65535).contains(&to_port) {
                return Err(ModuleError::InvalidParameter(
                    "TCP/UDP ports must be between 0 and 65535".to_string(),
                ));
            }
            if from_port > to_port {
                return Err(ModuleError::InvalidParameter(
                    "from_port cannot be greater than to_port".to_string(),
                ));
            }
        }
        "icmp" | "icmpv6" => {
            if !(-1..=255).contains(&from_port) || !(-1..=255).contains(&to_port) {
                return Err(ModuleError::InvalidParameter(
                    "ICMP type/code values must be between -1 and 255".to_string(),
                ));
            }
            if from_port > to_port && to_port != -1 {
                return Err(ModuleError::InvalidParameter(
                    "from_port cannot be greater than to_port for ICMP rules".to_string(),
                ));
            }
        }
        _ => {}
    }

    Ok(())
}

fn build_ip_permission(
    protocol: &str,
    from_port: i32,
    to_port: i32,
    ipv4_cidrs: &[String],
    ipv6_cidrs: &[String],
    group_ids: &[String],
    description: Option<&str>,
) -> Option<IpPermission> {
    if ipv4_cidrs.is_empty() && ipv6_cidrs.is_empty() && group_ids.is_empty() {
        return None;
    }

    let mut builder = IpPermission::builder()
        .ip_protocol(protocol)
        .from_port(from_port)
        .to_port(to_port);

    for cidr in ipv4_cidrs {
        let mut range = IpRange::builder().cidr_ip(cidr);
        if let Some(description) = description {
            range = range.description(description);
        }
        builder = builder.ip_ranges(range.build());
    }

    for cidr in ipv6_cidrs {
        let mut range = Ipv6Range::builder().cidr_ipv6(cidr);
        if let Some(description) = description {
            range = range.description(description);
        }
        builder = builder.ipv6_ranges(range.build());
    }

    for group_id in group_ids {
        let mut pair = UserIdGroupPair::builder().group_id(group_id);
        if let Some(description) = description {
            pair = pair.description(description);
        }
        builder = builder.user_id_group_pairs(pair.build());
    }

    Some(builder.build())
}

#[derive(Debug, Default)]
struct MatchingTargetState {
    present_ipv4: BTreeSet<String>,
    mismatched_ipv4: BTreeSet<String>,
    present_ipv6: BTreeSet<String>,
    mismatched_ipv6: BTreeSet<String>,
    present_group_ids: BTreeSet<String>,
    mismatched_group_ids: BTreeSet<String>,
}

impl MatchingTargetState {
    fn any_present(&self) -> bool {
        !self.present_ipv4.is_empty()
            || !self.present_ipv6.is_empty()
            || !self.present_group_ids.is_empty()
            || !self.mismatched_ipv4.is_empty()
            || !self.mismatched_ipv6.is_empty()
            || !self.mismatched_group_ids.is_empty()
    }

    fn fully_matches(&self, config: &SecurityGroupRuleConfig) -> bool {
        self.mismatched_ipv4.is_empty()
            && self.mismatched_ipv6.is_empty()
            && self.mismatched_group_ids.is_empty()
            && self.present_ipv4.len() == config.cidr_blocks.len()
            && self.present_ipv6.len() == config.ipv6_cidr_blocks.len()
            && self.present_group_ids.len() == config.desired_group_targets().len()
    }
}

fn description_matches(current: Option<&str>, desired: Option<&str>) -> bool {
    current == desired
}

fn inspect_matching_targets(
    security_group: &SecurityGroup,
    config: &SecurityGroupRuleConfig,
) -> MatchingTargetState {
    let permissions = match config.direction {
        RuleDirection::Ingress => security_group.ip_permissions(),
        RuleDirection::Egress => security_group.ip_permissions_egress(),
    };

    let mut state = MatchingTargetState::default();
    let desired_group_targets = config.desired_group_targets();

    for permission in permissions {
        let protocol = permission.ip_protocol().unwrap_or("-1");
        let from_port = permission.from_port().unwrap_or(-1);
        let to_port = permission.to_port().unwrap_or(-1);

        if protocol != config.protocol || from_port != config.from_port || to_port != config.to_port
        {
            continue;
        }

        for range in permission.ip_ranges() {
            if let Some(cidr) = range.cidr_ip() {
                if config.cidr_blocks.iter().any(|desired| desired == cidr) {
                    if description_matches(range.description(), config.description.as_deref()) {
                        state.present_ipv4.insert(cidr.to_string());
                    } else {
                        state.mismatched_ipv4.insert(cidr.to_string());
                    }
                }
            }
        }

        for range in permission.ipv6_ranges() {
            if let Some(cidr) = range.cidr_ipv6() {
                if config
                    .ipv6_cidr_blocks
                    .iter()
                    .any(|desired| desired == cidr)
                {
                    if description_matches(range.description(), config.description.as_deref()) {
                        state.present_ipv6.insert(cidr.to_string());
                    } else {
                        state.mismatched_ipv6.insert(cidr.to_string());
                    }
                }
            }
        }

        for pair in permission.user_id_group_pairs() {
            if let Some(group_id) = pair.group_id() {
                if desired_group_targets.contains(group_id) {
                    if description_matches(pair.description(), config.description.as_deref()) {
                        state.present_group_ids.insert(group_id.to_string());
                    } else {
                        state.mismatched_group_ids.insert(group_id.to_string());
                    }
                }
            }
        }
    }

    state
}

fn build_missing_permission(
    config: &SecurityGroupRuleConfig,
    state: &MatchingTargetState,
) -> Option<IpPermission> {
    let missing_ipv4: Vec<String> = config
        .cidr_blocks
        .iter()
        .filter(|cidr| {
            !state.present_ipv4.contains(*cidr) && !state.mismatched_ipv4.contains(*cidr)
        })
        .cloned()
        .collect();
    let missing_ipv6: Vec<String> = config
        .ipv6_cidr_blocks
        .iter()
        .filter(|cidr| {
            !state.present_ipv6.contains(*cidr) && !state.mismatched_ipv6.contains(*cidr)
        })
        .cloned()
        .collect();
    let missing_group_ids: Vec<String> = config
        .desired_group_targets()
        .into_iter()
        .filter(|group_id| {
            !state.present_group_ids.contains(group_id)
                && !state.mismatched_group_ids.contains(group_id)
        })
        .collect();

    build_ip_permission(
        &config.protocol,
        config.from_port,
        config.to_port,
        &missing_ipv4,
        &missing_ipv6,
        &missing_group_ids,
        config.description.as_deref(),
    )
}

fn build_revoke_permission(
    config: &SecurityGroupRuleConfig,
    state: &MatchingTargetState,
) -> Option<IpPermission> {
    let mut revoke_ipv4 = state.present_ipv4.clone();
    revoke_ipv4.extend(state.mismatched_ipv4.iter().cloned());
    let mut revoke_ipv6 = state.present_ipv6.clone();
    revoke_ipv6.extend(state.mismatched_ipv6.iter().cloned());
    let mut revoke_group_ids = state.present_group_ids.clone();
    revoke_group_ids.extend(state.mismatched_group_ids.iter().cloned());

    build_ip_permission(
        &config.protocol,
        config.from_port,
        config.to_port,
        &revoke_ipv4.into_iter().collect::<Vec<_>>(),
        &revoke_ipv6.into_iter().collect::<Vec<_>>(),
        &revoke_group_ids.into_iter().collect::<Vec<_>>(),
        None,
    )
}

async fn create_ec2_client(region: Option<&str>) -> ModuleResult<Client> {
    let config = if let Some(region_str) = region {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_ec2::config::Region::new(region_str.to_string()))
            .load()
            .await
    } else {
        aws_config::defaults(BehaviorVersion::latest()).load().await
    };

    Ok(Client::new(&config))
}

async fn load_security_group(
    client: &Client,
    group_id: &str,
) -> ModuleResult<Option<SecurityGroup>> {
    match client
        .describe_security_groups()
        .group_ids(group_id)
        .send()
        .await
    {
        Ok(response) => Ok(response.security_groups().first().cloned()),
        Err(error) => {
            let message = error.to_string();
            if message.contains("InvalidGroup.NotFound") {
                return Ok(None);
            }

            Err(ModuleError::ExecutionFailed(format!(
                "Failed to describe security group '{}': {}",
                group_id, error
            )))
        }
    }
}

async fn authorize_rule(
    client: &Client,
    direction: RuleDirection,
    group_id: &str,
    permission: IpPermission,
) -> ModuleResult<()> {
    match direction {
        RuleDirection::Ingress => {
            client
                .authorize_security_group_ingress()
                .group_id(group_id)
                .ip_permissions(permission)
                .send()
                .await
                .map_err(|error| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to authorize ingress rule on '{}': {}",
                        group_id, error
                    ))
                })?;
        }
        RuleDirection::Egress => {
            client
                .authorize_security_group_egress()
                .group_id(group_id)
                .ip_permissions(permission)
                .send()
                .await
                .map_err(|error| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to authorize egress rule on '{}': {}",
                        group_id, error
                    ))
                })?;
        }
    }

    Ok(())
}

async fn revoke_rule(
    client: &Client,
    direction: RuleDirection,
    group_id: &str,
    permission: IpPermission,
) -> ModuleResult<()> {
    match direction {
        RuleDirection::Ingress => {
            client
                .revoke_security_group_ingress()
                .group_id(group_id)
                .ip_permissions(permission)
                .send()
                .await
                .map_err(|error| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to revoke ingress rule on '{}': {}",
                        group_id, error
                    ))
                })?;
        }
        RuleDirection::Egress => {
            client
                .revoke_security_group_egress()
                .group_id(group_id)
                .ip_permissions(permission)
                .send()
                .await
                .map_err(|error| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to revoke egress rule on '{}': {}",
                        group_id, error
                    ))
                })?;
        }
    }

    Ok(())
}

pub struct AwsSecurityGroupRuleModule;

impl AwsSecurityGroupRuleModule {
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = SecurityGroupRuleConfig::from_params(params)?;
        config.validate()?;

        let info = config.info();
        let client = create_ec2_client(config.region.as_deref()).await?;
        let security_group = load_security_group(&client, &config.group_id).await?;

        match config.state {
            DesiredState::Present => {
                let security_group = security_group.ok_or_else(|| {
                    ModuleError::ExecutionFailed(format!(
                        "Security group '{}' was not found",
                        config.group_id
                    ))
                })?;

                let matching_state = inspect_matching_targets(&security_group, &config);
                if matching_state.fully_matches(&config) {
                    return Ok(ModuleOutput::ok(format!(
                        "Security group rule '{}' is up to date",
                        info.rule_id
                    ))
                    .with_data("rule", serialize_output_data("rule", &info)?));
                }

                let needs_recreate = !matching_state.mismatched_ipv4.is_empty()
                    || !matching_state.mismatched_ipv6.is_empty()
                    || !matching_state.mismatched_group_ids.is_empty();
                let action = if matching_state.any_present() {
                    "update"
                } else {
                    "create"
                };

                if context.check_mode {
                    let msg = if action == "update" {
                        format!("Would update security group rule '{}'", info.rule_id)
                    } else {
                        format!("Would create security group rule '{}'", info.rule_id)
                    };
                    return Ok(ModuleOutput::changed(msg)
                        .with_diff(Diff::new("current", "desired"))
                        .with_data("action", serde_json::json!(action))
                        .with_data("rule", serialize_output_data("rule", &info)?));
                }

                if needs_recreate {
                    if let Some(permission) = build_revoke_permission(&config, &matching_state) {
                        revoke_rule(&client, config.direction, &config.group_id, permission)
                            .await?;
                    }
                    if let Some(permission) = build_ip_permission(
                        &config.protocol,
                        config.from_port,
                        config.to_port,
                        &config.cidr_blocks,
                        &config.ipv6_cidr_blocks,
                        &config
                            .desired_group_targets()
                            .into_iter()
                            .collect::<Vec<_>>(),
                        config.description.as_deref(),
                    ) {
                        authorize_rule(&client, config.direction, &config.group_id, permission)
                            .await?;
                    }
                } else if let Some(permission) = build_missing_permission(&config, &matching_state)
                {
                    authorize_rule(&client, config.direction, &config.group_id, permission).await?;
                }

                Ok(ModuleOutput::changed(format!(
                    "{}d security group rule '{}'",
                    action[..1].to_uppercase() + &action[1..],
                    info.rule_id
                ))
                .with_data("action", serde_json::json!(action))
                .with_data("rule", serialize_output_data("rule", &info)?))
            }
            DesiredState::Absent => {
                let Some(security_group) = security_group else {
                    return Ok(ModuleOutput::ok(format!(
                        "Security group '{}' is missing so rule '{}' is absent",
                        config.group_id, info.rule_id
                    )));
                };

                let matching_state = inspect_matching_targets(&security_group, &config);
                if !matching_state.any_present() {
                    return Ok(ModuleOutput::ok(format!(
                        "Security group rule '{}' is already absent",
                        info.rule_id
                    ))
                    .with_data("rule", serialize_output_data("rule", &info)?));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete security group rule '{}'",
                        info.rule_id
                    ))
                    .with_diff(Diff::new("present", "absent"))
                    .with_data("action", serde_json::json!("delete"))
                    .with_data("rule", serialize_output_data("rule", &info)?));
                }

                if let Some(permission) = build_revoke_permission(&config, &matching_state) {
                    revoke_rule(&client, config.direction, &config.group_id, permission).await?;
                }

                Ok(
                    ModuleOutput::changed(format!(
                        "Deleted security group rule '{}'",
                        info.rule_id
                    ))
                    .with_data("action", serde_json::json!("delete"))
                    .with_data("rule", serialize_output_data("rule", &info)?),
                )
            }
        }
    }
}

impl Module for AwsSecurityGroupRuleModule {
    fn name(&self) -> &'static str {
        "aws_security_group_rule"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete standalone AWS EC2 security group rules"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["type"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("state", serde_json::json!("present"));
        params.insert("protocol", serde_json::json!("-1"));
        params.insert("from_port", serde_json::json!(-1));
        params.insert("to_port", serde_json::json!(-1));
        params.insert("cidr_blocks", serde_json::json!([]));
        params.insert("ipv6_cidr_blocks", serde_json::json!([]));
        params.insert("self_referencing", serde_json::json!(false));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        SecurityGroupRuleConfig::from_params(params)?.validate()
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|scope| {
            join_scoped_module_thread(
                scope
                    .spawn(|| handle.block_on(module.execute_async(&params, &context)))
                    .join(),
                module.name(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_params() -> ModuleParams {
        let mut params = ModuleParams::new();
        params.insert(
            "group_id".to_string(),
            serde_json::json!("sg-0123456789abcdef0"),
        );
        params.insert("type".to_string(), serde_json::json!("ingress"));
        params.insert("protocol".to_string(), serde_json::json!("tcp"));
        params.insert("from_port".to_string(), serde_json::json!(443));
        params.insert("to_port".to_string(), serde_json::json!(443));
        params.insert(
            "cidr_blocks".to_string(),
            serde_json::json!(["10.0.0.0/24"]),
        );
        params
    }

    #[test]
    fn test_rule_module_accepts_group_id_alias() {
        let mut params = ModuleParams::new();
        params.insert(
            "security_group_id".to_string(),
            serde_json::json!("sg-0123456789abcdef0"),
        );
        params.insert("type".to_string(), serde_json::json!("egress"));
        params.insert(
            "ipv6_cidr_blocks".to_string(),
            serde_json::json!(["2001:db8::/64"]),
        );

        let config = SecurityGroupRuleConfig::from_params(&params).unwrap();
        assert_eq!(config.group_id, "sg-0123456789abcdef0");
        assert_eq!(config.direction, RuleDirection::Egress);
    }

    #[test]
    fn test_rule_validation_rejects_missing_sources() {
        let mut params = ModuleParams::new();
        params.insert(
            "group_id".to_string(),
            serde_json::json!("sg-0123456789abcdef0"),
        );
        params.insert("type".to_string(), serde_json::json!("ingress"));

        let error = SecurityGroupRuleConfig::from_params(&params)
            .unwrap()
            .validate()
            .unwrap_err();
        assert!(error.to_string().contains("At least one of"));
    }

    #[test]
    fn test_rule_validation_rejects_invalid_ipv4_cidr() {
        let mut params = base_params();
        params.insert(
            "cidr_blocks".to_string(),
            serde_json::json!(["10.0.0.0/99"]),
        );

        assert!(SecurityGroupRuleConfig::from_params(&params)
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn test_rule_validation_rejects_invalid_port_range() {
        let mut params = base_params();
        params.insert("from_port".to_string(), serde_json::json!(9000));
        params.insert("to_port".to_string(), serde_json::json!(100));

        assert!(SecurityGroupRuleConfig::from_params(&params)
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn test_rule_validation_allows_self_referencing_rule() {
        let mut params = ModuleParams::new();
        params.insert(
            "group_id".to_string(),
            serde_json::json!("sg-0123456789abcdef0"),
        );
        params.insert("type".to_string(), serde_json::json!("ingress"));
        params.insert("protocol".to_string(), serde_json::json!("-1"));
        params.insert("from_port".to_string(), serde_json::json!(-1));
        params.insert("to_port".to_string(), serde_json::json!(-1));
        params.insert("self_referencing".to_string(), serde_json::json!(true));

        assert!(SecurityGroupRuleConfig::from_params(&params)
            .unwrap()
            .validate()
            .is_ok());
    }

    #[test]
    fn test_rule_info_includes_self_group_target() {
        let mut params = base_params();
        params.insert(
            "source_security_group_id".to_string(),
            serde_json::json!("sg-11111111111111111"),
        );
        params.insert("self_referencing".to_string(), serde_json::json!(true));

        let config = SecurityGroupRuleConfig::from_params(&params).unwrap();
        let info = config.info();
        assert_eq!(info.source_security_group_ids.len(), 2);
    }

    #[test]
    fn test_build_revoke_permission_omits_description() {
        let config = SecurityGroupRuleConfig::from_params(&base_params()).unwrap();
        let mut state = MatchingTargetState::default();
        state.present_ipv4.insert("10.0.0.0/24".to_string());
        let permission = build_revoke_permission(&config, &state).unwrap();
        assert_eq!(permission.ip_ranges().len(), 1);
        assert!(permission.ip_ranges()[0].description().is_none());
    }
}
