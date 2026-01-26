//! AWS S3 Bucket Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsS3BucketResource` which implements the `Resource` trait
//! for managing AWS S3 buckets declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_s3_bucket:
//!     my_bucket:
//!       bucket: my-unique-bucket-name
//!       acl: private
//!       versioning:
//!         enabled: true
//!       server_side_encryption_configuration:
//!         rule:
//!           apply_server_side_encryption_by_default:
//!             sse_algorithm: AES256
//!       tags:
//!         Name: my-bucket
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::types::{
    BucketLocationConstraint, BucketVersioningStatus, CreateBucketConfiguration,
    ServerSideEncryption, ServerSideEncryptionByDefault, ServerSideEncryptionConfiguration,
    ServerSideEncryptionRule, Tag as S3Tag, Tagging,
};
use aws_sdk_s3::Client;
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

/// Versioning configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VersioningConfig {
    /// Whether versioning is enabled
    #[serde(default)]
    pub enabled: bool,
    /// MFA delete enabled (requires MFA to delete versions)
    #[serde(default)]
    pub mfa_delete: bool,
}

/// Server-side encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SseConfig {
    /// SSE algorithm: AES256, aws:kms, aws:kms:dsse
    pub sse_algorithm: String,
    /// KMS key ID (required for aws:kms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_master_key_id: Option<String>,
    /// Whether to use bucket key for SSE-KMS
    #[serde(default)]
    pub bucket_key_enabled: bool,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoggingConfig {
    /// Target bucket for access logs
    pub target_bucket: String,
    /// Prefix for log objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_prefix: Option<String>,
}

/// Website configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebsiteConfig {
    /// Index document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_document: Option<String>,
    /// Error document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_document: Option<String>,
    /// Redirect all requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_all_requests_to: Option<String>,
}

/// S3 bucket configuration parsed from provisioning config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketConfig {
    /// Bucket name (required, globally unique)
    pub bucket: String,
    /// Bucket prefix for generated names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_prefix: Option<String>,
    /// Canned ACL: private, public-read, public-read-write, authenticated-read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acl: Option<String>,
    /// Force destroy (delete all objects on bucket deletion)
    #[serde(default)]
    pub force_destroy: bool,
    /// Object lock enabled
    #[serde(default)]
    pub object_lock_enabled: bool,
    /// Versioning configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versioning: Option<VersioningConfig>,
    /// Server-side encryption configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_side_encryption: Option<SseConfig>,
    /// Logging configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingConfig>,
    /// Website configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<WebsiteConfig>,
    /// Resource tags
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

impl S3BucketConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!("Invalid S3 bucket configuration: {}", e))
        })
    }
}

/// Computed attributes returned after S3 bucket operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct S3BucketState {
    /// Bucket name (also the ID)
    pub id: String,
    /// Bucket ARN
    pub arn: String,
    /// Bucket domain name
    pub bucket_domain_name: String,
    /// Bucket regional domain name
    pub bucket_regional_domain_name: String,
    /// Hosted zone ID (for Route53 alias)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_zone_id: Option<String>,
    /// Region
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Website endpoint (if website enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_endpoint: Option<String>,
    /// Website domain (if website enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_domain: Option<String>,
    /// Versioning enabled
    #[serde(default)]
    pub versioning_enabled: bool,
    /// SSE algorithm
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sse_algorithm: Option<String>,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS S3 Bucket Resource
// ============================================================================

/// AWS S3 Bucket Resource implementation
#[derive(Debug, Clone)]
pub struct AwsS3BucketResource;

impl AwsS3BucketResource {
    /// Create a new AWS S3 Bucket resource
    pub fn new() -> Self {
        Self
    }

    /// Create AWS S3 client from provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_s3::config::Region::new(region.clone()))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Build bucket ARN
    fn build_arn(&self, bucket: &str) -> String {
        format!("arn:aws:s3:::{}", bucket)
    }

    /// Build bucket domain name
    fn build_domain_name(&self, bucket: &str) -> String {
        format!("{}.s3.amazonaws.com", bucket)
    }

    /// Build bucket regional domain name
    fn build_regional_domain_name(&self, bucket: &str, region: &str) -> String {
        format!("{}.s3.{}.amazonaws.com", bucket, region)
    }

    /// Get hosted zone ID for S3 website endpoints
    fn get_hosted_zone_id(&self, region: &str) -> &'static str {
        // S3 hosted zone IDs for website endpoints
        match region {
            "us-east-1" => "Z3AQBSTGFYJSTF",
            "us-east-2" => "Z2O1EMRO9K5GLX",
            "us-west-1" => "Z2F56UZL2M1ACD",
            "us-west-2" => "Z3BJ6K6RIION7M",
            "eu-west-1" => "Z1BKCTXD74EZPE",
            "eu-west-2" => "Z3GKZC51ZF0DB4",
            "eu-west-3" => "Z3R1K369G5AVDG",
            "eu-central-1" => "Z21DNDUVLTQW6Q",
            "eu-north-1" => "Z3BAZG2TWCNX0D",
            "ap-northeast-1" => "Z2M4EHUR26P7ZW",
            "ap-northeast-2" => "Z3W03O7B5YMIYP",
            "ap-southeast-1" => "Z3O0J2DXBE1FTB",
            "ap-southeast-2" => "Z1WCIBER3XQSRI",
            "ap-south-1" => "Z11RGJOFQNVJUP",
            "sa-east-1" => "Z7KQH4QJS55SO",
            "ca-central-1" => "Z1QDHH18159H29",
            _ => "Z3AQBSTGFYJSTF", // Default to us-east-1
        }
    }

    /// Check if bucket exists
    async fn bucket_exists(&self, client: &Client, bucket: &str) -> ProvisioningResult<bool> {
        match client.head_bucket().bucket(bucket).send().await {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("NoSuchBucket") || err_str.contains("NotFound") {
                    Ok(false)
                } else if err_str.contains("403") || err_str.contains("Forbidden") {
                    // Bucket exists but we don't have access
                    Err(ProvisioningError::CloudApiError(format!(
                        "Bucket {} exists but access denied",
                        bucket
                    )))
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to check bucket: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Get bucket state
    async fn get_bucket_state(
        &self,
        client: &Client,
        bucket: &str,
        region: &str,
    ) -> ProvisioningResult<Option<S3BucketState>> {
        if !self.bucket_exists(client, bucket).await? {
            return Ok(None);
        }

        let mut state = S3BucketState {
            id: bucket.to_string(),
            arn: self.build_arn(bucket),
            bucket_domain_name: self.build_domain_name(bucket),
            bucket_regional_domain_name: self.build_regional_domain_name(bucket, region),
            hosted_zone_id: Some(self.get_hosted_zone_id(region).to_string()),
            region: Some(region.to_string()),
            ..Default::default()
        };

        // Get versioning status
        if let Ok(versioning) = client.get_bucket_versioning().bucket(bucket).send().await {
            state.versioning_enabled = versioning.status() == Some(&BucketVersioningStatus::Enabled);
        }

        // Get encryption configuration
        if let Ok(encryption) = client.get_bucket_encryption().bucket(bucket).send().await {
            if let Some(config) = encryption.server_side_encryption_configuration() {
                if let Some(rule) = config.rules().first() {
                    if let Some(default) = rule.apply_server_side_encryption_by_default() {
                        state.sse_algorithm = Some(default.sse_algorithm().as_str().to_string());
                    }
                }
            }
        }

        // Get tags
        if let Ok(tagging) = client.get_bucket_tagging().bucket(bucket).send().await {
            for tag in tagging.tag_set() {
                state.tags.insert(tag.key().to_string(), tag.value().to_string());
            }
        }

        // Get website configuration
        if let Ok(website) = client.get_bucket_website().bucket(bucket).send().await {
            let website_endpoint = format!("{}.s3-website-{}.amazonaws.com", bucket, region);
            state.website_endpoint = Some(website_endpoint.clone());
            state.website_domain = Some(format!("s3-website-{}.amazonaws.com", region));
        }

        Ok(Some(state))
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

    /// Build tags for S3 API
    fn build_tags(&self, tags: &HashMap<String, String>) -> Vec<S3Tag> {
        tags.iter()
            .map(|(k, v)| S3Tag::builder().key(k).value(v).build().unwrap())
            .collect()
    }

    /// Delete all objects in bucket (for force_destroy)
    async fn empty_bucket(&self, client: &Client, bucket: &str) -> ProvisioningResult<()> {
        debug!("Emptying bucket {} before deletion", bucket);

        // List and delete all objects
        let mut continuation_token: Option<String> = None;

        loop {
            let mut list_req = client.list_objects_v2().bucket(bucket);
            if let Some(ref token) = continuation_token {
                list_req = list_req.continuation_token(token);
            }

            let output = list_req.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to list objects: {}", e))
            })?;

            let objects: Vec<_> = output
                .contents()
                .iter()
                .filter_map(|obj| obj.key().map(|k| k.to_string()))
                .collect();

            if !objects.is_empty() {
                // Delete objects in batches
                for chunk in objects.chunks(1000) {
                    let delete_objects: Vec<_> = chunk
                        .iter()
                        .map(|key| {
                            aws_sdk_s3::types::ObjectIdentifier::builder()
                                .key(key)
                                .build()
                                .unwrap()
                        })
                        .collect();

                    let delete = aws_sdk_s3::types::Delete::builder()
                        .set_objects(Some(delete_objects))
                        .build()
                        .unwrap();

                    client
                        .delete_objects()
                        .bucket(bucket)
                        .delete(delete)
                        .send()
                        .await
                        .map_err(|e| {
                            ProvisioningError::CloudApiError(format!(
                                "Failed to delete objects: {}",
                                e
                            ))
                        })?;
                }
            }

            if output.is_truncated() == Some(true) {
                continuation_token = output.next_continuation_token().map(|s| s.to_string());
            } else {
                break;
            }
        }

        // Also delete all object versions (for versioned buckets)
        let mut key_marker: Option<String> = None;
        let mut version_id_marker: Option<String> = None;

        loop {
            let mut list_req = client.list_object_versions().bucket(bucket);
            if let Some(ref marker) = key_marker {
                list_req = list_req.key_marker(marker);
            }
            if let Some(ref vid_marker) = version_id_marker {
                list_req = list_req.version_id_marker(vid_marker);
            }

            let output = list_req.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to list object versions: {}", e))
            })?;

            let mut delete_objects = Vec::new();

            // Add versions
            for version in output.versions() {
                if let Some(key) = version.key() {
                    let mut builder = aws_sdk_s3::types::ObjectIdentifier::builder().key(key);
                    if let Some(vid) = version.version_id() {
                        builder = builder.version_id(vid);
                    }
                    delete_objects.push(builder.build().unwrap());
                }
            }

            // Add delete markers
            for marker in output.delete_markers() {
                if let Some(key) = marker.key() {
                    let mut builder = aws_sdk_s3::types::ObjectIdentifier::builder().key(key);
                    if let Some(vid) = marker.version_id() {
                        builder = builder.version_id(vid);
                    }
                    delete_objects.push(builder.build().unwrap());
                }
            }

            if !delete_objects.is_empty() {
                for chunk in delete_objects.chunks(1000) {
                    let delete = aws_sdk_s3::types::Delete::builder()
                        .set_objects(Some(chunk.to_vec()))
                        .build()
                        .unwrap();

                    client
                        .delete_objects()
                        .bucket(bucket)
                        .delete(delete)
                        .send()
                        .await
                        .map_err(|e| {
                            ProvisioningError::CloudApiError(format!(
                                "Failed to delete object versions: {}",
                                e
                            ))
                        })?;
                }
            }

            if output.is_truncated() == Some(true) {
                key_marker = output.next_key_marker().map(|s| s.to_string());
                version_id_marker = output.next_version_id_marker().map(|s| s.to_string());
            } else {
                break;
            }
        }

        Ok(())
    }
}

impl Default for AwsS3BucketResource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Resource for AwsS3BucketResource {
    fn resource_type(&self) -> &str {
        "aws_s3_bucket"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_s3_bucket".to_string(),
            description: "Provides an S3 bucket resource. Manages an S3 bucket.".to_string(),
            required_args: vec![SchemaField {
                name: "bucket".to_string(),
                field_type: FieldType::String,
                description: "The name of the bucket".to_string(),
                default: None,
                constraints: vec![
                    FieldConstraint::MinLength { min: 3 },
                    FieldConstraint::MaxLength { max: 63 },
                    FieldConstraint::Pattern {
                        regex: r"^[a-z0-9][a-z0-9.-]*[a-z0-9]$".to_string(),
                    },
                ],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "acl".to_string(),
                    field_type: FieldType::String,
                    description: "The canned ACL to apply".to_string(),
                    default: Some(Value::String("private".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "private".to_string(),
                            "public-read".to_string(),
                            "public-read-write".to_string(),
                            "authenticated-read".to_string(),
                            "aws-exec-read".to_string(),
                            "bucket-owner-read".to_string(),
                            "bucket-owner-full-control".to_string(),
                            "log-delivery-write".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "force_destroy".to_string(),
                    field_type: FieldType::Boolean,
                    description: "A boolean that indicates all objects should be deleted from the bucket when the bucket is destroyed".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "object_lock_enabled".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Indicates whether this bucket has an Object Lock configuration enabled".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "versioning".to_string(),
                    field_type: FieldType::Object(vec![
                        SchemaField {
                            name: "enabled".to_string(),
                            field_type: FieldType::Boolean,
                            description: "Enable versioning".to_string(),
                            default: Some(Value::Bool(false)),
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]),
                    description: "Versioning configuration".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "server_side_encryption".to_string(),
                    field_type: FieldType::Object(vec![
                        SchemaField {
                            name: "sse_algorithm".to_string(),
                            field_type: FieldType::String,
                            description: "SSE algorithm to use".to_string(),
                            default: Some(Value::String("AES256".to_string())),
                            constraints: vec![FieldConstraint::Enum {
                                values: vec![
                                    "AES256".to_string(),
                                    "aws:kms".to_string(),
                                    "aws:kms:dsse".to_string(),
                                ],
                            }],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "kms_master_key_id".to_string(),
                            field_type: FieldType::String,
                            description: "KMS master key ID".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]),
                    description: "Server-side encryption configuration".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "A map of tags to assign to the bucket".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the bucket".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the bucket".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "bucket_domain_name".to_string(),
                    field_type: FieldType::String,
                    description: "The bucket domain name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "bucket_regional_domain_name".to_string(),
                    field_type: FieldType::String,
                    description: "The bucket region-specific domain name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "hosted_zone_id".to_string(),
                    field_type: FieldType::String,
                    description: "The Route 53 Hosted Zone ID for this bucket's region".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "region".to_string(),
                    field_type: FieldType::String,
                    description: "The AWS region this bucket resides in".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "website_endpoint".to_string(),
                    field_type: FieldType::String,
                    description: "The website endpoint".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "bucket".to_string(),
                "object_lock_enabled".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
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
        let region = ctx.region.as_deref().unwrap_or("us-east-1");

        match self.get_bucket_state(&client, id, region).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize bucket attributes: {}",
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

                for (key, des_val) in desired_obj {
                    let cur_val = current_obj.get(key);

                    match cur_val {
                        Some(cv) if cv != des_val => {
                            diff.modifications
                                .insert(key.clone(), (cv.clone(), des_val.clone()));

                            if key == "bucket" || key == "object_lock_enabled" {
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
                    "id", "arn", "bucket_domain_name", "bucket_regional_domain_name",
                    "hosted_zone_id", "region", "website_endpoint", "website_domain",
                ];
                for key in current_obj.keys() {
                    if !desired_obj.contains_key(key) && !key.starts_with('_') && !computed_fields.contains(&key.as_str()) {
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
        let bucket_config = S3BucketConfig::from_value(config)?;
        let client = self.create_client(ctx).await?;
        let region = ctx.region.as_deref().unwrap_or("us-east-1");

        info!("Creating S3 bucket: {}", bucket_config.bucket);

        // Create the bucket
        let mut create_bucket = client.create_bucket().bucket(&bucket_config.bucket);

        // Set location constraint (required for non-us-east-1 regions)
        if region != "us-east-1" {
            let location_constraint = BucketLocationConstraint::from(region);
            let create_config = CreateBucketConfiguration::builder()
                .location_constraint(location_constraint)
                .build();
            create_bucket = create_bucket.create_bucket_configuration(create_config);
        }

        // Set ACL if specified
        if let Some(ref acl) = bucket_config.acl {
            create_bucket = create_bucket.acl(aws_sdk_s3::types::BucketCannedAcl::from(acl.as_str()));
        }

        // Set object lock if enabled
        if bucket_config.object_lock_enabled {
            create_bucket = create_bucket.object_lock_enabled_for_bucket(true);
        }

        create_bucket.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create bucket: {}", e))
        })?;

        // Configure versioning if specified
        if let Some(ref versioning) = bucket_config.versioning {
            if versioning.enabled {
                client
                    .put_bucket_versioning()
                    .bucket(&bucket_config.bucket)
                    .versioning_configuration(
                        aws_sdk_s3::types::VersioningConfiguration::builder()
                            .status(BucketVersioningStatus::Enabled)
                            .build(),
                    )
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to enable versioning: {}",
                            e
                        ))
                    })?;
            }
        }

        // Configure server-side encryption if specified
        if let Some(ref sse) = bucket_config.server_side_encryption {
            let algorithm = match sse.sse_algorithm.as_str() {
                "AES256" => ServerSideEncryption::Aes256,
                "aws:kms" => ServerSideEncryption::AwsKms,
                "aws:kms:dsse" => ServerSideEncryption::AwsKmsDsse,
                _ => ServerSideEncryption::Aes256,
            };

            let mut default_builder = ServerSideEncryptionByDefault::builder()
                .sse_algorithm(algorithm);

            if let Some(ref kms_key) = sse.kms_master_key_id {
                default_builder = default_builder.kms_master_key_id(kms_key);
            }

            let rule = ServerSideEncryptionRule::builder()
                .apply_server_side_encryption_by_default(default_builder.build().map_err(|e| {
                    ProvisioningError::ValidationError(format!("Invalid SSE config: {}", e))
                })?)
                .bucket_key_enabled(sse.bucket_key_enabled)
                .build();

            let sse_config = ServerSideEncryptionConfiguration::builder()
                .rules(rule)
                .build()
                .map_err(|e| {
                    ProvisioningError::ValidationError(format!("Invalid SSE configuration: {}", e))
                })?;

            client
                .put_bucket_encryption()
                .bucket(&bucket_config.bucket)
                .server_side_encryption_configuration(sse_config)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to set encryption: {}", e))
                })?;
        }

        // Configure logging if specified
        if let Some(ref logging) = bucket_config.logging {
            let mut logging_config = aws_sdk_s3::types::LoggingEnabled::builder()
                .target_bucket(&logging.target_bucket);

            if let Some(ref prefix) = logging.target_prefix {
                logging_config = logging_config.target_prefix(prefix);
            }

            client
                .put_bucket_logging()
                .bucket(&bucket_config.bucket)
                .bucket_logging_status(
                    aws_sdk_s3::types::BucketLoggingStatus::builder()
                        .logging_enabled(logging_config.build().unwrap())
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to set logging: {}", e))
                })?;
        }

        // Configure website if specified
        if let Some(ref website) = bucket_config.website {
            let mut website_config = aws_sdk_s3::types::WebsiteConfiguration::builder();

            if let Some(ref index) = website.index_document {
                website_config = website_config.index_document(
                    aws_sdk_s3::types::IndexDocument::builder()
                        .suffix(index)
                        .build()
                        .unwrap(),
                );
            }

            if let Some(ref error) = website.error_document {
                website_config = website_config.error_document(
                    aws_sdk_s3::types::ErrorDocument::builder()
                        .key(error)
                        .build()
                        .unwrap(),
                );
            }

            if let Some(ref redirect) = website.redirect_all_requests_to {
                website_config = website_config.redirect_all_requests_to(
                    aws_sdk_s3::types::RedirectAllRequestsTo::builder()
                        .host_name(redirect)
                        .build()
                        .unwrap(),
                );
            }

            client
                .put_bucket_website()
                .bucket(&bucket_config.bucket)
                .website_configuration(website_config.build())
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to set website config: {}", e))
                })?;
        }

        // Set tags
        if !bucket_config.tags.is_empty() || !ctx.default_tags.is_empty() {
            let mut all_tags = ctx.default_tags.clone();
            all_tags.extend(bucket_config.tags.clone());

            let tags = self.build_tags(&all_tags);
            let tagging = Tagging::builder().set_tag_set(Some(tags)).build().unwrap();

            client
                .put_bucket_tagging()
                .bucket(&bucket_config.bucket)
                .tagging(tagging)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to set tags: {}", e))
                })?;
        }

        // Get final state
        let state = self
            .get_bucket_state(&client, &bucket_config.bucket, region)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Bucket not found after creation".to_string())
            })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        info!("Created S3 bucket: {}", bucket_config.bucket);

        Ok(ResourceResult::success(&bucket_config.bucket, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("bucket_domain_name", Value::String(state.bucket_domain_name.clone()))
            .with_output("bucket_regional_domain_name", Value::String(state.bucket_regional_domain_name.clone())))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let bucket_config = S3BucketConfig::from_value(new)?;
        let client = self.create_client(ctx).await?;
        let region = ctx.region.as_deref().unwrap_or("us-east-1");

        info!("Updating S3 bucket: {}", id);

        // Update versioning
        if let Some(ref versioning) = bucket_config.versioning {
            let status = if versioning.enabled {
                BucketVersioningStatus::Enabled
            } else {
                BucketVersioningStatus::Suspended
            };

            client
                .put_bucket_versioning()
                .bucket(id)
                .versioning_configuration(
                    aws_sdk_s3::types::VersioningConfiguration::builder()
                        .status(status)
                        .build(),
                )
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update versioning: {}", e))
                })?;
        }

        // Update encryption
        if let Some(ref sse) = bucket_config.server_side_encryption {
            let algorithm = match sse.sse_algorithm.as_str() {
                "AES256" => ServerSideEncryption::Aes256,
                "aws:kms" => ServerSideEncryption::AwsKms,
                "aws:kms:dsse" => ServerSideEncryption::AwsKmsDsse,
                _ => ServerSideEncryption::Aes256,
            };

            let mut default_builder = ServerSideEncryptionByDefault::builder()
                .sse_algorithm(algorithm);

            if let Some(ref kms_key) = sse.kms_master_key_id {
                default_builder = default_builder.kms_master_key_id(kms_key);
            }

            let rule = ServerSideEncryptionRule::builder()
                .apply_server_side_encryption_by_default(default_builder.build().map_err(|e| {
                    ProvisioningError::ValidationError(format!("Invalid SSE config: {}", e))
                })?)
                .bucket_key_enabled(sse.bucket_key_enabled)
                .build();

            let sse_config = ServerSideEncryptionConfiguration::builder()
                .rules(rule)
                .build()
                .map_err(|e| {
                    ProvisioningError::ValidationError(format!("Invalid SSE configuration: {}", e))
                })?;

            client
                .put_bucket_encryption()
                .bucket(id)
                .server_side_encryption_configuration(sse_config)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update encryption: {}", e))
                })?;
        }

        // Update tags
        let mut all_tags = ctx.default_tags.clone();
        all_tags.extend(bucket_config.tags.clone());

        if !all_tags.is_empty() {
            let tags = self.build_tags(&all_tags);
            let tagging = Tagging::builder().set_tag_set(Some(tags)).build().unwrap();

            client
                .put_bucket_tagging()
                .bucket(id)
                .tagging(tagging)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                })?;
        }

        // Get final state
        let state = self
            .get_bucket_state(&client, id, region)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Bucket not found after update".to_string())
            })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Check if bucket exists
        if !self.bucket_exists(&client, id).await? {
            return Ok(ResourceResult::success(id, Value::Null));
        }

        info!("Deleting S3 bucket: {}", id);

        // Empty the bucket first (force_destroy behavior)
        // Note: In production, you'd check the config for force_destroy
        self.empty_bucket(&client, id).await?;

        // Delete the bucket
        client
            .delete_bucket()
            .bucket(id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete bucket: {}", e))
            })?;

        info!("Deleted S3 bucket: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;
        let region = ctx.region.as_deref().unwrap_or("us-east-1");

        let state = self.get_bucket_state(&client, id, region).await?.ok_or_else(|| {
            ProvisioningError::ImportError {
                resource_type: "aws_s3_bucket".to_string(),
                resource_id: id.to_string(),
                message: "Bucket not found".to_string(),
            }
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(obj) = config.as_object() {
            // Check logging target_bucket for references
            if let Some(logging) = obj.get("logging") {
                if let Some(target) = logging.get("target_bucket") {
                    deps.extend(self.extract_references(target));
                }
            }

            // Check server_side_encryption kms_master_key_id for references
            if let Some(sse) = obj.get("server_side_encryption") {
                if let Some(kms) = sse.get("kms_master_key_id") {
                    deps.extend(self.extract_references(kms));
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "bucket".to_string(),
            "object_lock_enabled".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate required fields
        if !obj.contains_key("bucket") {
            return Err(ProvisioningError::ValidationError(
                "bucket is required".to_string(),
            ));
        }

        // Validate bucket name
        if let Some(bucket) = obj.get("bucket").and_then(|v| v.as_str()) {
            if bucket.len() < 3 || bucket.len() > 63 {
                return Err(ProvisioningError::ValidationError(
                    "bucket name must be between 3 and 63 characters".to_string(),
                ));
            }

            // Check valid characters
            if !bucket.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-') {
                return Err(ProvisioningError::ValidationError(
                    "bucket name can only contain lowercase letters, numbers, hyphens, and periods".to_string(),
                ));
            }

            // Must start with letter or number
            if !bucket.chars().next().map(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).unwrap_or(false) {
                return Err(ProvisioningError::ValidationError(
                    "bucket name must start with a lowercase letter or number".to_string(),
                ));
            }

            // Must not look like an IP address
            if bucket.split('.').all(|part| part.parse::<u8>().is_ok()) {
                return Err(ProvisioningError::ValidationError(
                    "bucket name must not be formatted as an IP address".to_string(),
                ));
            }
        }

        // Validate ACL if specified
        if let Some(acl) = obj.get("acl").and_then(|v| v.as_str()) {
            let valid_acls = [
                "private", "public-read", "public-read-write", "authenticated-read",
                "aws-exec-read", "bucket-owner-read", "bucket-owner-full-control", "log-delivery-write",
            ];
            if !valid_acls.contains(&acl) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid ACL: {}. Must be one of: {}",
                    acl,
                    valid_acls.join(", ")
                )));
            }
        }

        // Validate SSE algorithm if specified
        if let Some(sse) = obj.get("server_side_encryption") {
            if let Some(algorithm) = sse.get("sse_algorithm").and_then(|v| v.as_str()) {
                let valid_algorithms = ["AES256", "aws:kms", "aws:kms:dsse"];
                if !valid_algorithms.contains(&algorithm) {
                    return Err(ProvisioningError::ValidationError(format!(
                        "Invalid SSE algorithm: {}. Must be one of: {}",
                        algorithm,
                        valid_algorithms.join(", ")
                    )));
                }

                // KMS key required for aws:kms
                if (algorithm == "aws:kms" || algorithm == "aws:kms:dsse")
                    && !sse.get("kms_master_key_id").map(|v| v.is_string()).unwrap_or(false)
                {
                    // KMS key is optional - AWS will use default key if not specified
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
        let resource = AwsS3BucketResource::new();
        assert_eq!(resource.resource_type(), "aws_s3_bucket");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsS3BucketResource::new();
        let forces = resource.forces_replacement();

        assert!(forces.contains(&"bucket".to_string()));
        assert!(forces.contains(&"object_lock_enabled".to_string()));
    }

    #[test]
    fn test_schema_has_required_fields() {
        let resource = AwsS3BucketResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_s3_bucket");
        assert!(!schema.required_args.is_empty());

        let has_bucket = schema.required_args.iter().any(|f| f.name == "bucket");
        assert!(has_bucket);
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my-valid-bucket-name"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_bucket() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "acl": "private"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_bucket_too_short() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "ab"  // Too short
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_bucket_invalid_chars() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my_bucket_name"  // Underscores not allowed
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_bucket_starts_with_number() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "123-bucket"  // Starting with number is valid
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_bucket_ip_address() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "192.168.1.1"  // IP addresses not allowed
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_acl() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my-bucket",
            "acl": "invalid-acl"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_acl() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my-bucket",
            "acl": "public-read"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_invalid_sse_algorithm() {
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my-bucket",
            "server_side_encryption": {
                "sse_algorithm": "invalid-algo"
            }
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_bucket_config_parsing() {
        let config = json!({
            "bucket": "my-bucket",
            "acl": "private",
            "force_destroy": true,
            "versioning": {
                "enabled": true
            },
            "server_side_encryption": {
                "sse_algorithm": "AES256"
            },
            "tags": {
                "Name": "my-bucket",
                "Environment": "test"
            }
        });

        let bucket_config = S3BucketConfig::from_value(&config).unwrap();

        assert_eq!(bucket_config.bucket, "my-bucket");
        assert_eq!(bucket_config.acl, Some("private".to_string()));
        assert!(bucket_config.force_destroy);
        assert!(bucket_config.versioning.unwrap().enabled);
        assert_eq!(bucket_config.server_side_encryption.unwrap().sse_algorithm, "AES256");
        assert_eq!(bucket_config.tags.get("Name"), Some(&"my-bucket".to_string()));
    }

    #[test]
    fn test_bucket_config_defaults() {
        let config = json!({
            "bucket": "my-bucket"
        });

        let bucket_config = S3BucketConfig::from_value(&config).unwrap();

        assert!(!bucket_config.force_destroy);
        assert!(!bucket_config.object_lock_enabled);
        assert!(bucket_config.versioning.is_none());
        assert!(bucket_config.tags.is_empty());
    }

    #[test]
    fn test_build_arn() {
        let resource = AwsS3BucketResource::new();
        let arn = resource.build_arn("my-bucket");
        assert_eq!(arn, "arn:aws:s3:::my-bucket");
    }

    #[test]
    fn test_build_domain_name() {
        let resource = AwsS3BucketResource::new();
        let domain = resource.build_domain_name("my-bucket");
        assert_eq!(domain, "my-bucket.s3.amazonaws.com");
    }

    #[test]
    fn test_build_regional_domain_name() {
        let resource = AwsS3BucketResource::new();
        let domain = resource.build_regional_domain_name("my-bucket", "us-west-2");
        assert_eq!(domain, "my-bucket.s3.us-west-2.amazonaws.com");
    }

    #[test]
    fn test_hosted_zone_ids() {
        let resource = AwsS3BucketResource::new();

        assert_eq!(resource.get_hosted_zone_id("us-east-1"), "Z3AQBSTGFYJSTF");
        assert_eq!(resource.get_hosted_zone_id("eu-west-1"), "Z1BKCTXD74EZPE");
        assert_eq!(resource.get_hosted_zone_id("ap-northeast-1"), "Z2M4EHUR26P7ZW");
    }

    #[test]
    fn test_plan_create() {
        let resource = AwsS3BucketResource::new();

        let desired = json!({
            "bucket": "my-bucket"
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
        let resource = AwsS3BucketResource::new();

        let config = json!({
            "bucket": "my-bucket"
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
    fn test_plan_replace_bucket_name() {
        let resource = AwsS3BucketResource::new();

        let current = json!({
            "bucket": "old-bucket"
        });

        let desired = json!({
            "bucket": "new-bucket"
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
        let state = S3BucketState {
            id: "my-bucket".to_string(),
            arn: "arn:aws:s3:::my-bucket".to_string(),
            bucket_domain_name: "my-bucket.s3.amazonaws.com".to_string(),
            bucket_regional_domain_name: "my-bucket.s3.us-east-1.amazonaws.com".to_string(),
            hosted_zone_id: Some("Z3AQBSTGFYJSTF".to_string()),
            region: Some("us-east-1".to_string()),
            versioning_enabled: true,
            sse_algorithm: Some("AES256".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "my-bucket");
        assert_eq!(json["arn"], "arn:aws:s3:::my-bucket");
        assert_eq!(json["versioning_enabled"], true);
    }

    #[test]
    fn test_build_tags() {
        let resource = AwsS3BucketResource::new();

        let mut tags = HashMap::new();
        tags.insert("Name".to_string(), "my-bucket".to_string());
        tags.insert("Environment".to_string(), "test".to_string());

        let s3_tags = resource.build_tags(&tags);
        assert_eq!(s3_tags.len(), 2);
    }
}
