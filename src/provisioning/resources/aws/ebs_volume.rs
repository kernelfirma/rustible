//! AWS EBS Volume Resource for Infrastructure Provisioning
//!
//! This module implements the `aws_ebs_volume` resource type for managing
//! Elastic Block Store volumes in AWS.
//!
//! # Example
//!
//! ```yaml
//! resources:
//!   aws_ebs_volume:
//!     data_volume:
//!       availability_zone: us-east-1a
//!       size: 100
//!       type: gp3
//!       iops: 3000
//!       throughput: 125
//!       encrypted: true
//!       kms_key_id: "{{ resources.aws_kms_key.data.arn }}"
//!       tags:
//!         Name: data-volume
//!         Environment: production
//!
//!     snapshot_restore:
//!       availability_zone: us-east-1a
//!       snapshot_id: snap-0123456789abcdef0
//!       tags:
//!         Name: restored-volume
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{ResourceType, Tag, TagSpecification, VolumeType};
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
// EBS Volume Resource
// ============================================================================

/// AWS EBS Volume resource implementation
#[derive(Debug, Clone)]
pub struct AwsEbsVolumeResource;

impl AwsEbsVolumeResource {
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
    fn parse_config(&self, config: &Value) -> ProvisioningResult<EbsVolumeConfig> {
        serde_json::from_value(config.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!("Invalid EBS volume configuration: {}", e))
        })
    }

    /// Find EBS volume by ID
    async fn find_by_id(
        &self,
        volume_id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<EbsVolumeState>> {
        let client = self.create_client(ctx).await?;

        let resp = client.describe_volumes().volume_ids(volume_id).send().await;

        match resp {
            Ok(r) => {
                if let Some(volume) = r.volumes().iter().next() {
                    return Ok(Some(self.parse_volume(volume)));
                }
                Ok(None)
            }
            Err(e) => {
                if e.to_string().contains("InvalidVolume.NotFound") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to describe volume: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Parse AWS Volume into our state
    fn parse_volume(&self, vol: &aws_sdk_ec2::types::Volume) -> EbsVolumeState {
        let mut tags = HashMap::new();
        for tag in vol.tags() {
            if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                tags.insert(key.to_string(), value.to_string());
            }
        }

        EbsVolumeState {
            id: vol.volume_id().unwrap_or_default().to_string(),
            arn: format!(
                "arn:aws:ec2:{}:{}:volume/{}",
                "", // Region would come from context
                "",
                vol.volume_id().unwrap_or_default()
            ),
            availability_zone: vol.availability_zone().unwrap_or_default().to_string(),
            size: vol.size().unwrap_or_default(),
            volume_type: vol
                .volume_type()
                .map(|t| t.as_str().to_string())
                .unwrap_or_else(|| "gp2".to_string()),
            iops: vol.iops(),
            throughput: vol.throughput(),
            encrypted: vol.encrypted().unwrap_or(false),
            kms_key_id: vol.kms_key_id().map(|s| s.to_string()),
            snapshot_id: vol.snapshot_id().map(|s| s.to_string()),
            state: vol
                .state()
                .map(|s| s.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            tags,
        }
    }

    /// Convert string to VolumeType
    fn parse_volume_type(&self, s: &str) -> VolumeType {
        match s.to_lowercase().as_str() {
            "gp2" => VolumeType::Gp2,
            "gp3" => VolumeType::Gp3,
            "io1" => VolumeType::Io1,
            "io2" => VolumeType::Io2,
            "st1" => VolumeType::St1,
            "sc1" => VolumeType::Sc1,
            "standard" => VolumeType::Standard,
            _ => VolumeType::Gp3, // Default to gp3
        }
    }
}

impl Default for AwsEbsVolumeResource {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EBS Volume Configuration (from YAML/JSON)
// ============================================================================

/// EBS Volume configuration as parsed from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbsVolumeConfig {
    /// Availability zone (required)
    pub availability_zone: String,

    /// Size in GiB (required unless snapshot_id provided)
    #[serde(default)]
    pub size: Option<i32>,

    /// Volume type (gp2, gp3, io1, io2, st1, sc1, standard)
    #[serde(rename = "type", default = "default_volume_type")]
    pub volume_type: String,

    /// IOPS (for io1, io2, gp3)
    #[serde(default)]
    pub iops: Option<i32>,

    /// Throughput in MiB/s (for gp3)
    #[serde(default)]
    pub throughput: Option<i32>,

    /// Whether to encrypt the volume
    #[serde(default)]
    pub encrypted: bool,

    /// KMS key ID for encryption
    #[serde(default)]
    pub kms_key_id: Option<String>,

    /// Snapshot ID to restore from
    #[serde(default)]
    pub snapshot_id: Option<String>,

    /// Multi-attach enabled (for io1, io2)
    #[serde(default)]
    pub multi_attach_enabled: bool,

    /// Resource tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn default_volume_type() -> String {
    "gp3".to_string()
}

// ============================================================================
// EBS Volume State (from AWS)
// ============================================================================

/// Current state of an EBS volume from AWS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbsVolumeState {
    /// Volume ID
    pub id: String,

    /// Volume ARN
    pub arn: String,

    /// Availability zone
    pub availability_zone: String,

    /// Size in GiB
    pub size: i32,

    /// Volume type
    pub volume_type: String,

    /// IOPS
    pub iops: Option<i32>,

    /// Throughput in MiB/s
    pub throughput: Option<i32>,

    /// Whether encrypted
    pub encrypted: bool,

    /// KMS key ID
    pub kms_key_id: Option<String>,

    /// Snapshot ID if created from snapshot
    pub snapshot_id: Option<String>,

    /// Current state (creating, available, in-use, deleting, deleted, error)
    pub state: String,

    /// Tags
    pub tags: HashMap<String, String>,
}

// ============================================================================
// Resource Trait Implementation
// ============================================================================

#[async_trait]
impl Resource for AwsEbsVolumeResource {
    fn resource_type(&self) -> &str {
        "aws_ebs_volume"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_ebs_volume".to_string(),
            description: "Provides an AWS EBS volume resource for block storage".to_string(),
            required_args: vec![SchemaField {
                name: "availability_zone".to_string(),
                field_type: FieldType::String,
                description: "Availability zone for the volume".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "size".to_string(),
                    field_type: FieldType::Integer,
                    description: "Size of the volume in GiB".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 1 },
                        FieldConstraint::MaxValue { value: 16384 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "type".to_string(),
                    field_type: FieldType::String,
                    description: "Volume type (gp2, gp3, io1, io2, st1, sc1, standard)".to_string(),
                    default: Some(Value::String("gp3".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "gp2".to_string(),
                            "gp3".to_string(),
                            "io1".to_string(),
                            "io2".to_string(),
                            "st1".to_string(),
                            "sc1".to_string(),
                            "standard".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "iops".to_string(),
                    field_type: FieldType::Integer,
                    description: "IOPS for io1, io2, or gp3 volumes".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 100 },
                        FieldConstraint::MaxValue { value: 256000 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "throughput".to_string(),
                    field_type: FieldType::Integer,
                    description: "Throughput in MiB/s for gp3 volumes".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 125 },
                        FieldConstraint::MaxValue { value: 1000 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "encrypted".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether to encrypt the volume".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "kms_key_id".to_string(),
                    field_type: FieldType::String,
                    description: "KMS key ID for encryption".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "snapshot_id".to_string(),
                    field_type: FieldType::String,
                    description: "Snapshot ID to create volume from".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "multi_attach_enabled".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable multi-attach (io1, io2 only)".to_string(),
                    default: Some(Value::Bool(false)),
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
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Volume ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "Volume ARN".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "state".to_string(),
                    field_type: FieldType::String,
                    description: "Volume state".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "availability_zone".to_string(),
                "snapshot_id".to_string(),
                "encrypted".to_string(),
            ],
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
        debug!("Reading EBS volume: {}", id);

        match self.find_by_id(id, ctx).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize EBS volume state: {}",
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
                let current_state: EbsVolumeState = serde_json::from_value(current_value.clone())
                    .map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to parse current state: {}",
                        e
                    ))
                })?;

                // Check for force_new fields
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                if config.availability_zone != current_state.availability_zone {
                    requires_replacement = true;
                    replacement_fields.push("availability_zone".to_string());
                }

                if config.snapshot_id != current_state.snapshot_id {
                    requires_replacement = true;
                    replacement_fields.push("snapshot_id".to_string());
                }

                if config.encrypted != current_state.encrypted {
                    requires_replacement = true;
                    replacement_fields.push("encrypted".to_string());
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

                // Check for modifiable fields
                let mut modifications = HashMap::new();

                // Size can be increased (not decreased)
                if let Some(new_size) = config.size {
                    if new_size != current_state.size {
                        if new_size < current_state.size {
                            return Err(ProvisioningError::ValidationError(
                                "EBS volume size can only be increased, not decreased".to_string(),
                            ));
                        }
                        modifications.insert(
                            "size".to_string(),
                            (
                                Value::Number(current_state.size.into()),
                                Value::Number(new_size.into()),
                            ),
                        );
                    }
                }

                // Volume type can be changed
                if config.volume_type != current_state.volume_type {
                    modifications.insert(
                        "type".to_string(),
                        (
                            Value::String(current_state.volume_type.clone()),
                            Value::String(config.volume_type.clone()),
                        ),
                    );
                }

                // IOPS can be changed
                if config.iops != current_state.iops {
                    modifications.insert(
                        "iops".to_string(),
                        (
                            serde_json::to_value(current_state.iops).unwrap(),
                            serde_json::to_value(config.iops).unwrap(),
                        ),
                    );
                }

                // Throughput can be changed (gp3 only)
                if config.throughput != current_state.throughput {
                    modifications.insert(
                        "throughput".to_string(),
                        (
                            serde_json::to_value(current_state.throughput).unwrap(),
                            serde_json::to_value(config.throughput).unwrap(),
                        ),
                    );
                }

                // Tags can be changed
                if config.tags != current_state.tags {
                    modifications.insert(
                        "tags".to_string(),
                        (
                            serde_json::to_value(&current_state.tags).unwrap(),
                            serde_json::to_value(&config.tags).unwrap(),
                        ),
                    );
                }

                if modifications.is_empty() {
                    return Ok(ResourceDiff::no_change());
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
        let vol_config = self.parse_config(config)?;
        let client = self.create_client(ctx).await?;

        info!(
            "Creating EBS volume in {}: {} GiB {}",
            vol_config.availability_zone,
            vol_config.size.unwrap_or(0),
            vol_config.volume_type
        );

        // Build tags
        let mut tags = vec![];
        for (key, value) in &vol_config.tags {
            tags.push(Tag::builder().key(key).value(value).build());
        }

        // Apply default tags from provider context
        for (key, value) in &ctx.default_tags {
            if !vol_config.tags.contains_key(key) {
                tags.push(Tag::builder().key(key).value(value).build());
            }
        }

        // Build create request
        let mut req = client
            .create_volume()
            .availability_zone(&vol_config.availability_zone)
            .volume_type(self.parse_volume_type(&vol_config.volume_type))
            .encrypted(vol_config.encrypted);

        // Size (required unless snapshot_id)
        if let Some(size) = vol_config.size {
            req = req.size(size);
        }

        // Snapshot ID
        if let Some(ref snapshot_id) = vol_config.snapshot_id {
            req = req.snapshot_id(snapshot_id);
        }

        // IOPS (for io1, io2, gp3)
        if let Some(iops) = vol_config.iops {
            req = req.iops(iops);
        }

        // Throughput (for gp3)
        if let Some(throughput) = vol_config.throughput {
            req = req.throughput(throughput);
        }

        // KMS key
        if let Some(ref kms_key_id) = vol_config.kms_key_id {
            req = req.kms_key_id(kms_key_id);
        }

        // Multi-attach
        if vol_config.multi_attach_enabled {
            req = req.multi_attach_enabled(true);
        }

        // Tags
        if !tags.is_empty() {
            req = req.tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::Volume)
                    .set_tags(Some(tags))
                    .build(),
            );
        }

        let resp = req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create EBS volume: {}", e))
        })?;

        let volume_id = resp
            .volume_id()
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("No volume ID in create response".to_string())
            })?
            .to_string();

        info!("Created EBS volume: {}", volume_id);

        // Wait for volume to become available
        self.wait_for_available(&client, &volume_id).await?;

        // Read back the created volume
        let state = self.find_by_id(&volume_id, ctx).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("Volume not found after creation".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize EBS volume state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(&volume_id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("state", Value::String(state.state.clone())))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let old_state: EbsVolumeState = serde_json::from_value(old.clone()).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to parse old state: {}", e))
        })?;

        let new_config = self.parse_config(new)?;
        let client = self.create_client(ctx).await?;

        info!("Updating EBS volume: {}", id);

        // Check if we need to modify volume attributes
        let size_changed = new_config.size.is_some_and(|s| s != old_state.size);
        let type_changed = new_config.volume_type != old_state.volume_type;
        let iops_changed = new_config.iops != old_state.iops;
        let throughput_changed = new_config.throughput != old_state.throughput;

        if size_changed || type_changed || iops_changed || throughput_changed {
            let mut req = client.modify_volume().volume_id(id);

            if let Some(size) = new_config.size {
                if size != old_state.size {
                    req = req.size(size);
                }
            }

            if new_config.volume_type != old_state.volume_type {
                req = req.volume_type(self.parse_volume_type(&new_config.volume_type));
            }

            if new_config.iops != old_state.iops {
                if let Some(iops) = new_config.iops {
                    req = req.iops(iops);
                }
            }

            if new_config.throughput != old_state.throughput {
                if let Some(throughput) = new_config.throughput {
                    req = req.throughput(throughput);
                }
            }

            req.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to modify EBS volume: {}", e))
            })?;

            // Wait for modification to complete
            self.wait_for_modification(&client, id).await?;
        }

        // Update tags if changed
        if new_config.tags != old_state.tags {
            // Delete old tags
            let old_keys: Vec<String> = old_state
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k))
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
            ProvisioningError::CloudApiError("Volume not found after update".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize EBS volume state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        info!("Deleting EBS volume: {}", id);

        client
            .delete_volume()
            .volume_id(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete EBS volume: {}", e))
            })?;

        info!("Deleted EBS volume: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        debug!("Importing EBS volume: {}", id);

        let state =
            self.find_by_id(id, ctx)
                .await?
                .ok_or_else(|| ProvisioningError::ImportError {
                    resource_type: "aws_ebs_volume".to_string(),
                    resource_id: id.to_string(),
                    message: "EBS volume not found".to_string(),
                })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize EBS volume state: {}",
                e
            ))
        })?;

        Ok(ResourceResult::success(id, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone())))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Check for KMS key reference
        if let Some(kms_key_id) = config.get("kms_key_id").and_then(|v| v.as_str()) {
            if let Some(captures) = parse_resource_reference(kms_key_id) {
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
            "availability_zone".to_string(),
            "snapshot_id".to_string(),
            "encrypted".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate availability_zone is present
        if config
            .get("availability_zone")
            .and_then(|v| v.as_str())
            .is_none()
        {
            return Err(ProvisioningError::ValidationError(
                "availability_zone is required".to_string(),
            ));
        }

        // Validate size or snapshot_id is present
        let has_size = config.get("size").and_then(|v| v.as_i64()).is_some();
        let has_snapshot = config.get("snapshot_id").and_then(|v| v.as_str()).is_some();

        if !has_size && !has_snapshot {
            return Err(ProvisioningError::ValidationError(
                "Either size or snapshot_id must be specified".to_string(),
            ));
        }

        // Validate volume type
        if let Some(vol_type) = config.get("type").and_then(|v| v.as_str()) {
            let valid_types = ["gp2", "gp3", "io1", "io2", "st1", "sc1", "standard"];
            if !valid_types.contains(&vol_type.to_lowercase().as_str()) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid volume type '{}'. Valid types: {}",
                    vol_type,
                    valid_types.join(", ")
                )));
            }
        }

        // Validate IOPS for supported volume types
        if let Some(iops) = config.get("iops").and_then(|v| v.as_i64()) {
            let vol_type = config.get("type").and_then(|v| v.as_str()).unwrap_or("gp3");
            if !["gp3", "io1", "io2"].contains(&vol_type.to_lowercase().as_str()) {
                return Err(ProvisioningError::ValidationError(format!(
                    "IOPS can only be specified for gp3, io1, or io2 volumes, not {}",
                    vol_type
                )));
            }
            if !(100..=256000).contains(&iops) {
                return Err(ProvisioningError::ValidationError(
                    "IOPS must be between 100 and 256000".to_string(),
                ));
            }
        }

        // Validate throughput for gp3
        if let Some(throughput) = config.get("throughput").and_then(|v| v.as_i64()) {
            let vol_type = config.get("type").and_then(|v| v.as_str()).unwrap_or("gp3");
            if vol_type.to_lowercase() != "gp3" {
                return Err(ProvisioningError::ValidationError(
                    "Throughput can only be specified for gp3 volumes".to_string(),
                ));
            }
            if !(125..=1000).contains(&throughput) {
                return Err(ProvisioningError::ValidationError(
                    "Throughput must be between 125 and 1000 MiB/s".to_string(),
                ));
            }
        }

        // Validate multi-attach for supported types
        if let Some(true) = config.get("multi_attach_enabled").and_then(|v| v.as_bool()) {
            let vol_type = config.get("type").and_then(|v| v.as_str()).unwrap_or("gp3");
            if !["io1", "io2"].contains(&vol_type.to_lowercase().as_str()) {
                return Err(ProvisioningError::ValidationError(
                    "Multi-attach can only be enabled for io1 or io2 volumes".to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl AwsEbsVolumeResource {
    /// Wait for volume to become available
    async fn wait_for_available(&self, client: &Client, volume_id: &str) -> ProvisioningResult<()> {
        use tokio::time::{sleep, Duration};

        for _ in 0..60 {
            let resp = client
                .describe_volumes()
                .volume_ids(volume_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to describe volume: {}", e))
                })?;

            for vol in resp.volumes() {
                if let Some(state) = vol.state() {
                    if state.as_str() == "available" {
                        return Ok(());
                    }
                    if state.as_str() == "error" {
                        return Err(ProvisioningError::CloudApiError(
                            "Volume creation failed".to_string(),
                        ));
                    }
                }
            }

            sleep(Duration::from_secs(5)).await;
        }

        Err(ProvisioningError::CloudApiError(
            "Timeout waiting for volume to become available".to_string(),
        ))
    }

    /// Wait for volume modification to complete
    async fn wait_for_modification(
        &self,
        client: &Client,
        volume_id: &str,
    ) -> ProvisioningResult<()> {
        use tokio::time::{sleep, Duration};

        for _ in 0..120 {
            let resp = client
                .describe_volumes_modifications()
                .volume_ids(volume_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to describe volume modifications: {}",
                        e
                    ))
                })?;

            for mod_info in resp.volumes_modifications() {
                if let Some(state) = mod_info.modification_state() {
                    let state_str = state.as_str();
                    if state_str == "completed" || state_str == "optimizing" {
                        return Ok(());
                    }
                    if state_str == "failed" {
                        return Err(ProvisioningError::CloudApiError(
                            "Volume modification failed".to_string(),
                        ));
                    }
                }
            }

            sleep(Duration::from_secs(5)).await;
        }

        Err(ProvisioningError::CloudApiError(
            "Timeout waiting for volume modification to complete".to_string(),
        ))
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

/// Parse a resource reference string like "{{ resources.aws_kms_key.data.arn }}"
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
        let resource = AwsEbsVolumeResource::new();
        assert_eq!(resource.resource_type(), "aws_ebs_volume");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsEbsVolumeResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_ebs_volume");
        assert!(schema
            .required_args
            .iter()
            .any(|f| f.name == "availability_zone"));
        assert!(schema.optional_args.iter().any(|f| f.name == "size"));
        assert!(schema.optional_args.iter().any(|f| f.name == "type"));
        assert!(schema.optional_args.iter().any(|f| f.name == "iops"));
    }

    #[test]
    fn test_parse_config() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "gp3",
            "iops": 3000,
            "throughput": 125,
            "encrypted": true,
            "tags": {
                "Name": "test-volume"
            }
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.availability_zone, "us-east-1a");
        assert_eq!(parsed.size, Some(100));
        assert_eq!(parsed.volume_type, "gp3");
        assert_eq!(parsed.iops, Some(3000));
        assert_eq!(parsed.throughput, Some(125));
        assert!(parsed.encrypted);
    }

    #[test]
    fn test_parse_config_defaults() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 50
        });

        let parsed = resource.parse_config(&config).unwrap();
        assert_eq!(parsed.volume_type, "gp3"); // Default
        assert!(!parsed.encrypted); // Default
        assert!(parsed.iops.is_none());
        assert!(parsed.throughput.is_none());
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "gp3"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_availability_zone() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "size": 100
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_missing_size_and_snapshot() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_snapshot_without_size() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "snapshot_id": "snap-12345"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_invalid_volume_type() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "invalid"
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_iops_wrong_volume_type() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "gp2",
            "iops": 3000
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_throughput_wrong_volume_type() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "io1",
            "iops": 3000,
            "throughput": 125
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_multi_attach_wrong_type() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "gp3",
            "multi_attach_enabled": true
        });

        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_multi_attach_io1() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "type": "io1",
            "iops": 3000,
            "multi_attach_enabled": true
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsEbsVolumeResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"availability_zone".to_string()));
        assert!(force_new.contains(&"snapshot_id".to_string()));
        assert!(force_new.contains(&"encrypted".to_string()));
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsEbsVolumeResource::new();
        let config = serde_json::json!({
            "availability_zone": "us-east-1a",
            "size": 100,
            "encrypted": true,
            "kms_key_id": "{{ resources.aws_kms_key.data.arn }}"
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 1);
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_kms_key" && d.resource_name == "data"));
    }

    #[test]
    fn test_state_serialization() {
        let state = EbsVolumeState {
            id: "vol-12345".to_string(),
            arn: "arn:aws:ec2:us-east-1::volume/vol-12345".to_string(),
            availability_zone: "us-east-1a".to_string(),
            size: 100,
            volume_type: "gp3".to_string(),
            iops: Some(3000),
            throughput: Some(125),
            encrypted: true,
            kms_key_id: None,
            snapshot_id: None,
            state: "available".to_string(),
            tags: HashMap::new(),
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "vol-12345");
        assert_eq!(json["size"], 100);
        assert_eq!(json["volume_type"], "gp3");
        assert_eq!(json["encrypted"], true);
    }

    #[test]
    fn test_parse_volume_type() {
        let resource = AwsEbsVolumeResource::new();

        assert_eq!(resource.parse_volume_type("gp2"), VolumeType::Gp2);
        assert_eq!(resource.parse_volume_type("gp3"), VolumeType::Gp3);
        assert_eq!(resource.parse_volume_type("io1"), VolumeType::Io1);
        assert_eq!(resource.parse_volume_type("io2"), VolumeType::Io2);
        assert_eq!(resource.parse_volume_type("st1"), VolumeType::St1);
        assert_eq!(resource.parse_volume_type("sc1"), VolumeType::Sc1);
        assert_eq!(resource.parse_volume_type("standard"), VolumeType::Standard);
        assert_eq!(resource.parse_volume_type("GP3"), VolumeType::Gp3); // Case insensitive
    }
}
