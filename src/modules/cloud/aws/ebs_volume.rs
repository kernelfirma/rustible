//! AWS native module for AWS EBS volumes.
//!
//! This module manages EBS volumes directly from playbooks using the AWS SDK
//! for Rust. Volumes can be selected by `volume_id` or by a stable `Name` tag.

use std::collections::HashMap;
use std::time::Duration;

use aws_config::BehaviorVersion;
use aws_sdk_ec2::types::{Filter, ResourceType, Tag, TagSpecification, VolumeType};
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

#[derive(Debug, Clone)]
struct EbsVolumeConfig {
    volume_id: Option<String>,
    name: Option<String>,
    state: DesiredState,
    availability_zone: Option<String>,
    size: Option<i32>,
    volume_type: Option<String>,
    iops: Option<i32>,
    throughput: Option<i32>,
    encrypted: Option<bool>,
    kms_key_id: Option<String>,
    snapshot_id: Option<String>,
    tags: Option<HashMap<String, String>>,
    region: Option<String>,
}

impl EbsVolumeConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let volume_id = params.get_string("volume_id")?;
        let name = params.get_string("name")?;
        let state = DesiredState::from_optional_str(params.get_string("state")?)?;
        let availability_zone = params.get_string("availability_zone")?;
        let size = params
            .get_i64("size")?
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    ModuleError::InvalidParameter(
                        "size must fit in a signed 32-bit integer".to_string(),
                    )
                })
            })
            .transpose()?;
        let volume_type = get_string_alias(params, "type", "volume_type")?
            .map(|value| value.to_ascii_lowercase());
        let iops = params
            .get_i64("iops")?
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    ModuleError::InvalidParameter(
                        "iops must fit in a signed 32-bit integer".to_string(),
                    )
                })
            })
            .transpose()?;
        let throughput = params
            .get_i64("throughput")?
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    ModuleError::InvalidParameter(
                        "throughput must fit in a signed 32-bit integer".to_string(),
                    )
                })
            })
            .transpose()?;
        let encrypted = params.get_bool("encrypted")?;
        let kms_key_id = params.get_string("kms_key_id")?;
        let snapshot_id = params.get_string("snapshot_id")?;
        let mut tags = parse_optional_string_map_param(params, "tags")?;
        let region = params.get_string("region")?;

        if let Some(name) = &name {
            let tag_map = tags.get_or_insert_with(HashMap::new);
            if let Some(existing) = tag_map.get("Name") {
                if existing != name {
                    return Err(ModuleError::InvalidParameter(
                        "'name' and tags.Name cannot differ".to_string(),
                    ));
                }
            } else {
                tag_map.insert("Name".to_string(), name.clone());
            }
        }

        Ok(Self {
            volume_id,
            name,
            state,
            availability_zone,
            size,
            volume_type,
            iops,
            throughput,
            encrypted,
            kms_key_id,
            snapshot_id,
            tags,
            region,
        })
    }

    fn validate(&self) -> ModuleResult<()> {
        if self.volume_id.is_none() && self.lookup_name().is_none() {
            return Err(ModuleError::MissingParameter(
                "one of volume_id, name, or tags.Name must be provided".to_string(),
            ));
        }

        if let Some(volume_id) = &self.volume_id {
            if !volume_id.starts_with("vol-") {
                return Err(ModuleError::InvalidParameter(
                    "volume_id must look like an AWS EBS volume ID (vol-...)".to_string(),
                ));
            }
        }

        if let Some(availability_zone) = &self.availability_zone {
            if availability_zone.trim().is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "availability_zone cannot be empty".to_string(),
                ));
            }
        }

        if let Some(size) = self.size {
            if !(1..=16384).contains(&size) {
                return Err(ModuleError::InvalidParameter(
                    "size must be between 1 and 16384 GiB".to_string(),
                ));
            }
        }

        if let Some(volume_type) = &self.volume_type {
            validate_volume_type(volume_type)?;
        }

        let effective_type = self
            .volume_type
            .as_deref()
            .unwrap_or("gp3")
            .to_ascii_lowercase();
        validate_performance_settings(&effective_type, self.iops, self.throughput)?;

        if self.kms_key_id.is_some() && self.encrypted == Some(false) {
            return Err(ModuleError::InvalidParameter(
                "kms_key_id requires encrypted=true".to_string(),
            ));
        }

        Ok(())
    }

    fn lookup_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .or_else(|| self.tags.as_ref()?.get("Name").map(String::as_str))
    }

    fn desired_tags(&self) -> HashMap<String, String> {
        self.tags.clone().unwrap_or_default()
    }

    fn desired_info(&self) -> EbsVolumeInfo {
        EbsVolumeInfo {
            volume_id: self
                .volume_id
                .clone()
                .unwrap_or_else(|| "pending".to_string()),
            name: self.lookup_name().map(ToString::to_string),
            availability_zone: self.availability_zone.clone().unwrap_or_default(),
            size: self.size.unwrap_or_default(),
            volume_type: self
                .volume_type
                .clone()
                .unwrap_or_else(|| "gp3".to_string()),
            iops: self.iops,
            throughput: self.throughput,
            encrypted: self.encrypted.unwrap_or(false),
            kms_key_id: self.kms_key_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            state: "planned".to_string(),
            tags: self.desired_tags(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct EbsVolumeInfo {
    volume_id: String,
    name: Option<String>,
    availability_zone: String,
    size: i32,
    volume_type: String,
    iops: Option<i32>,
    throughput: Option<i32>,
    encrypted: bool,
    kms_key_id: Option<String>,
    snapshot_id: Option<String>,
    state: String,
    tags: HashMap<String, String>,
}

fn parse_optional_string_map_param(
    params: &ModuleParams,
    field: &str,
) -> ModuleResult<Option<HashMap<String, String>>> {
    let Some(value) = params.get(field) else {
        return Ok(None);
    };

    let object = value
        .as_object()
        .ok_or_else(|| ModuleError::InvalidParameter(format!("'{}' must be an object", field)))?;

    let mut values = HashMap::new();
    for (key, value) in object {
        let value = match value {
            serde_json::Value::String(string) => string.clone(),
            other => other.to_string().trim_matches('"').to_string(),
        };
        values.insert(key.clone(), value);
    }

    Ok(Some(values))
}

fn get_string_alias(
    params: &ModuleParams,
    primary: &str,
    alias: &str,
) -> ModuleResult<Option<String>> {
    let primary_value = params.get_string(primary)?;
    let alias_value = params.get_string(alias)?;

    match (primary_value, alias_value) {
        (Some(primary_value), Some(alias_value)) if primary_value != alias_value => Err(
            ModuleError::InvalidParameter(format!("'{}' and '{}' cannot differ", primary, alias)),
        ),
        (Some(primary_value), _) => Ok(Some(primary_value)),
        (_, Some(alias_value)) => Ok(Some(alias_value)),
        (None, None) => Ok(None),
    }
}

fn validate_volume_type(value: &str) -> ModuleResult<()> {
    if matches!(
        value,
        "gp2" | "gp3" | "io1" | "io2" | "st1" | "sc1" | "standard"
    ) {
        Ok(())
    } else {
        Err(ModuleError::InvalidParameter(format!(
            "Invalid volume type '{}'. Valid types: gp2, gp3, io1, io2, st1, sc1, standard",
            value
        )))
    }
}

fn validate_performance_settings(
    volume_type: &str,
    iops: Option<i32>,
    throughput: Option<i32>,
) -> ModuleResult<()> {
    if let Some(iops) = iops {
        if !matches!(volume_type, "gp3" | "io1" | "io2") {
            return Err(ModuleError::InvalidParameter(format!(
                "iops can only be specified for gp3, io1, or io2 volumes, not '{}'",
                volume_type
            )));
        }
        if !(100..=256000).contains(&iops) {
            return Err(ModuleError::InvalidParameter(
                "iops must be between 100 and 256000".to_string(),
            ));
        }
    }

    if let Some(throughput) = throughput {
        if volume_type != "gp3" {
            return Err(ModuleError::InvalidParameter(
                "throughput can only be specified for gp3 volumes".to_string(),
            ));
        }
        if !(125..=1000).contains(&throughput) {
            return Err(ModuleError::InvalidParameter(
                "throughput must be between 125 and 1000 MiB/s".to_string(),
            ));
        }
    }

    Ok(())
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

fn build_tags(tags: &HashMap<String, String>) -> Vec<Tag> {
    tags.iter()
        .map(|(key, value)| Tag::builder().key(key).value(value).build())
        .collect()
}

fn parse_volume_info(volume: &aws_sdk_ec2::types::Volume) -> EbsVolumeInfo {
    let mut tags = HashMap::new();
    for tag in volume.tags() {
        if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
            tags.insert(key.to_string(), value.to_string());
        }
    }

    EbsVolumeInfo {
        volume_id: volume.volume_id().unwrap_or_default().to_string(),
        name: tags.get("Name").cloned(),
        availability_zone: volume.availability_zone().unwrap_or_default().to_string(),
        size: volume.size().unwrap_or_default(),
        volume_type: volume
            .volume_type()
            .map(|volume_type| volume_type.as_str().to_string())
            .unwrap_or_else(|| "gp2".to_string()),
        iops: volume.iops(),
        throughput: volume.throughput(),
        encrypted: volume.encrypted().unwrap_or(false),
        kms_key_id: volume.kms_key_id().map(ToString::to_string),
        snapshot_id: volume.snapshot_id().map(ToString::to_string),
        state: volume
            .state()
            .map(|state| state.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        tags,
    }
}

async fn read_volume_by_id(
    client: &Client,
    volume_id: &str,
) -> ModuleResult<Option<EbsVolumeInfo>> {
    match client.describe_volumes().volume_ids(volume_id).send().await {
        Ok(response) => Ok(response.volumes().first().map(parse_volume_info)),
        Err(error) => {
            if error.to_string().contains("InvalidVolume.NotFound") {
                return Ok(None);
            }

            Err(ModuleError::ExecutionFailed(format!(
                "Failed to describe EBS volume '{}': {}",
                volume_id, error
            )))
        }
    }
}

async fn find_volume(
    client: &Client,
    config: &EbsVolumeConfig,
) -> ModuleResult<Option<EbsVolumeInfo>> {
    if let Some(volume_id) = &config.volume_id {
        return read_volume_by_id(client, volume_id).await;
    }

    let Some(name) = config.lookup_name() else {
        return Ok(None);
    };

    let response = client
        .describe_volumes()
        .filters(Filter::builder().name("tag:Name").values(name).build())
        .send()
        .await
        .map_err(|error| {
            ModuleError::ExecutionFailed(format!(
                "Failed to describe EBS volumes by Name tag '{}': {}",
                name, error
            ))
        })?;

    let mut matches = response
        .volumes()
        .iter()
        .map(parse_volume_info)
        .collect::<Vec<_>>();

    if let Some(availability_zone) = &config.availability_zone {
        matches.retain(|volume| &volume.availability_zone == availability_zone);
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(ModuleError::ExecutionFailed(format!(
            "Multiple EBS volumes matched Name tag '{}'; use volume_id to disambiguate",
            name
        ))),
    }
}

fn ensure_create_allowed(config: &EbsVolumeConfig) -> ModuleResult<()> {
    if config.lookup_name().is_none() {
        return Err(ModuleError::InvalidParameter(
            "Creating a new EBS volume requires 'name' or tags.Name for stable identification"
                .to_string(),
        ));
    }

    if config.availability_zone.is_none() {
        return Err(ModuleError::MissingParameter(
            "availability_zone is required when creating a volume".to_string(),
        ));
    }

    if config.size.is_none() && config.snapshot_id.is_none() {
        return Err(ModuleError::MissingParameter(
            "either size or snapshot_id is required when creating a volume".to_string(),
        ));
    }

    Ok(())
}

fn validate_update(
    config: &EbsVolumeConfig,
    current: &EbsVolumeInfo,
    effective_volume_type: &str,
) -> ModuleResult<()> {
    if let Some(availability_zone) = &config.availability_zone {
        if availability_zone != &current.availability_zone {
            return Err(ModuleError::Unsupported(format!(
                "Updating availability_zone is not supported for volume '{}'; recreate the volume to change availability zones",
                current.volume_id
            )));
        }
    }

    if let Some(snapshot_id) = &config.snapshot_id {
        if current.snapshot_id.as_deref() != Some(snapshot_id.as_str()) {
            return Err(ModuleError::Unsupported(format!(
                "Updating snapshot_id is not supported for volume '{}'; recreate the volume to change snapshots",
                current.volume_id
            )));
        }
    }

    if let Some(encrypted) = config.encrypted {
        if encrypted != current.encrypted {
            return Err(ModuleError::Unsupported(format!(
                "Updating encrypted is not supported for volume '{}'; recreate the volume to change encryption",
                current.volume_id
            )));
        }
    }

    if let Some(kms_key_id) = &config.kms_key_id {
        if current.kms_key_id.as_deref() != Some(kms_key_id.as_str()) {
            return Err(ModuleError::Unsupported(format!(
                "Updating kms_key_id is not supported for volume '{}'; recreate the volume to change the KMS key",
                current.volume_id
            )));
        }
    }

    if let Some(size) = config.size {
        if size < current.size {
            return Err(ModuleError::Unsupported(format!(
                "Shrinking EBS volume '{}' from {} GiB to {} GiB is not supported",
                current.volume_id, current.size, size
            )));
        }
    }

    validate_volume_type(effective_volume_type)?;
    validate_performance_settings(effective_volume_type, config.iops, config.throughput)?;

    Ok(())
}

fn with_volume_output(output: ModuleOutput, info: &EbsVolumeInfo) -> ModuleResult<ModuleOutput> {
    Ok(output
        .with_data(
            "volume_id",
            serialize_output_data("volume_id", &info.volume_id)?,
        )
        .with_data("name", serialize_output_data("name", &info.name)?)
        .with_data(
            "availability_zone",
            serialize_output_data("availability_zone", &info.availability_zone)?,
        )
        .with_data("size", serialize_output_data("size", &info.size)?)
        .with_data(
            "volume_type",
            serialize_output_data("volume_type", &info.volume_type)?,
        )
        .with_data("iops", serialize_output_data("iops", &info.iops)?)
        .with_data(
            "throughput",
            serialize_output_data("throughput", &info.throughput)?,
        )
        .with_data(
            "encrypted",
            serialize_output_data("encrypted", &info.encrypted)?,
        )
        .with_data(
            "kms_key_id",
            serialize_output_data("kms_key_id", &info.kms_key_id)?,
        )
        .with_data(
            "snapshot_id",
            serialize_output_data("snapshot_id", &info.snapshot_id)?,
        )
        .with_data("state", serialize_output_data("state", &info.state)?)
        .with_data("tags", serialize_output_data("tags", &info.tags)?))
}

fn effective_volume_type(config: &EbsVolumeConfig, current: Option<&EbsVolumeInfo>) -> String {
    config
        .volume_type
        .clone()
        .or_else(|| current.map(|current| current.volume_type.clone()))
        .unwrap_or_else(|| "gp3".to_string())
}

async fn wait_for_available(client: &Client, volume_id: &str) -> ModuleResult<()> {
    for _ in 0..60 {
        let Some(volume) = read_volume_by_id(client, volume_id).await? else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };

        match volume.state.as_str() {
            "available" => return Ok(()),
            "error" => {
                return Err(ModuleError::ExecutionFailed(format!(
                    "EBS volume '{}' entered the error state during creation",
                    volume_id
                )))
            }
            _ => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }

    Err(ModuleError::ExecutionFailed(format!(
        "Timed out waiting for EBS volume '{}' to become available",
        volume_id
    )))
}

async fn wait_for_modification(client: &Client, volume_id: &str) -> ModuleResult<()> {
    for _ in 0..120 {
        let response = client
            .describe_volumes_modifications()
            .volume_ids(volume_id)
            .send()
            .await
            .map_err(|error| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to poll EBS volume modification '{}': {}",
                    volume_id, error
                ))
            })?;

        for modification in response.volumes_modifications() {
            if let Some(state) = modification.modification_state() {
                match state.as_str() {
                    "completed" | "optimizing" => return Ok(()),
                    "failed" => {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "EBS volume modification failed for '{}'",
                            volume_id
                        )))
                    }
                    _ => {}
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Err(ModuleError::ExecutionFailed(format!(
        "Timed out waiting for EBS volume '{}' modification to complete",
        volume_id
    )))
}

async fn create_volume(client: &Client, config: &EbsVolumeConfig) -> ModuleResult<EbsVolumeInfo> {
    let availability_zone = config.availability_zone.as_deref().ok_or_else(|| {
        ModuleError::MissingParameter(
            "availability_zone is required when creating a volume".to_string(),
        )
    })?;

    let mut request = client
        .create_volume()
        .availability_zone(availability_zone)
        .volume_type(parse_volume_type(&effective_volume_type(config, None)))
        .encrypted(config.encrypted.unwrap_or(false));

    if let Some(size) = config.size {
        request = request.size(size);
    }

    if let Some(snapshot_id) = &config.snapshot_id {
        request = request.snapshot_id(snapshot_id);
    }

    if let Some(iops) = config.iops {
        request = request.iops(iops);
    }

    if let Some(throughput) = config.throughput {
        request = request.throughput(throughput);
    }

    if let Some(kms_key_id) = &config.kms_key_id {
        request = request.kms_key_id(kms_key_id);
    }

    let tags = config.desired_tags();
    if !tags.is_empty() {
        request = request.tag_specifications(
            TagSpecification::builder()
                .resource_type(ResourceType::Volume)
                .set_tags(Some(build_tags(&tags)))
                .build(),
        );
    }

    let response = request.send().await.map_err(|error| {
        ModuleError::ExecutionFailed(format!("Failed to create EBS volume: {}", error))
    })?;

    let volume_id = response
        .volume_id()
        .ok_or_else(|| ModuleError::ExecutionFailed("AWS returned no volume ID".to_string()))?
        .to_string();

    wait_for_available(client, &volume_id).await?;

    read_volume_by_id(client, &volume_id).await?.ok_or_else(|| {
        ModuleError::ExecutionFailed(format!(
            "EBS volume '{}' was created but could not be read back",
            volume_id
        ))
    })
}

async fn sync_tags(
    client: &Client,
    volume_id: &str,
    current: &HashMap<String, String>,
    desired: &HashMap<String, String>,
) -> ModuleResult<()> {
    let removed_keys = current
        .keys()
        .filter(|key| !desired.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();

    if !removed_keys.is_empty() {
        let delete_tags = removed_keys
            .iter()
            .map(|key| Tag::builder().key(key).build())
            .collect::<Vec<_>>();

        client
            .delete_tags()
            .resources(volume_id)
            .set_tags(Some(delete_tags))
            .send()
            .await
            .map_err(|error| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to delete tags from EBS volume '{}': {}",
                    volume_id, error
                ))
            })?;
    }

    if !desired.is_empty() {
        client
            .create_tags()
            .resources(volume_id)
            .set_tags(Some(build_tags(desired)))
            .send()
            .await
            .map_err(|error| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to update tags on EBS volume '{}': {}",
                    volume_id, error
                ))
            })?;
    }

    Ok(())
}

fn parse_volume_type(value: &str) -> VolumeType {
    match value {
        "gp2" => VolumeType::Gp2,
        "gp3" => VolumeType::Gp3,
        "io1" => VolumeType::Io1,
        "io2" => VolumeType::Io2,
        "st1" => VolumeType::St1,
        "sc1" => VolumeType::Sc1,
        "standard" => VolumeType::Standard,
        _ => VolumeType::Gp3,
    }
}

async fn update_volume(
    client: &Client,
    config: &EbsVolumeConfig,
    current: &EbsVolumeInfo,
    effective_volume_type: &str,
) -> ModuleResult<EbsVolumeInfo> {
    let mut modified = false;
    let mut request = client.modify_volume().volume_id(&current.volume_id);

    if let Some(size) = config.size {
        if size != current.size {
            request = request.size(size);
            modified = true;
        }
    }

    if config
        .volume_type
        .as_ref()
        .is_some_and(|volume_type| volume_type != &current.volume_type)
    {
        request = request.volume_type(parse_volume_type(effective_volume_type));
        modified = true;
    }

    if let Some(iops) = config.iops {
        if current.iops != Some(iops) {
            request = request.iops(iops);
            modified = true;
        }
    }

    if let Some(throughput) = config.throughput {
        if current.throughput != Some(throughput) {
            request = request.throughput(throughput);
            modified = true;
        }
    }

    if modified {
        request.send().await.map_err(|error| {
            ModuleError::ExecutionFailed(format!(
                "Failed to modify EBS volume '{}': {}",
                current.volume_id, error
            ))
        })?;
        wait_for_modification(client, &current.volume_id).await?;
    }

    if let Some(tags) = &config.tags {
        sync_tags(client, &current.volume_id, &current.tags, tags).await?;
    }

    read_volume_by_id(client, &current.volume_id)
        .await?
        .ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Updated EBS volume '{}' could not be read back",
                current.volume_id
            ))
        })
}

#[derive(Debug, Default)]
pub struct AwsEbsVolumeModule;

impl AwsEbsVolumeModule {
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = EbsVolumeConfig::from_params(params)?;
        config.validate()?;

        let client = create_ec2_client(config.region.as_deref()).await?;
        let current = find_volume(&client, &config).await?;

        match config.state {
            DesiredState::Absent => {
                let Some(current) = current else {
                    return Ok(ModuleOutput::ok("EBS volume is already absent"));
                };

                if context.check_mode {
                    return with_volume_output(
                        ModuleOutput::changed(format!(
                            "Would delete EBS volume '{}'",
                            current.volume_id
                        ))
                        .with_diff(Diff::new(
                            format!("EBS volume '{}' exists", current.volume_id),
                            "volume will be deleted".to_string(),
                        )),
                        &current,
                    );
                }

                client
                    .delete_volume()
                    .volume_id(&current.volume_id)
                    .send()
                    .await
                    .map_err(|error| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to delete EBS volume '{}': {}",
                            current.volume_id, error
                        ))
                    })?;

                with_volume_output(
                    ModuleOutput::changed(format!("Deleted EBS volume '{}'", current.volume_id)),
                    &current,
                )
            }
            DesiredState::Present => {
                let Some(current) = current else {
                    ensure_create_allowed(&config)?;
                    let desired = config.desired_info();

                    if context.check_mode {
                        return with_volume_output(
                            ModuleOutput::changed(format!(
                                "Would create EBS volume '{}'",
                                desired
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| "unnamed".to_string())
                            ))
                            .with_diff(Diff::new(
                                "volume does not exist",
                                "volume will be created".to_string(),
                            )),
                            &desired,
                        );
                    }

                    let created = create_volume(&client, &config).await?;
                    return with_volume_output(
                        ModuleOutput::changed(format!(
                            "Created EBS volume '{}'",
                            created.volume_id
                        )),
                        &created,
                    );
                };

                let desired_volume_type = effective_volume_type(&config, Some(&current));
                validate_update(&config, &current, &desired_volume_type)?;

                let mut changed_fields = Vec::new();

                if let Some(size) = config.size {
                    if size != current.size {
                        changed_fields.push("size");
                    }
                }

                if config
                    .volume_type
                    .as_ref()
                    .is_some_and(|volume_type| volume_type != &current.volume_type)
                {
                    changed_fields.push("volume_type");
                }

                if config.iops.is_some_and(|iops| current.iops != Some(iops)) {
                    changed_fields.push("iops");
                }

                if config
                    .throughput
                    .is_some_and(|throughput| current.throughput != Some(throughput))
                {
                    changed_fields.push("throughput");
                }

                if let Some(tags) = &config.tags {
                    if tags != &current.tags {
                        changed_fields.push("tags");
                    }
                }

                if changed_fields.is_empty() {
                    return with_volume_output(
                        ModuleOutput::ok(format!(
                            "EBS volume '{}' is up to date",
                            current.volume_id
                        )),
                        &current,
                    );
                }

                if context.check_mode {
                    return with_volume_output(
                        ModuleOutput::changed(format!(
                            "Would update EBS volume '{}'",
                            current.volume_id
                        ))
                        .with_diff(Diff::new(
                            "current EBS volume configuration",
                            format!("updated fields: {}", changed_fields.join(", ")),
                        )),
                        &current,
                    );
                }

                let updated =
                    update_volume(&client, &config, &current, &desired_volume_type).await?;
                with_volume_output(
                    ModuleOutput::changed(format!("Updated EBS volume '{}'", updated.volume_id))
                        .with_diff(Diff::new(
                            "current EBS volume configuration",
                            format!("updated fields: {}", changed_fields.join(", ")),
                        )),
                    &updated,
                )
            }
        }
    }
}

impl Module for AwsEbsVolumeModule {
    fn name(&self) -> &'static str {
        "aws_ebs_volume"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete AWS EBS volumes by volume ID or Name tag"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("state", serde_json::json!("present"));
        params.insert("type", serde_json::json!("gp3"));
        params.insert("tags", serde_json::json!({}));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        EbsVolumeConfig::from_params(params)?.validate()
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
        params.insert("name".to_string(), serde_json::json!("data-volume"));
        params.insert(
            "availability_zone".to_string(),
            serde_json::json!("us-east-1a"),
        );
        params.insert("size".to_string(), serde_json::json!(100));
        params
    }

    #[test]
    fn test_name_populates_name_tag() {
        let config = EbsVolumeConfig::from_params(&base_params()).unwrap();
        assert_eq!(config.lookup_name(), Some("data-volume"));
        assert_eq!(
            config.tags.unwrap().get("Name"),
            Some(&"data-volume".to_string())
        );
    }

    #[test]
    fn test_validate_rejects_throughput_for_non_gp3() {
        let mut params = base_params();
        params.insert("type".to_string(), serde_json::json!("io2"));
        params.insert("throughput".to_string(), serde_json::json!(250));

        assert!(EbsVolumeConfig::from_params(&params)
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn test_validate_rejects_conflicting_type_aliases() {
        let mut params = base_params();
        params.insert("type".to_string(), serde_json::json!("gp3"));
        params.insert("volume_type".to_string(), serde_json::json!("io2"));

        assert!(EbsVolumeConfig::from_params(&params).is_err());
    }
}
