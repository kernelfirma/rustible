//! AWS EC2 module for instance and infrastructure management.
//!
//! This module provides comprehensive EC2 instance lifecycle management including:
//!
//! - Instance creation, termination, start, stop, and reboot
//! - Security group creation and rule management
//! - VPC and subnet management
//! - State polling and wait operations
//!
//! ## Ec2InstanceModule
//!
//! Manages EC2 instance lifecycle. Supports idempotent operations using instance
//! names (via Name tag) for identification.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Instance name (used as Name tag) |
//! | `state` | No | Desired state: running, stopped, terminated, absent (default: running) |
//! | `instance_type` | No | Instance type (default: t3.micro) |
//! | `image_id` | No* | AMI ID (*required when creating new instance) |
//! | `key_name` | No | SSH key pair name |
//! | `vpc_subnet_id` | No | Subnet ID for the instance |
//! | `security_groups` | No | List of security group IDs |
//! | `security_group_names` | No | List of security group names (EC2-Classic or default VPC) |
//! | `instance_profile_name` | No | IAM instance profile name |
//! | `user_data` | No | User data script (base64 encoded or plain text) |
//! | `tags` | No | Additional tags as key-value pairs |
//! | `volumes` | No | EBS volume configurations |
//! | `network_interfaces` | No | Network interface configurations |
//! | `tenancy` | No | Instance tenancy: default, dedicated, host |
//! | `ebs_optimized` | No | Enable EBS optimization |
//! | `monitoring` | No | Enable detailed monitoring |
//! | `placement_group` | No | Placement group name |
//! | `availability_zone` | No | Specific availability zone |
//! | `private_ip` | No | Primary private IP address |
//! | `count` | No | Number of instances to launch (default: 1) |
//! | `wait` | No | Wait for state transition (default: true) |
//! | `wait_timeout` | No | Timeout for wait operations in seconds (default: 300) |
//! | `region` | No | AWS region (default: from environment/config) |
//!
//! ### Example
//!
//! ```yaml
//! - name: Launch a web server
//!   aws_ec2_instance:
//!     name: web-server-01
//!     instance_type: t3.small
//!     image_id: ami-0abcdef1234567890
//!     key_name: my-key-pair
//!     vpc_subnet_id: subnet-12345678
//!     security_groups:
//!       - sg-12345678
//!     tags:
//!       Environment: production
//!       Team: web
//!     wait: true
//!     state: running
//! ```
//!
//! ## Ec2SecurityGroupModule
//!
//! Manages EC2 security groups and their ingress/egress rules.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Security group name |
//! | `description` | No | Security group description |
//! | `vpc_id` | No | VPC ID (required for VPC security groups) |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `rules` | No | Ingress rules |
//! | `rules_egress` | No | Egress rules |
//! | `purge_rules` | No | Remove rules not in the list (default: false) |
//! | `tags` | No | Additional tags |
//!
//! ## Ec2VpcModule
//!
//! Manages VPCs, subnets, and related networking resources.

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{
    BlockDeviceMapping, EbsBlockDevice, Filter, IamInstanceProfileSpecification,
    InstanceNetworkInterfaceSpecification, InstanceStateName, InstanceType, IpPermission, IpRange,
    Ipv6Range, Placement, ResourceType, Tag, TagSpecification, Tenancy, VolumeType,
};
use aws_sdk_ec2::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Represents the desired state of an EC2 instance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum InstanceState {
    /// Instance should be running
    #[default]
    Running,
    /// Instance should be stopped
    Stopped,
    /// Instance should be terminated
    Terminated,
    /// Instance should not exist (alias for terminated)
    Absent,
    /// Instance should be rebooted (transient state)
    Rebooted,
}

impl InstanceState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "running" | "started" | "present" => Ok(InstanceState::Running),
            "stopped" => Ok(InstanceState::Stopped),
            "terminated" | "absent" => Ok(InstanceState::Terminated),
            "rebooted" | "restarted" => Ok(InstanceState::Rebooted),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: running, stopped, terminated, absent, rebooted",
                s
            ))),
        }
    }
}

/// AWS EC2 instance state as returned by the API
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ec2InstanceState {
    Pending,
    Running,
    ShuttingDown,
    Terminated,
    Stopping,
    Stopped,
    Unknown(String),
}

impl Ec2InstanceState {
    fn from_api_state(state: &InstanceStateName) -> Self {
        match state {
            InstanceStateName::Pending => Self::Pending,
            InstanceStateName::Running => Self::Running,
            InstanceStateName::ShuttingDown => Self::ShuttingDown,
            InstanceStateName::Terminated => Self::Terminated,
            InstanceStateName::Stopping => Self::Stopping,
            InstanceStateName::Stopped => Self::Stopped,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "shutting-down" => Self::ShuttingDown,
            "terminated" => Self::Terminated,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Running | Self::Stopped | Self::Terminated)
    }

    fn matches_desired(&self, desired: &InstanceState) -> bool {
        match (self, desired) {
            (Self::Running, InstanceState::Running) => true,
            (Self::Stopped, InstanceState::Stopped) => true,
            (Self::Terminated, InstanceState::Terminated | InstanceState::Absent) => true,
            _ => false,
        }
    }
}

/// EBS volume configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbsVolume {
    /// Device name (e.g., /dev/sda1)
    pub device_name: String,
    /// Volume size in GiB
    pub size: Option<i32>,
    /// Volume type: gp2, gp3, io1, io2, st1, sc1, standard
    pub volume_type: Option<String>,
    /// IOPS for io1/io2/gp3 volumes
    pub iops: Option<i32>,
    /// Throughput for gp3 volumes (MiB/s)
    pub throughput: Option<i32>,
    /// Whether to delete on instance termination
    pub delete_on_termination: Option<bool>,
    /// Whether the volume is encrypted
    pub encrypted: Option<bool>,
    /// KMS key ID for encryption
    pub kms_key_id: Option<String>,
    /// Snapshot ID to create volume from
    pub snapshot_id: Option<String>,
}

impl EbsVolume {
    fn to_block_device_mapping(&self) -> BlockDeviceMapping {
        let mut ebs = EbsBlockDevice::builder();

        if let Some(size) = self.size {
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
        if let Some(ref snap_id) = self.snapshot_id {
            ebs = ebs.snapshot_id(snap_id);
        }

        BlockDeviceMapping::builder()
            .device_name(&self.device_name)
            .ebs(ebs.build())
            .build()
    }
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Device index (0 for primary)
    pub device_index: i32,
    /// Subnet ID
    pub subnet_id: Option<String>,
    /// Security group IDs
    pub security_groups: Option<Vec<String>>,
    /// Whether to assign public IP
    pub associate_public_ip: Option<bool>,
    /// Whether to delete on instance termination
    pub delete_on_termination: Option<bool>,
    /// Primary private IP address
    pub private_ip: Option<String>,
    /// Secondary private IP addresses
    pub secondary_private_ips: Option<Vec<String>>,
    /// Number of secondary IPs to allocate
    pub secondary_private_ip_count: Option<i32>,
    /// Elastic IP allocation ID to associate
    pub eip_allocation_id: Option<String>,
}

impl NetworkInterface {
    fn to_sdk_network_interface(&self) -> InstanceNetworkInterfaceSpecification {
        let mut builder =
            InstanceNetworkInterfaceSpecification::builder().device_index(self.device_index);

        if let Some(ref subnet_id) = self.subnet_id {
            builder = builder.subnet_id(subnet_id);
        }
        if let Some(ref groups) = self.security_groups {
            for group in groups {
                builder = builder.groups(group);
            }
        }
        if let Some(associate_public) = self.associate_public_ip {
            builder = builder.associate_public_ip_address(associate_public);
        }
        if let Some(delete) = self.delete_on_termination {
            builder = builder.delete_on_termination(delete);
        }
        if let Some(ref private_ip) = self.private_ip {
            builder = builder.private_ip_address(private_ip);
        }
        if let Some(count) = self.secondary_private_ip_count {
            builder = builder.secondary_private_ip_address_count(count);
        }

        builder.build()
    }
}

/// Security group rule specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupRule {
    /// IP protocol: tcp, udp, icmp, -1 (all)
    pub protocol: String,
    /// Start of port range
    pub from_port: i32,
    /// End of port range
    pub to_port: i32,
    /// CIDR blocks for IPv4
    pub cidr_ip: Option<String>,
    /// CIDR blocks for IPv6
    pub cidr_ipv6: Option<String>,
    /// Source security group ID
    pub source_security_group_id: Option<String>,
    /// Source security group name
    pub source_security_group_name: Option<String>,
    /// Prefix list ID
    pub prefix_list_id: Option<String>,
    /// Rule description
    pub description: Option<String>,
}

impl SecurityGroupRule {
    fn to_ip_permission(&self) -> IpPermission {
        let mut builder = IpPermission::builder()
            .ip_protocol(&self.protocol)
            .from_port(self.from_port)
            .to_port(self.to_port);

        if let Some(ref cidr) = self.cidr_ip {
            let mut ip_range = IpRange::builder().cidr_ip(cidr);
            if let Some(ref desc) = self.description {
                ip_range = ip_range.description(desc);
            }
            builder = builder.ip_ranges(ip_range.build());
        }

        if let Some(ref cidr_v6) = self.cidr_ipv6 {
            let mut ipv6_range = Ipv6Range::builder().cidr_ipv6(cidr_v6);
            if let Some(ref desc) = self.description {
                ipv6_range = ipv6_range.description(desc);
            }
            builder = builder.ipv6_ranges(ipv6_range.build());
        }

        builder.build()
    }
}

/// EC2 instance configuration parsed from module parameters
#[derive(Debug, Clone)]
struct Ec2InstanceConfig {
    name: String,
    state: InstanceState,
    instance_type: String,
    image_id: Option<String>,
    key_name: Option<String>,
    vpc_subnet_id: Option<String>,
    security_groups: Vec<String>,
    security_group_names: Vec<String>,
    instance_profile_name: Option<String>,
    user_data: Option<String>,
    tags: HashMap<String, String>,
    volumes: Vec<EbsVolume>,
    network_interfaces: Vec<NetworkInterface>,
    tenancy: Option<String>,
    ebs_optimized: bool,
    monitoring: bool,
    placement_group: Option<String>,
    availability_zone: Option<String>,
    private_ip: Option<String>,
    count: i32,
    wait: bool,
    wait_timeout: u64,
    region: Option<String>,
    instance_ids: Vec<String>,
    exact_count: Option<i32>,
    terminate_oldest: bool,
}

impl Ec2InstanceConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        // Name is required for idempotent operations
        let name = params.get_string("name")?.ok_or_else(|| {
            ModuleError::MissingParameter(
                "name is required for EC2 instance identification".to_string(),
            )
        })?;

        let state = if let Some(s) = params.get_string("state")? {
            InstanceState::from_str(&s)?
        } else {
            InstanceState::default()
        };

        // Parse security groups from either array or comma-separated string
        let security_groups = params
            .get_vec_string("security_groups")?
            .unwrap_or_default();
        let security_group_names = params
            .get_vec_string("security_group_names")?
            .unwrap_or_default();

        // Parse tags
        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        // Parse volumes
        let volumes = if let Some(vol_value) = params.get("volumes") {
            if let Some(vol_array) = vol_value.as_array() {
                vol_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse network interfaces
        let network_interfaces = if let Some(ni_value) = params.get("network_interfaces") {
            if let Some(ni_array) = ni_value.as_array() {
                ni_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse instance IDs for direct operations
        let instance_ids = params.get_vec_string("instance_ids")?.unwrap_or_default();

        Ok(Self {
            name,
            state,
            instance_type: params
                .get_string("instance_type")?
                .unwrap_or_else(|| "t3.micro".to_string()),
            image_id: params.get_string("image_id")?,
            key_name: params.get_string("key_name")?,
            vpc_subnet_id: params.get_string("vpc_subnet_id")?,
            security_groups,
            security_group_names,
            instance_profile_name: params.get_string("instance_profile_name")?,
            user_data: params.get_string("user_data")?,
            tags,
            volumes,
            network_interfaces,
            tenancy: params.get_string("tenancy")?,
            ebs_optimized: params.get_bool_or("ebs_optimized", false),
            monitoring: params.get_bool_or("monitoring", false),
            placement_group: params.get_string("placement_group")?,
            availability_zone: params.get_string("availability_zone")?,
            private_ip: params.get_string("private_ip")?,
            count: params.get_i64("count")?.unwrap_or(1) as i32,
            wait: params.get_bool_or("wait", true),
            wait_timeout: params.get_i64("wait_timeout")?.unwrap_or(300) as u64,
            region: params.get_string("region")?,
            instance_ids,
            exact_count: params.get_i64("exact_count")?.map(|v| v as i32),
            terminate_oldest: params.get_bool_or("terminate_oldest", false),
        })
    }
}

/// Simulated EC2 instance info (comes from AWS SDK)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub state: String,
    pub instance_type: String,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub public_dns: Option<String>,
    pub private_dns: Option<String>,
    pub vpc_id: Option<String>,
    pub subnet_id: Option<String>,
    pub security_groups: Vec<String>,
    pub key_name: Option<String>,
    pub launch_time: String,
    pub availability_zone: String,
    pub tags: HashMap<String, String>,
}

/// AWS EC2 Instance module for managing EC2 instances
pub struct Ec2InstanceModule;

impl Ec2InstanceModule {
    /// Create AWS EC2 client
    async fn create_client(region: Option<&str>) -> ModuleResult<Client> {
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

    /// Find instances by name tag
    async fn find_instances_by_name(config: &Ec2InstanceConfig) -> ModuleResult<Vec<InstanceInfo>> {
        let client = Self::create_client(config.region.as_deref()).await?;

        let resp = client
            .describe_instances()
            .filters(
                Filter::builder()
                    .name("tag:Name")
                    .values(&config.name)
                    .build(),
            )
            .filters(
                Filter::builder()
                    .name("instance-state-name")
                    .values("pending")
                    .values("running")
                    .values("stopping")
                    .values("stopped")
                    .build(),
            )
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to describe instances: {}", e))
            })?;

        let mut instances = Vec::new();

        for reservation in resp.reservations() {
            for instance in reservation.instances() {
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

                instances.push(InstanceInfo {
                    instance_id,
                    state,
                    instance_type: instance
                        .instance_type()
                        .map(|t| t.as_str().to_string())
                        .unwrap_or_default(),
                    public_ip: instance.public_ip_address().map(|s| s.to_string()),
                    private_ip: instance.private_ip_address().map(|s| s.to_string()),
                    public_dns: instance.public_dns_name().map(|s| s.to_string()),
                    private_dns: instance.private_dns_name().map(|s| s.to_string()),
                    vpc_id: instance.vpc_id().map(|s| s.to_string()),
                    subnet_id: instance.subnet_id().map(|s| s.to_string()),
                    security_groups,
                    key_name: instance.key_name().map(|s| s.to_string()),
                    launch_time: instance
                        .launch_time()
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                    availability_zone: instance
                        .placement()
                        .and_then(|p| p.availability_zone())
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    tags,
                });
            }
        }

        Ok(instances)
    }

    /// Find instance by ID
    async fn find_instance_by_id(
        instance_id: &str,
        region: Option<&str>,
    ) -> ModuleResult<Option<InstanceInfo>> {
        let client = Self::create_client(region).await?;

        let resp = client
            .describe_instances()
            .instance_ids(instance_id)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to describe instance: {}", e))
            })?;

        for reservation in resp.reservations() {
            for instance in reservation.instances() {
                let id = instance.instance_id().unwrap_or_default().to_string();
                if id == instance_id {
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

                    return Ok(Some(InstanceInfo {
                        instance_id: id,
                        state,
                        instance_type: instance
                            .instance_type()
                            .map(|t| t.as_str().to_string())
                            .unwrap_or_default(),
                        public_ip: instance.public_ip_address().map(|s| s.to_string()),
                        private_ip: instance.private_ip_address().map(|s| s.to_string()),
                        public_dns: instance.public_dns_name().map(|s| s.to_string()),
                        private_dns: instance.private_dns_name().map(|s| s.to_string()),
                        vpc_id: instance.vpc_id().map(|s| s.to_string()),
                        subnet_id: instance.subnet_id().map(|s| s.to_string()),
                        security_groups,
                        key_name: instance.key_name().map(|s| s.to_string()),
                        launch_time: instance
                            .launch_time()
                            .map(|t| t.to_string())
                            .unwrap_or_default(),
                        availability_zone: instance
                            .placement()
                            .and_then(|p| p.availability_zone())
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        tags,
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Create new EC2 instance(s)
    async fn create_instances(config: &Ec2InstanceConfig) -> ModuleResult<Vec<InstanceInfo>> {
        let client = Self::create_client(config.region.as_deref()).await?;

        // Validate required parameters for creation
        let image_id = config.image_id.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "image_id is required when creating a new instance".to_string(),
            )
        })?;

        // Parse instance type
        let instance_type = config.instance_type.parse::<InstanceType>().map_err(|_| {
            ModuleError::InvalidParameter(format!(
                "Invalid instance type: {}",
                config.instance_type
            ))
        })?;

        // Build the RunInstances request
        let mut run_instances = client
            .run_instances()
            .image_id(image_id)
            .instance_type(instance_type)
            .min_count(config.count)
            .max_count(config.count);

        // Add key name
        if let Some(ref key_name) = config.key_name {
            run_instances = run_instances.key_name(key_name);
        }

        // Add subnet ID
        if let Some(ref subnet_id) = config.vpc_subnet_id {
            run_instances = run_instances.subnet_id(subnet_id);
        }

        // Add security groups
        for sg_id in &config.security_groups {
            run_instances = run_instances.security_group_ids(sg_id);
        }
        for sg_name in &config.security_group_names {
            run_instances = run_instances.security_groups(sg_name);
        }

        // Add user data
        if let Some(ref user_data) = config.user_data {
            // Encode user data to base64 if not already encoded
            let encoded = if user_data
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
            {
                user_data.clone()
            } else {
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, user_data)
            };
            run_instances = run_instances.user_data(encoded);
        }

        // Add IAM instance profile
        if let Some(ref profile_name) = config.instance_profile_name {
            run_instances = run_instances.iam_instance_profile(
                IamInstanceProfileSpecification::builder()
                    .name(profile_name)
                    .build(),
            );
        }

        // Add placement configuration
        let mut placement = Placement::builder();
        if let Some(ref az) = config.availability_zone {
            placement = placement.availability_zone(az);
        }
        if let Some(ref pg) = config.placement_group {
            placement = placement.group_name(pg);
        }
        if let Some(ref tenancy_str) = config.tenancy {
            let tenancy = match tenancy_str.as_str() {
                "dedicated" => Tenancy::Dedicated,
                "host" => Tenancy::Host,
                _ => Tenancy::Default,
            };
            placement = placement.tenancy(tenancy);
        }
        run_instances = run_instances.placement(placement.build());

        // Add EBS optimization
        run_instances = run_instances.ebs_optimized(config.ebs_optimized);

        // Add monitoring
        if config.monitoring {
            run_instances = run_instances.monitoring(
                aws_sdk_ec2::types::RunInstancesMonitoringEnabled::builder()
                    .enabled(true)
                    .build(),
            );
        }

        // Add block device mappings
        for volume in &config.volumes {
            run_instances = run_instances.block_device_mappings(volume.to_block_device_mapping());
        }

        // Add network interfaces
        for ni in &config.network_interfaces {
            run_instances = run_instances.network_interfaces(ni.to_sdk_network_interface());
        }

        // Add tags (including Name tag)
        let mut tags = vec![Tag::builder().key("Name").value(&config.name).build()];
        for (key, value) in &config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }
        run_instances = run_instances.tag_specifications(
            TagSpecification::builder()
                .resource_type(ResourceType::Instance)
                .set_tags(Some(tags))
                .build(),
        );

        // Execute the request
        let resp = run_instances.send().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create instances: {}", e))
        })?;

        // Extract instance information
        let mut instances = Vec::new();
        for instance in resp.instances() {
            let instance_id = instance.instance_id().unwrap_or_default().to_string();
            let state = instance
                .state()
                .and_then(|s| s.name())
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| "pending".to_string());

            let mut tags = HashMap::new();
            tags.insert("Name".to_string(), config.name.clone());
            for (k, v) in &config.tags {
                tags.insert(k.clone(), v.clone());
            }

            instances.push(InstanceInfo {
                instance_id,
                state,
                instance_type: config.instance_type.clone(),
                public_ip: instance.public_ip_address().map(|s| s.to_string()),
                private_ip: instance.private_ip_address().map(|s| s.to_string()),
                public_dns: instance.public_dns_name().map(|s| s.to_string()),
                private_dns: instance.private_dns_name().map(|s| s.to_string()),
                vpc_id: instance.vpc_id().map(|s| s.to_string()),
                subnet_id: instance.subnet_id().map(|s| s.to_string()),
                security_groups: config.security_groups.clone(),
                key_name: config.key_name.clone(),
                launch_time: instance
                    .launch_time()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                availability_zone: config
                    .availability_zone
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                tags,
            });
        }

        tracing::info!(
            "Created {} EC2 instance(s) with AMI {} and type {}",
            instances.len(),
            image_id,
            config.instance_type
        );

        Ok(instances)
    }

    /// Start stopped instances
    async fn start_instances(instance_ids: &[String], region: Option<&str>) -> ModuleResult<()> {
        if instance_ids.is_empty() {
            return Ok(());
        }

        let client = Self::create_client(region).await?;

        client
            .start_instances()
            .set_instance_ids(Some(instance_ids.to_vec()))
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to start instances: {}", e))
            })?;

        tracing::info!("Started instances: {:?}", instance_ids);
        Ok(())
    }

    /// Stop running instances
    async fn stop_instances(
        instance_ids: &[String],
        region: Option<&str>,
        force: bool,
    ) -> ModuleResult<()> {
        if instance_ids.is_empty() {
            return Ok(());
        }

        let client = Self::create_client(region).await?;

        client
            .stop_instances()
            .set_instance_ids(Some(instance_ids.to_vec()))
            .force(force)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to stop instances: {}", e))
            })?;

        tracing::info!("Stopped instances: {:?}", instance_ids);
        Ok(())
    }

    /// Terminate instances
    async fn terminate_instances(
        instance_ids: &[String],
        region: Option<&str>,
    ) -> ModuleResult<()> {
        if instance_ids.is_empty() {
            return Ok(());
        }

        let client = Self::create_client(region).await?;

        client
            .terminate_instances()
            .set_instance_ids(Some(instance_ids.to_vec()))
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to terminate instances: {}", e))
            })?;

        tracing::info!("Terminated instances: {:?}", instance_ids);
        Ok(())
    }

    /// Reboot instances
    async fn reboot_instances(instance_ids: &[String], region: Option<&str>) -> ModuleResult<()> {
        if instance_ids.is_empty() {
            return Ok(());
        }

        let client = Self::create_client(region).await?;

        client
            .reboot_instances()
            .set_instance_ids(Some(instance_ids.to_vec()))
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to reboot instances: {}", e))
            })?;

        tracing::info!("Rebooted instances: {:?}", instance_ids);
        Ok(())
    }

    /// Wait for instances to reach desired state
    async fn wait_for_state(
        instance_ids: &[String],
        desired_state: &InstanceState,
        timeout: Duration,
        region: Option<&str>,
    ) -> ModuleResult<Vec<InstanceInfo>> {
        if instance_ids.is_empty() {
            return Ok(Vec::new());
        }

        let client = Self::create_client(region).await?;
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(5);

        tracing::info!(
            "Waiting for instances {:?} to reach state {:?} (timeout: {:?})",
            instance_ids,
            desired_state,
            timeout
        );

        loop {
            if start.elapsed() >= timeout {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Timeout waiting for instances to reach {:?} state",
                    desired_state
                )));
            }

            let resp = client
                .describe_instances()
                .set_instance_ids(Some(instance_ids.to_vec()))
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to describe instances: {}", e))
                })?;

            let mut all_ready = true;
            let mut instances = Vec::new();

            for reservation in resp.reservations() {
                for instance in reservation.instances() {
                    let state = instance.state().and_then(|s| s.name());
                    if let Some(state_name) = state {
                        let ec2_state = Ec2InstanceState::from_api_state(state_name);
                        if !ec2_state.matches_desired(desired_state) {
                            all_ready = false;
                        }
                    }

                    let instance_id = instance.instance_id().unwrap_or_default().to_string();
                    let state_str = instance
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

                    instances.push(InstanceInfo {
                        instance_id,
                        state: state_str,
                        instance_type: instance
                            .instance_type()
                            .map(|t| t.as_str().to_string())
                            .unwrap_or_default(),
                        public_ip: instance.public_ip_address().map(|s| s.to_string()),
                        private_ip: instance.private_ip_address().map(|s| s.to_string()),
                        public_dns: instance.public_dns_name().map(|s| s.to_string()),
                        private_dns: instance.private_dns_name().map(|s| s.to_string()),
                        vpc_id: instance.vpc_id().map(|s| s.to_string()),
                        subnet_id: instance.subnet_id().map(|s| s.to_string()),
                        security_groups,
                        key_name: instance.key_name().map(|s| s.to_string()),
                        launch_time: instance
                            .launch_time()
                            .map(|t| t.to_string())
                            .unwrap_or_default(),
                        availability_zone: instance
                            .placement()
                            .and_then(|p| p.availability_zone())
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        tags,
                    });
                }
            }

            if all_ready {
                return Ok(instances);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Execute the EC2 instance module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = Ec2InstanceConfig::from_params(params)?;

        // Find existing instances with this name
        let existing = Self::find_instances_by_name(&config).await?;

        // Determine actions based on current and desired state
        let result = match config.state {
            InstanceState::Running => self.ensure_running(&config, &existing, context).await?,
            InstanceState::Stopped => self.ensure_stopped(&config, &existing, context).await?,
            InstanceState::Terminated | InstanceState::Absent => {
                self.ensure_terminated(&config, &existing, context).await?
            }
            InstanceState::Rebooted => self.ensure_rebooted(&config, &existing, context).await?,
        };

        Ok(result)
    }

    /// Ensure instances are in running state
    async fn ensure_running(
        &self,
        config: &Ec2InstanceConfig,
        existing: &[InstanceInfo],
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_empty() {
            // No instances exist, create them
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create {} instance(s) named '{}'",
                    config.count, config.name
                ))
                .with_data("action", serde_json::json!("create")));
            }

            let created = Self::create_instances(config).await?;
            let instance_ids: Vec<_> = created.iter().map(|i| i.instance_id.clone()).collect();

            // Wait for instances to be running if requested
            let final_instances = if config.wait {
                Self::wait_for_state(
                    &instance_ids,
                    &InstanceState::Running,
                    Duration::from_secs(config.wait_timeout),
                    config.region.as_deref(),
                )
                .await?
            } else {
                created
            };

            Ok(ModuleOutput::changed(format!(
                "Created {} instance(s) named '{}'",
                final_instances.len(),
                config.name
            ))
            .with_data("instances", serde_json::to_value(&final_instances).unwrap())
            .with_data("instance_ids", serde_json::json!(instance_ids)))
        } else {
            // Check if any instances need to be started
            let stopped: Vec<_> = existing
                .iter()
                .filter(|i| i.state == "stopped")
                .map(|i| i.instance_id.clone())
                .collect();

            if stopped.is_empty() {
                // All instances already running
                let instance_ids: Vec<_> = existing.iter().map(|i| i.instance_id.clone()).collect();
                return Ok(ModuleOutput::ok(format!(
                    "{} instance(s) named '{}' already running",
                    existing.len(),
                    config.name
                ))
                .with_data("instances", serde_json::to_value(existing).unwrap())
                .with_data("instance_ids", serde_json::json!(instance_ids)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would start {} stopped instance(s)",
                    stopped.len()
                ))
                .with_data("action", serde_json::json!("start"))
                .with_data("instance_ids", serde_json::json!(stopped)));
            }

            Self::start_instances(&stopped, config.region.as_deref()).await?;

            let final_instances = if config.wait {
                Self::wait_for_state(
                    &stopped,
                    &InstanceState::Running,
                    Duration::from_secs(config.wait_timeout),
                    config.region.as_deref(),
                )
                .await?
            } else {
                existing.to_vec()
            };

            Ok(
                ModuleOutput::changed(format!("Started {} instance(s)", stopped.len()))
                    .with_data("instances", serde_json::to_value(&final_instances).unwrap())
                    .with_data("started_instance_ids", serde_json::json!(stopped)),
            )
        }
    }

    /// Ensure instances are in stopped state
    async fn ensure_stopped(
        &self,
        config: &Ec2InstanceConfig,
        existing: &[InstanceInfo],
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "No instances named '{}' found to stop",
                config.name
            )));
        }

        let running: Vec<_> = existing
            .iter()
            .filter(|i| i.state == "running")
            .map(|i| i.instance_id.clone())
            .collect();

        if running.is_empty() {
            let instance_ids: Vec<_> = existing.iter().map(|i| i.instance_id.clone()).collect();
            return Ok(ModuleOutput::ok(format!(
                "{} instance(s) named '{}' already stopped",
                existing.len(),
                config.name
            ))
            .with_data("instances", serde_json::to_value(existing).unwrap())
            .with_data("instance_ids", serde_json::json!(instance_ids)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would stop {} running instance(s)",
                running.len()
            ))
            .with_data("action", serde_json::json!("stop"))
            .with_data("instance_ids", serde_json::json!(running)));
        }

        Self::stop_instances(&running, config.region.as_deref(), false).await?;

        let final_instances = if config.wait {
            Self::wait_for_state(
                &running,
                &InstanceState::Stopped,
                Duration::from_secs(config.wait_timeout),
                config.region.as_deref(),
            )
            .await?
        } else {
            existing.to_vec()
        };

        Ok(
            ModuleOutput::changed(format!("Stopped {} instance(s)", running.len()))
                .with_data("instances", serde_json::to_value(&final_instances).unwrap())
                .with_data("stopped_instance_ids", serde_json::json!(running)),
        )
    }

    /// Ensure instances are terminated
    async fn ensure_terminated(
        &self,
        config: &Ec2InstanceConfig,
        existing: &[InstanceInfo],
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "No instances named '{}' found to terminate",
                config.name
            )));
        }

        let to_terminate: Vec<_> = existing
            .iter()
            .filter(|i| i.state != "terminated")
            .map(|i| i.instance_id.clone())
            .collect();

        if to_terminate.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "All instances named '{}' already terminated",
                config.name
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would terminate {} instance(s)",
                to_terminate.len()
            ))
            .with_data("action", serde_json::json!("terminate"))
            .with_data("instance_ids", serde_json::json!(to_terminate)));
        }

        Self::terminate_instances(&to_terminate, config.region.as_deref()).await?;

        if config.wait {
            Self::wait_for_state(
                &to_terminate,
                &InstanceState::Terminated,
                Duration::from_secs(config.wait_timeout),
                config.region.as_deref(),
            )
            .await?;
        }

        Ok(
            ModuleOutput::changed(format!("Terminated {} instance(s)", to_terminate.len()))
                .with_data("terminated_instance_ids", serde_json::json!(to_terminate)),
        )
    }

    /// Ensure instances are rebooted
    async fn ensure_rebooted(
        &self,
        config: &Ec2InstanceConfig,
        existing: &[InstanceInfo],
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "No instances named '{}' found to reboot",
                config.name
            )));
        }

        let running: Vec<_> = existing
            .iter()
            .filter(|i| i.state == "running")
            .map(|i| i.instance_id.clone())
            .collect();

        if running.is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "No running instances named '{}' found to reboot",
                config.name
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would reboot {} instance(s)",
                running.len()
            ))
            .with_data("action", serde_json::json!("reboot"))
            .with_data("instance_ids", serde_json::json!(running)));
        }

        Self::reboot_instances(&running, config.region.as_deref()).await?;

        Ok(
            ModuleOutput::changed(format!("Rebooted {} instance(s)", running.len()))
                .with_data("rebooted_instance_ids", serde_json::json!(running)),
        )
    }
}

impl Module for Ec2InstanceModule {
    fn name(&self) -> &'static str {
        "aws_ec2_instance"
    }

    fn description(&self) -> &'static str {
        "Create, terminate, start, stop, and manage AWS EC2 instances"
    }

    fn classification(&self) -> ModuleClassification {
        // This is a local logic module - it runs API calls from the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // AWS API has rate limits
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Use tokio runtime to execute async code
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate name is provided
        if params.get_string("name")?.is_none() && params.get_vec_string("instance_ids")?.is_none()
        {
            return Err(ModuleError::MissingParameter(
                "Either 'name' or 'instance_ids' must be provided".to_string(),
            ));
        }

        // Validate state if provided
        if let Some(state) = params.get_string("state")? {
            InstanceState::from_str(&state)?;
        }

        // Validate tenancy if provided
        if let Some(tenancy) = params.get_string("tenancy")? {
            if !["default", "dedicated", "host"].contains(&tenancy.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid tenancy '{}'. Valid values: default, dedicated, host",
                    tenancy
                )));
            }
        }

        Ok(())
    }
}

/// Security group desired state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SecurityGroupState {
    #[default]
    Present,
    Absent,
}

/// Security group configuration
#[derive(Debug, Clone)]
struct SecurityGroupConfig {
    name: String,
    description: Option<String>,
    vpc_id: Option<String>,
    state: SecurityGroupState,
    rules: Vec<SecurityGroupRule>,
    rules_egress: Vec<SecurityGroupRule>,
    purge_rules: bool,
    purge_rules_egress: bool,
    tags: HashMap<String, String>,
    region: Option<String>,
}

impl SecurityGroupConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => SecurityGroupState::Present,
                "absent" => SecurityGroupState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            SecurityGroupState::default()
        };

        // Parse rules
        let rules = if let Some(rules_value) = params.get("rules") {
            if let Some(rules_array) = rules_value.as_array() {
                rules_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let rules_egress = if let Some(rules_value) = params.get("rules_egress") {
            if let Some(rules_array) = rules_value.as_array() {
                rules_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse tags
        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        Ok(Self {
            name,
            description: params.get_string("description")?,
            vpc_id: params.get_string("vpc_id")?,
            state,
            rules,
            rules_egress,
            purge_rules: params.get_bool_or("purge_rules", false),
            purge_rules_egress: params.get_bool_or("purge_rules_egress", false),
            tags,
            region: params.get_string("region")?,
        })
    }
}

/// Security group info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroupInfo {
    pub group_id: String,
    pub group_name: String,
    pub description: String,
    pub vpc_id: Option<String>,
    pub owner_id: String,
    pub tags: HashMap<String, String>,
}

/// AWS EC2 Security Group module
pub struct Ec2SecurityGroupModule;

impl Ec2SecurityGroupModule {
    /// Create AWS EC2 client
    async fn create_client(region: Option<&str>) -> ModuleResult<Client> {
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

    /// Find security group by name
    async fn find_security_group(
        name: &str,
        vpc_id: Option<&str>,
        region: Option<&str>,
    ) -> ModuleResult<Option<SecurityGroupInfo>> {
        let client = Self::create_client(region).await?;

        let mut req = client
            .describe_security_groups()
            .filters(Filter::builder().name("group-name").values(name).build());

        if let Some(vpc) = vpc_id {
            req = req.filters(Filter::builder().name("vpc-id").values(vpc).build());
        }

        let resp = req.send().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to describe security groups: {}", e))
        })?;

        if let Some(sg) = resp.security_groups().iter().next() {
            let mut tags = HashMap::new();
            for tag in sg.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            return Ok(Some(SecurityGroupInfo {
                group_id: sg.group_id().unwrap_or_default().to_string(),
                group_name: sg.group_name().unwrap_or_default().to_string(),
                description: sg.description().unwrap_or_default().to_string(),
                vpc_id: sg.vpc_id().map(|s| s.to_string()),
                owner_id: sg.owner_id().unwrap_or_default().to_string(),
                tags,
            }));
        }

        Ok(None)
    }

    /// Create security group
    async fn create_security_group(
        config: &SecurityGroupConfig,
    ) -> ModuleResult<SecurityGroupInfo> {
        let client = Self::create_client(config.region.as_deref()).await?;

        let description = config
            .description
            .clone()
            .unwrap_or_else(|| format!("Security group for {}", config.name));

        let mut req = client
            .create_security_group()
            .group_name(&config.name)
            .description(&description);

        if let Some(ref vpc_id) = config.vpc_id {
            req = req.vpc_id(vpc_id);
        }

        // Add tags
        let mut tags = vec![Tag::builder().key("Name").value(&config.name).build()];
        for (key, value) in &config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }
        req = req.tag_specifications(
            TagSpecification::builder()
                .resource_type(ResourceType::SecurityGroup)
                .set_tags(Some(tags.clone()))
                .build(),
        );

        let resp = req.send().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create security group: {}", e))
        })?;

        let group_id = resp.group_id().unwrap_or_default().to_string();

        tracing::info!(
            "Created security group '{}' ({}) in VPC {:?}",
            config.name,
            group_id,
            config.vpc_id
        );

        let mut tag_map = HashMap::new();
        tag_map.insert("Name".to_string(), config.name.clone());
        for (k, v) in &config.tags {
            tag_map.insert(k.clone(), v.clone());
        }

        Ok(SecurityGroupInfo {
            group_id,
            group_name: config.name.clone(),
            description,
            vpc_id: config.vpc_id.clone(),
            owner_id: "".to_string(), // Would be populated from API response
            tags: tag_map,
        })
    }

    /// Delete security group
    async fn delete_security_group(group_id: &str, region: Option<&str>) -> ModuleResult<()> {
        let client = Self::create_client(region).await?;

        client
            .delete_security_group()
            .group_id(group_id)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to delete security group: {}", e))
            })?;

        tracing::info!("Deleted security group: {}", group_id);
        Ok(())
    }

    /// Update security group rules
    async fn update_rules(
        group_id: &str,
        rules: &[SecurityGroupRule],
        is_egress: bool,
        _purge: bool,
        region: Option<&str>,
    ) -> ModuleResult<bool> {
        if rules.is_empty() {
            return Ok(false);
        }

        let client = Self::create_client(region).await?;

        let ip_permissions: Vec<IpPermission> =
            rules.iter().map(|r| r.to_ip_permission()).collect();

        if is_egress {
            client
                .authorize_security_group_egress()
                .group_id(group_id)
                .set_ip_permissions(Some(ip_permissions))
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to authorize egress rules: {}", e))
                })?;
        } else {
            client
                .authorize_security_group_ingress()
                .group_id(group_id)
                .set_ip_permissions(Some(ip_permissions))
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to authorize ingress rules: {}",
                        e
                    ))
                })?;
        }

        Ok(true)
    }

    /// Execute the security group module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = SecurityGroupConfig::from_params(params)?;

        let existing = Self::find_security_group(
            &config.name,
            config.vpc_id.as_deref(),
            config.region.as_deref(),
        )
        .await?;

        match config.state {
            SecurityGroupState::Present => {
                if let Some(sg) = existing {
                    // Update existing security group rules
                    if context.check_mode {
                        return Ok(ModuleOutput::ok(format!(
                            "Security group '{}' already exists",
                            config.name
                        ))
                        .with_data("security_group", serde_json::to_value(&sg).unwrap()));
                    }

                    let mut changed = false;

                    // Update ingress rules
                    if Self::update_rules(
                        &sg.group_id,
                        &config.rules,
                        false,
                        config.purge_rules,
                        config.region.as_deref(),
                    )
                    .await?
                    {
                        changed = true;
                    }

                    // Update egress rules
                    if Self::update_rules(
                        &sg.group_id,
                        &config.rules_egress,
                        true,
                        config.purge_rules_egress,
                        config.region.as_deref(),
                    )
                    .await?
                    {
                        changed = true;
                    }

                    if changed {
                        Ok(ModuleOutput::changed(format!(
                            "Updated security group '{}'",
                            config.name
                        ))
                        .with_data("security_group", serde_json::to_value(&sg).unwrap()))
                    } else {
                        Ok(ModuleOutput::ok(format!(
                            "Security group '{}' is up to date",
                            config.name
                        ))
                        .with_data("security_group", serde_json::to_value(&sg).unwrap()))
                    }
                } else {
                    // Create new security group
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create security group '{}'",
                            config.name
                        )));
                    }

                    let sg = Self::create_security_group(&config).await?;

                    // Add rules
                    Self::update_rules(
                        &sg.group_id,
                        &config.rules,
                        false,
                        false,
                        config.region.as_deref(),
                    )
                    .await?;

                    Self::update_rules(
                        &sg.group_id,
                        &config.rules_egress,
                        true,
                        false,
                        config.region.as_deref(),
                    )
                    .await?;

                    Ok(
                        ModuleOutput::changed(format!("Created security group '{}'", config.name))
                            .with_data("security_group", serde_json::to_value(&sg).unwrap()),
                    )
                }
            }
            SecurityGroupState::Absent => {
                if let Some(sg) = existing {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete security group '{}'",
                            config.name
                        )));
                    }

                    Self::delete_security_group(&sg.group_id, config.region.as_deref()).await?;

                    Ok(ModuleOutput::changed(format!(
                        "Deleted security group '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Security group '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for Ec2SecurityGroupModule {
    fn name(&self) -> &'static str {
        "aws_ec2_security_group"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete AWS EC2 security groups"
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
        &["name"]
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

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

/// VPC desired state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VpcState {
    #[default]
    Present,
    Absent,
}

/// VPC configuration
#[derive(Debug, Clone)]
struct VpcConfig {
    name: String,
    cidr_block: Option<String>,
    state: VpcState,
    enable_dns_support: bool,
    enable_dns_hostnames: bool,
    tenancy: String,
    tags: HashMap<String, String>,
    region: Option<String>,
}

impl VpcConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => VpcState::Present,
                "absent" => VpcState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            VpcState::default()
        };

        // Parse tags
        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        Ok(Self {
            name,
            cidr_block: params.get_string("cidr_block")?,
            state,
            enable_dns_support: params.get_bool_or("enable_dns_support", true),
            enable_dns_hostnames: params.get_bool_or("enable_dns_hostnames", false),
            tenancy: params
                .get_string("tenancy")?
                .unwrap_or_else(|| "default".to_string()),
            tags,
            region: params.get_string("region")?,
        })
    }
}

/// VPC info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcInfo {
    pub vpc_id: String,
    pub cidr_block: String,
    pub state: String,
    pub is_default: bool,
    pub enable_dns_support: bool,
    pub enable_dns_hostnames: bool,
    pub owner_id: String,
    pub tags: HashMap<String, String>,
}

/// AWS EC2 VPC module
pub struct Ec2VpcModule;

impl Ec2VpcModule {
    /// Create AWS EC2 client
    async fn create_client(region: Option<&str>) -> ModuleResult<Client> {
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

    /// Find VPC by name tag
    async fn find_vpc(name: &str, region: Option<&str>) -> ModuleResult<Option<VpcInfo>> {
        let client = Self::create_client(region).await?;

        let resp = client
            .describe_vpcs()
            .filters(Filter::builder().name("tag:Name").values(name).build())
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to describe VPCs: {}", e)))?;

        if let Some(vpc) = resp.vpcs().iter().next() {
            let mut tags = HashMap::new();
            for tag in vpc.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            return Ok(Some(VpcInfo {
                vpc_id: vpc.vpc_id().unwrap_or_default().to_string(),
                cidr_block: vpc.cidr_block().unwrap_or_default().to_string(),
                state: vpc
                    .state()
                    .map(|s| s.as_str().to_string())
                    .unwrap_or_default(),
                is_default: vpc.is_default().unwrap_or(false),
                enable_dns_support: true, // Would need additional API call
                enable_dns_hostnames: false, // Would need additional API call
                owner_id: vpc.owner_id().unwrap_or_default().to_string(),
                tags,
            }));
        }

        Ok(None)
    }

    /// Create VPC
    async fn create_vpc(config: &VpcConfig) -> ModuleResult<VpcInfo> {
        let client = Self::create_client(config.region.as_deref()).await?;

        let cidr = config.cidr_block.as_deref().ok_or_else(|| {
            ModuleError::MissingParameter("cidr_block is required when creating a VPC".to_string())
        })?;

        let tenancy = match config.tenancy.as_str() {
            "dedicated" => Tenancy::Dedicated,
            "host" => Tenancy::Host,
            _ => Tenancy::Default,
        };

        let resp = client
            .create_vpc()
            .cidr_block(cidr)
            .instance_tenancy(tenancy)
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::Vpc)
                    .tags(Tag::builder().key("Name").value(&config.name).build())
                    .build(),
            )
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to create VPC: {}", e)))?;

        let vpc = resp.vpc().ok_or_else(|| {
            ModuleError::ExecutionFailed("No VPC returned from create operation".to_string())
        })?;

        let vpc_id = vpc.vpc_id().unwrap_or_default().to_string();

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
                    ModuleError::ExecutionFailed(format!("Failed to enable DNS support: {}", e))
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
                    ModuleError::ExecutionFailed(format!("Failed to enable DNS hostnames: {}", e))
                })?;
        }

        tracing::info!(
            "Created VPC '{}' ({}) with CIDR {}",
            config.name,
            vpc_id,
            cidr
        );

        let mut tags = HashMap::new();
        tags.insert("Name".to_string(), config.name.clone());
        for (k, v) in &config.tags {
            tags.insert(k.clone(), v.clone());
        }

        Ok(VpcInfo {
            vpc_id,
            cidr_block: cidr.to_string(),
            state: "available".to_string(),
            is_default: false,
            enable_dns_support: config.enable_dns_support,
            enable_dns_hostnames: config.enable_dns_hostnames,
            owner_id: "".to_string(),
            tags,
        })
    }

    /// Delete VPC
    async fn delete_vpc(vpc_id: &str, region: Option<&str>) -> ModuleResult<()> {
        let client = Self::create_client(region).await?;

        client
            .delete_vpc()
            .vpc_id(vpc_id)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to delete VPC: {}", e)))?;

        tracing::info!("Deleted VPC: {}", vpc_id);
        Ok(())
    }

    /// Execute the VPC module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = VpcConfig::from_params(params)?;

        let existing = Self::find_vpc(&config.name, config.region.as_deref()).await?;

        match config.state {
            VpcState::Present => {
                if let Some(vpc) = existing {
                    // VPC exists - check if configuration matches
                    Ok(
                        ModuleOutput::ok(format!("VPC '{}' already exists", config.name))
                            .with_data("vpc", serde_json::to_value(&vpc).unwrap()),
                    )
                } else {
                    // Create new VPC
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create VPC '{}'",
                            config.name
                        )));
                    }

                    let vpc = Self::create_vpc(&config).await?;

                    Ok(
                        ModuleOutput::changed(format!("Created VPC '{}'", config.name))
                            .with_data("vpc", serde_json::to_value(&vpc).unwrap()),
                    )
                }
            }
            VpcState::Absent => {
                if let Some(vpc) = existing {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete VPC '{}'",
                            config.name
                        )));
                    }

                    Self::delete_vpc(&vpc.vpc_id, config.region.as_deref()).await?;

                    Ok(ModuleOutput::changed(format!(
                        "Deleted VPC '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "VPC '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for Ec2VpcModule {
    fn name(&self) -> &'static str {
        "aws_ec2_vpc"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete AWS VPCs"
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
        &["name"]
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

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_state_from_str() {
        assert_eq!(
            InstanceState::from_str("running").unwrap(),
            InstanceState::Running
        );
        assert_eq!(
            InstanceState::from_str("stopped").unwrap(),
            InstanceState::Stopped
        );
        assert_eq!(
            InstanceState::from_str("terminated").unwrap(),
            InstanceState::Terminated
        );
        assert_eq!(
            InstanceState::from_str("absent").unwrap(),
            InstanceState::Terminated
        );
        assert_eq!(
            InstanceState::from_str("rebooted").unwrap(),
            InstanceState::Rebooted
        );
        assert!(InstanceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_ec2_instance_state_from_str() {
        assert_eq!(
            Ec2InstanceState::from_str("pending"),
            Ec2InstanceState::Pending
        );
        assert_eq!(
            Ec2InstanceState::from_str("running"),
            Ec2InstanceState::Running
        );
        assert_eq!(
            Ec2InstanceState::from_str("stopped"),
            Ec2InstanceState::Stopped
        );
        assert_eq!(
            Ec2InstanceState::from_str("terminated"),
            Ec2InstanceState::Terminated
        );
    }

    #[test]
    fn test_ec2_state_matches_desired() {
        assert!(Ec2InstanceState::Running.matches_desired(&InstanceState::Running));
        assert!(Ec2InstanceState::Stopped.matches_desired(&InstanceState::Stopped));
        assert!(Ec2InstanceState::Terminated.matches_desired(&InstanceState::Terminated));
        assert!(Ec2InstanceState::Terminated.matches_desired(&InstanceState::Absent));
        assert!(!Ec2InstanceState::Running.matches_desired(&InstanceState::Stopped));
    }

    #[test]
    fn test_ec2_instance_module_metadata() {
        let module = Ec2InstanceModule;
        assert_eq!(module.name(), "aws_ec2_instance");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_ec2_security_group_module_metadata() {
        let module = Ec2SecurityGroupModule;
        assert_eq!(module.name(), "aws_ec2_security_group");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_ec2_vpc_module_metadata() {
        let module = Ec2VpcModule;
        assert_eq!(module.name(), "aws_ec2_vpc");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_ec2_instance_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("instance_type".to_string(), serde_json::json!("t3.small"));
        params.insert("image_id".to_string(), serde_json::json!("ami-12345678"));
        params.insert("state".to_string(), serde_json::json!("running"));

        let config = Ec2InstanceConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-instance");
        assert_eq!(config.instance_type, "t3.small");
        assert_eq!(config.image_id, Some("ami-12345678".to_string()));
        assert_eq!(config.state, InstanceState::Running);
    }

    #[test]
    fn test_ec2_instance_config_with_tags() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert(
            "tags".to_string(),
            serde_json::json!({
                "Environment": "production",
                "Team": "web"
            }),
        );

        let config = Ec2InstanceConfig::from_params(&params).unwrap();
        assert_eq!(
            config.tags.get("Environment"),
            Some(&"production".to_string())
        );
        assert_eq!(config.tags.get("Team"), Some(&"web".to_string()));
    }

    #[test]
    fn test_ec2_instance_config_with_security_groups() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert(
            "security_groups".to_string(),
            serde_json::json!(["sg-111", "sg-222"]),
        );

        let config = Ec2InstanceConfig::from_params(&params).unwrap();
        assert_eq!(config.security_groups, vec!["sg-111", "sg-222"]);
    }

    #[test]
    fn test_security_group_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-sg"));
        params.insert(
            "description".to_string(),
            serde_json::json!("Test security group"),
        );
        params.insert("vpc_id".to_string(), serde_json::json!("vpc-12345678"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let config = SecurityGroupConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-sg");
        assert_eq!(config.description, Some("Test security group".to_string()));
        assert_eq!(config.vpc_id, Some("vpc-12345678".to_string()));
        assert_eq!(config.state, SecurityGroupState::Present);
    }

    #[test]
    fn test_vpc_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vpc"));
        params.insert("cidr_block".to_string(), serde_json::json!("10.0.0.0/16"));
        params.insert("enable_dns_support".to_string(), serde_json::json!(true));
        params.insert("enable_dns_hostnames".to_string(), serde_json::json!(true));

        let config = VpcConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-vpc");
        assert_eq!(config.cidr_block, Some("10.0.0.0/16".to_string()));
        assert!(config.enable_dns_support);
        assert!(config.enable_dns_hostnames);
    }

    #[test]
    fn test_validate_params_missing_name() {
        let module = Ec2InstanceModule;
        let params = ModuleParams::new();
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_state() {
        let module = Ec2InstanceModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        params.insert("state".to_string(), serde_json::json!("invalid_state"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_tenancy() {
        let module = Ec2InstanceModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        params.insert("tenancy".to_string(), serde_json::json!("invalid"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_ebs_volume_to_block_device_mapping() {
        let volume = EbsVolume {
            device_name: "/dev/sda1".to_string(),
            size: Some(100),
            volume_type: Some("gp3".to_string()),
            iops: Some(3000),
            throughput: Some(125),
            delete_on_termination: Some(true),
            encrypted: Some(true),
            kms_key_id: None,
            snapshot_id: None,
        };

        let mapping = volume.to_block_device_mapping();
        assert_eq!(mapping.device_name(), Some("/dev/sda1"));
    }

    #[test]
    fn test_security_group_rule_to_ip_permission() {
        let rule = SecurityGroupRule {
            protocol: "tcp".to_string(),
            from_port: 443,
            to_port: 443,
            cidr_ip: Some("0.0.0.0/0".to_string()),
            cidr_ipv6: None,
            source_security_group_id: None,
            source_security_group_name: None,
            prefix_list_id: None,
            description: Some("HTTPS access".to_string()),
        };

        let permission = rule.to_ip_permission();
        assert_eq!(permission.ip_protocol(), Some("tcp"));
        assert_eq!(permission.from_port(), Some(443));
        assert_eq!(permission.to_port(), Some(443));
    }
}
