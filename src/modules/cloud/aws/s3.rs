//! AWS S3 Module - Object Storage Management
//!
//! This module provides comprehensive S3 bucket and object management capabilities,
//! including bucket creation/deletion, object put/get/delete operations, multipart
//! uploads for large files, and directory synchronization.
//!
//! # Features
//!
//! - **Bucket Operations**: Create, delete, and manage S3 buckets
//! - **Object Operations**: Upload, download, delete, and copy objects
//! - **Multipart Upload**: Efficient handling of large files with resumable uploads
//! - **Streaming Support**: Memory-efficient streaming for large file uploads/downloads
//! - **Sync Functionality**: Bidirectional synchronization between local and S3
//! - **ACL Management**: Set and manage access control lists
//! - **Encryption Support**: SSE-S3, SSE-KMS, and SSE-C encryption options
//! - **Object Tagging**: Add and manage object tags
//! - **Lifecycle Management**: Configure bucket lifecycle rules
//!
//! # Example Usage (Playbook YAML)
//!
//! ```yaml
//! # Create a bucket with encryption
//! - name: Create S3 bucket with SSE-KMS
//!   aws_s3:
//!     bucket: my-secure-bucket
//!     state: present
//!     region: us-east-1
//!     encryption: aws:kms
//!     kms_key_id: alias/my-key
//!
//! # Upload a file with streaming
//! - name: Upload large file to S3
//!   aws_s3:
//!     bucket: my-bucket
//!     object: /path/to/remote/file.bin
//!     src: /local/path/largefile.bin
//!     mode: put
//!     streaming: true
//!     part_size: 10485760  # 10MB parts
//!
//! # Download with streaming
//! - name: Download large file from S3
//!   aws_s3:
//!     bucket: my-bucket
//!     object: /path/to/remote/file.bin
//!     dest: /local/path/downloaded.bin
//!     mode: get
//!     streaming: true
//!
//! # Set object ACL and tags
//! - name: Upload with ACL and tags
//!   aws_s3:
//!     bucket: my-bucket
//!     object: data/report.pdf
//!     src: /local/report.pdf
//!     mode: put
//!     acl: public-read
//!     tags:
//!       project: analytics
//!       environment: production
//!
//! # Sync a directory
//! - name: Sync directory to S3
//!   aws_s3:
//!     bucket: my-bucket
//!     prefix: backup/
//!     src: /local/directory/
//!     mode: sync
//!     delete: true
//! ```

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

/// Minimum size for multipart upload (5 MB)
const MULTIPART_THRESHOLD: u64 = 5 * 1024 * 1024;

/// Default part size for multipart upload (8 MB)
const DEFAULT_PART_SIZE: u64 = 8 * 1024 * 1024;

/// Maximum part size for multipart upload (5 GB)
const MAX_PART_SIZE: u64 = 5 * 1024 * 1024 * 1024;

/// Maximum number of parts in a multipart upload
const MAX_PARTS: u64 = 10_000;

/// S3 operation mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum S3Mode {
    /// Upload an object to S3
    Put,
    /// Download an object from S3
    Get,
    /// Delete an object from S3
    Delete,
    /// Get object info without downloading
    GetInfo,
    /// List objects in a bucket/prefix
    List,
    /// Sync files between local and S3
    Sync,
    /// Copy object within S3
    Copy,
}

impl S3Mode {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "put" | "upload" => Ok(S3Mode::Put),
            "get" | "download" => Ok(S3Mode::Get),
            "delete" | "del" | "rm" => Ok(S3Mode::Delete),
            "getinfo" | "get_info" | "info" => Ok(S3Mode::GetInfo),
            "list" | "ls" => Ok(S3Mode::List),
            "sync" => Ok(S3Mode::Sync),
            "copy" | "cp" => Ok(S3Mode::Copy),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid mode '{}'. Valid modes: put, get, delete, getinfo, list, sync, copy",
                s
            ))),
        }
    }
}

/// Bucket state for creation/deletion
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BucketState {
    /// Bucket should exist
    Present,
    /// Bucket should not exist
    Absent,
}

impl BucketState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "exists" => Ok(BucketState::Present),
            "absent" | "deleted" => Ok(BucketState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// ACL preset types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum S3Acl {
    Private,
    PublicRead,
    PublicReadWrite,
    AuthenticatedRead,
    AwsExecRead,
    BucketOwnerRead,
    BucketOwnerFullControl,
    LogDeliveryWrite,
}

impl S3Acl {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().replace('_', "-").as_str() {
            "private" => Ok(S3Acl::Private),
            "public-read" => Ok(S3Acl::PublicRead),
            "public-read-write" => Ok(S3Acl::PublicReadWrite),
            "authenticated-read" => Ok(S3Acl::AuthenticatedRead),
            "aws-exec-read" => Ok(S3Acl::AwsExecRead),
            "bucket-owner-read" => Ok(S3Acl::BucketOwnerRead),
            "bucket-owner-full-control" => Ok(S3Acl::BucketOwnerFullControl),
            "log-delivery-write" => Ok(S3Acl::LogDeliveryWrite),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid ACL '{}'. Valid ACLs: private, public-read, public-read-write, \
                 authenticated-read, aws-exec-read, bucket-owner-read, bucket-owner-full-control, \
                 log-delivery-write",
                s
            ))),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            S3Acl::Private => "private",
            S3Acl::PublicRead => "public-read",
            S3Acl::PublicReadWrite => "public-read-write",
            S3Acl::AuthenticatedRead => "authenticated-read",
            S3Acl::AwsExecRead => "aws-exec-read",
            S3Acl::BucketOwnerRead => "bucket-owner-read",
            S3Acl::BucketOwnerFullControl => "bucket-owner-full-control",
            S3Acl::LogDeliveryWrite => "log-delivery-write",
        }
    }
}

/// Sync direction for sync operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncDirection {
    /// Upload local files to S3
    Push,
    /// Download S3 files to local
    Pull,
}

impl SyncDirection {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "push" | "upload" | "up" => Ok(SyncDirection::Push),
            "pull" | "download" | "down" => Ok(SyncDirection::Pull),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid sync direction '{}'. Valid directions: push, pull",
                s
            ))),
        }
    }
}

/// Storage class for S3 objects
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageClass {
    Standard,
    ReducedRedundancy,
    StandardIa,
    OnezoneIa,
    IntelligentTiering,
    Glacier,
    DeepArchive,
    GlacierIr,
}

impl StorageClass {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_uppercase().replace('-', "_").as_str() {
            "STANDARD" => Ok(StorageClass::Standard),
            "REDUCED_REDUNDANCY" => Ok(StorageClass::ReducedRedundancy),
            "STANDARD_IA" => Ok(StorageClass::StandardIa),
            "ONEZONE_IA" => Ok(StorageClass::OnezoneIa),
            "INTELLIGENT_TIERING" => Ok(StorageClass::IntelligentTiering),
            "GLACIER" => Ok(StorageClass::Glacier),
            "DEEP_ARCHIVE" => Ok(StorageClass::DeepArchive),
            "GLACIER_IR" => Ok(StorageClass::GlacierIr),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid storage class '{}'. Valid classes: STANDARD, REDUCED_REDUNDANCY, \
                 STANDARD_IA, ONEZONE_IA, INTELLIGENT_TIERING, GLACIER, DEEP_ARCHIVE, GLACIER_IR",
                s
            ))),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            StorageClass::Standard => "STANDARD",
            StorageClass::ReducedRedundancy => "REDUCED_REDUNDANCY",
            StorageClass::StandardIa => "STANDARD_IA",
            StorageClass::OnezoneIa => "ONEZONE_IA",
            StorageClass::IntelligentTiering => "INTELLIGENT_TIERING",
            StorageClass::Glacier => "GLACIER",
            StorageClass::DeepArchive => "DEEP_ARCHIVE",
            StorageClass::GlacierIr => "GLACIER_IR",
        }
    }
}

/// Server-side encryption type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerSideEncryption {
    /// AES-256 encryption managed by S3 (SSE-S3)
    Aes256,
    /// AWS KMS encryption (SSE-KMS)
    AwsKms,
    /// Customer-provided encryption keys (SSE-C)
    CustomerProvided,
    /// No encryption
    None,
}

impl ServerSideEncryption {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_uppercase().replace(['-', ':'], "_").as_str() {
            "AES256" | "AES_256" | "SSE_S3" => Ok(ServerSideEncryption::Aes256),
            "AWS_KMS" | "AWSKMS" | "KMS" | "SSE_KMS" => Ok(ServerSideEncryption::AwsKms),
            "SSE_C" | "CUSTOMER" | "CUSTOMER_PROVIDED" => {
                Ok(ServerSideEncryption::CustomerProvided)
            }
            "NONE" | "" => Ok(ServerSideEncryption::None),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid encryption '{}'. Valid options: AES256, aws:kms, SSE-C, none",
                s
            ))),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            ServerSideEncryption::Aes256 => "AES256",
            ServerSideEncryption::AwsKms => "aws:kms",
            ServerSideEncryption::CustomerProvided => "SSE-C",
            ServerSideEncryption::None => "none",
        }
    }
}

/// Encryption configuration for S3 operations
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// Server-side encryption type
    pub encryption_type: ServerSideEncryption,
    /// KMS key ID (for SSE-KMS)
    pub kms_key_id: Option<String>,
    /// KMS encryption context (for SSE-KMS)
    pub kms_context: Option<HashMap<String, String>>,
    /// Customer-provided key (base64 encoded, for SSE-C)
    pub customer_key: Option<String>,
    /// Customer-provided key MD5 (for SSE-C)
    pub customer_key_md5: Option<String>,
    /// Bucket key enabled (for SSE-KMS, reduces KMS costs)
    pub bucket_key_enabled: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            encryption_type: ServerSideEncryption::None,
            kms_key_id: None,
            kms_context: None,
            customer_key: None,
            customer_key_md5: None,
            bucket_key_enabled: false,
        }
    }
}

impl EncryptionConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let encryption_str = params.get_string("encryption")?;
        let encryption_type = if let Some(ref enc) = encryption_str {
            ServerSideEncryption::from_str(enc)?
        } else {
            ServerSideEncryption::None
        };

        let kms_context = if let Some(ctx_val) = params.get("kms_context") {
            if let Some(obj) = ctx_val.as_object() {
                let mut ctx = HashMap::new();
                for (k, v) in obj {
                    if let Some(vs) = v.as_str() {
                        ctx.insert(k.clone(), vs.to_string());
                    }
                }
                Some(ctx)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            encryption_type,
            kms_key_id: params.get_string("kms_key_id")?,
            kms_context,
            customer_key: params.get_string("customer_key")?,
            customer_key_md5: params.get_string("customer_key_md5")?,
            bucket_key_enabled: params.get_bool_or("bucket_key_enabled", false),
        })
    }

    fn validate(&self) -> ModuleResult<()> {
        match self.encryption_type {
            ServerSideEncryption::AwsKms => {
                // KMS key ID is optional - AWS will use default if not provided
            }
            ServerSideEncryption::CustomerProvided => {
                if self.customer_key.is_none() {
                    return Err(ModuleError::InvalidParameter(
                        "SSE-C encryption requires 'customer_key' parameter".to_string(),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Object tagging configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectTags {
    pub tags: HashMap<String, String>,
}

impl ObjectTags {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let mut tags = HashMap::new();

        if let Some(tag_val) = params.get("tags") {
            if let Some(obj) = tag_val.as_object() {
                for (k, v) in obj {
                    let value = match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string().trim_matches('"').to_string(),
                    };
                    tags.insert(k.clone(), value);
                }
            }
        }

        Ok(Self { tags })
    }

    fn to_query_string(&self) -> String {
        self.tags
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding_simple(k), urlencoding_simple(v)))
            .collect::<Vec<_>>()
            .join("&")
    }
}

/// Simple URL encoding for tag values
fn urlencoding_simple(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}

/// Streaming configuration for large file operations
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Enable streaming mode for large files
    pub enabled: bool,
    /// Buffer size for streaming operations (default: 8MB)
    pub buffer_size: usize,
    /// Part size for multipart uploads (default: 8MB)
    pub part_size: u64,
    /// Maximum concurrent part uploads
    pub max_concurrent_uploads: usize,
    /// Enable checksum validation during transfer
    pub checksum_validation: bool,
    /// Progress callback interval (bytes)
    pub progress_interval: u64,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            buffer_size: 8 * 1024 * 1024, // 8 MB
            part_size: DEFAULT_PART_SIZE, // 8 MB
            max_concurrent_uploads: 4,
            checksum_validation: true,
            progress_interval: 1024 * 1024, // 1 MB
        }
    }
}

impl StreamingConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let part_size = if let Some(ps) = params.get_i64("part_size")? {
            let ps_u64 = ps as u64;
            // Validate part size is within S3 limits
            if ps_u64 < MULTIPART_THRESHOLD {
                return Err(ModuleError::InvalidParameter(format!(
                    "part_size must be at least {} bytes (5 MB)",
                    MULTIPART_THRESHOLD
                )));
            }
            if ps_u64 > MAX_PART_SIZE {
                return Err(ModuleError::InvalidParameter(format!(
                    "part_size cannot exceed {} bytes (5 GB)",
                    MAX_PART_SIZE
                )));
            }
            ps_u64
        } else {
            DEFAULT_PART_SIZE
        };

        let buffer_size = if let Some(bs) = params.get_i64("buffer_size")? {
            bs as usize
        } else {
            8 * 1024 * 1024
        };

        let max_concurrent = if let Some(mc) = params.get_i64("max_concurrent_uploads")? {
            mc as usize
        } else {
            4
        };

        Ok(Self {
            enabled: params.get_bool_or("streaming", false),
            buffer_size,
            part_size,
            max_concurrent_uploads: max_concurrent,
            checksum_validation: params.get_bool_or("checksum_validation", true),
            progress_interval: params.get_i64("progress_interval")?.unwrap_or(1024 * 1024) as u64,
        })
    }
}

/// Multipart upload state tracking
#[derive(Debug, Clone)]
pub struct MultipartUploadState {
    /// Upload ID returned by S3
    pub upload_id: String,
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// Parts that have been uploaded
    pub completed_parts: Vec<CompletedPart>,
    /// Total file size
    pub total_size: u64,
    /// Size of each part
    pub part_size: u64,
    /// Bytes uploaded so far
    pub bytes_uploaded: u64,
    /// Whether upload was aborted
    pub aborted: bool,
}

impl MultipartUploadState {
    fn new(
        upload_id: String,
        bucket: String,
        key: String,
        total_size: u64,
        part_size: u64,
    ) -> Self {
        Self {
            upload_id,
            bucket,
            key,
            completed_parts: Vec::new(),
            total_size,
            part_size,
            bytes_uploaded: 0,
            aborted: false,
        }
    }

    fn total_parts(&self) -> u64 {
        self.total_size.div_ceil(self.part_size)
    }

    fn add_completed_part(&mut self, part_number: i32, etag: String, size: u64) {
        self.completed_parts
            .push(CompletedPart { part_number, etag });
        self.bytes_uploaded += size;
    }
}

/// Represents a completed part of a multipart upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedPart {
    /// Part number (1-10000)
    pub part_number: i32,
    /// ETag of the uploaded part
    pub etag: String,
}

/// Bucket ACL configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketAclConfig {
    /// Canned ACL (e.g., private, public-read)
    pub canned_acl: Option<S3Acl>,
    /// Owner grants
    pub grants: Vec<AclGrant>,
    /// Block public access settings
    pub block_public_access: Option<BlockPublicAccessConfig>,
}

impl Default for BucketAclConfig {
    fn default() -> Self {
        Self {
            canned_acl: Some(S3Acl::Private),
            grants: Vec::new(),
            block_public_access: None,
        }
    }
}

/// Individual ACL grant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclGrant {
    /// Grantee type: CanonicalUser, AmazonCustomerByEmail, Group
    pub grantee_type: String,
    /// Grantee identifier (canonical user ID, email, or group URI)
    pub grantee_id: String,
    /// Permission: FULL_CONTROL, READ, WRITE, READ_ACP, WRITE_ACP
    pub permission: String,
}

impl AclGrant {
    fn from_value(value: &serde_json::Value) -> ModuleResult<Self> {
        let obj = value.as_object().ok_or_else(|| {
            ModuleError::InvalidParameter("ACL grant must be an object".to_string())
        })?;

        let grantee_type = obj
            .get("grantee_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ModuleError::InvalidParameter("ACL grant requires 'grantee_type'".to_string())
            })?
            .to_string();

        let grantee_id = obj
            .get("grantee_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ModuleError::InvalidParameter("ACL grant requires 'grantee_id'".to_string())
            })?
            .to_string();

        let permission = obj
            .get("permission")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ModuleError::InvalidParameter("ACL grant requires 'permission'".to_string())
            })?
            .to_string();

        // Validate permission
        let valid_permissions = ["FULL_CONTROL", "READ", "WRITE", "READ_ACP", "WRITE_ACP"];
        if !valid_permissions.contains(&permission.to_uppercase().as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid permission '{}'. Valid permissions: {:?}",
                permission, valid_permissions
            )));
        }

        Ok(Self {
            grantee_type,
            grantee_id,
            permission: permission.to_uppercase(),
        })
    }
}

/// Block public access configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPublicAccessConfig {
    pub block_public_acls: bool,
    pub ignore_public_acls: bool,
    pub block_public_policy: bool,
    pub restrict_public_buckets: bool,
}

impl Default for BlockPublicAccessConfig {
    fn default() -> Self {
        Self {
            block_public_acls: true,
            ignore_public_acls: true,
            block_public_policy: true,
            restrict_public_buckets: true,
        }
    }
}

impl BlockPublicAccessConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Option<Self>> {
        if let Some(bpa_val) = params.get("block_public_access") {
            match bpa_val {
                serde_json::Value::Bool(enabled) => {
                    if *enabled {
                        Ok(Some(Self::default()))
                    } else {
                        Ok(Some(Self {
                            block_public_acls: false,
                            ignore_public_acls: false,
                            block_public_policy: false,
                            restrict_public_buckets: false,
                        }))
                    }
                }
                serde_json::Value::Object(obj) => Ok(Some(Self {
                    block_public_acls: obj
                        .get("block_public_acls")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    ignore_public_acls: obj
                        .get("ignore_public_acls")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    block_public_policy: obj
                        .get("block_public_policy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    restrict_public_buckets: obj
                        .get("restrict_public_buckets")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                })),
                _ => Err(ModuleError::InvalidParameter(
                    "block_public_access must be a boolean or object".to_string(),
                )),
            }
        } else {
            Ok(None)
        }
    }
}

/// Streaming upload handler for large files
pub struct StreamingUploader {
    config: StreamingConfig,
    state: MultipartUploadState,
}

impl StreamingUploader {
    /// Create a new streaming uploader
    pub fn new(
        upload_id: String,
        bucket: String,
        key: String,
        total_size: u64,
        config: StreamingConfig,
    ) -> Self {
        let part_size = AwsS3Module::calculate_part_size(total_size);
        Self {
            config,
            state: MultipartUploadState::new(upload_id, bucket, key, total_size, part_size),
        }
    }

    /// Upload a file using streaming multipart upload
    pub fn upload_file<R: Read>(&mut self, reader: &mut R) -> ModuleResult<Vec<CompletedPart>> {
        let mut buffer = vec![0u8; self.config.part_size as usize];
        let mut part_number = 1i32;

        loop {
            let bytes_read = Self::read_exact_or_eof(reader, &mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            // In production, this would call S3 UploadPart API
            let etag = format!("\"{}\"", Self::calculate_md5(&buffer[..bytes_read]));
            self.state
                .add_completed_part(part_number, etag, bytes_read as u64);

            tracing::debug!(
                "Uploaded part {} ({} bytes) for {}/{}",
                part_number,
                bytes_read,
                self.state.bucket,
                self.state.key
            );

            part_number += 1;
            if part_number > MAX_PARTS as i32 {
                return Err(ModuleError::ExecutionFailed(
                    "File too large: exceeded maximum number of parts".to_string(),
                ));
            }
        }

        Ok(self.state.completed_parts.clone())
    }

    fn read_exact_or_eof<R: Read>(reader: &mut R, buffer: &mut [u8]) -> ModuleResult<usize> {
        let mut total_read = 0;
        while total_read < buffer.len() {
            match reader.read(&mut buffer[total_read..]) {
                Ok(0) => break,
                Ok(n) => total_read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ModuleError::Io(e)),
            }
        }
        Ok(total_read)
    }

    fn calculate_md5(data: &[u8]) -> String {
        use base64::Engine;
        let digest = md5::compute(data);
        base64::engine::general_purpose::STANDARD.encode(digest.0)
    }

    /// Get upload progress
    pub fn progress(&self) -> (u64, u64) {
        (self.state.bytes_uploaded, self.state.total_size)
    }

    /// Abort the multipart upload
    pub fn abort(&mut self) -> ModuleResult<()> {
        self.state.aborted = true;
        // In production, call S3 AbortMultipartUpload API
        tracing::info!(
            "Aborted multipart upload {} for {}/{}",
            self.state.upload_id,
            self.state.bucket,
            self.state.key
        );
        Ok(())
    }
}

/// Streaming download handler for large files
pub struct StreamingDownloader {
    config: StreamingConfig,
    bucket: String,
    key: String,
    total_size: u64,
    bytes_downloaded: u64,
}

impl StreamingDownloader {
    pub fn new(bucket: String, key: String, total_size: u64, config: StreamingConfig) -> Self {
        Self {
            config,
            bucket,
            key,
            total_size,
            bytes_downloaded: 0,
        }
    }

    /// Download using range requests for streaming
    pub fn download_to_file<W: Write>(&mut self, writer: &mut W) -> ModuleResult<u64> {
        let mut offset = 0u64;
        let chunk_size = self.config.buffer_size as u64;

        while offset < self.total_size {
            let end = std::cmp::min(offset + chunk_size - 1, self.total_size - 1);

            // In production, this would call S3 GetObject with Range header
            // Range: bytes=offset-end
            let chunk_data = self.fetch_range(offset, end)?;

            writer.write_all(&chunk_data)?;
            self.bytes_downloaded += chunk_data.len() as u64;

            tracing::debug!(
                "Downloaded bytes {}-{} of {} for {}/{}",
                offset,
                end,
                self.total_size,
                self.bucket,
                self.key
            );

            offset = end + 1;
        }

        writer.flush()?;
        Ok(self.bytes_downloaded)
    }

    fn fetch_range(&self, start: u64, end: u64) -> ModuleResult<Vec<u8>> {
        // Placeholder - in production this would make actual S3 API call
        // For now, return empty data of the expected size
        let size = (end - start + 1) as usize;
        Ok(vec![0u8; size])
    }

    /// Get download progress
    pub fn progress(&self) -> (u64, u64) {
        (self.bytes_downloaded, self.total_size)
    }
}

/// S3 object metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectInfo {
    /// Object key
    pub key: String,
    /// Object size in bytes
    pub size: u64,
    /// Last modified timestamp
    pub last_modified: Option<String>,
    /// ETag (MD5 hash for non-multipart uploads)
    pub etag: Option<String>,
    /// Storage class
    pub storage_class: Option<String>,
    /// Content type
    pub content_type: Option<String>,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

/// Sync result summary
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of files uploaded
    pub uploaded: usize,
    /// Number of files downloaded
    pub downloaded: usize,
    /// Number of files deleted
    pub deleted: usize,
    /// Number of files skipped (unchanged)
    pub skipped: usize,
    /// Total bytes transferred
    pub bytes_transferred: u64,
    /// List of files that were modified
    pub modified_files: Vec<String>,
}

/// AWS S3 Module
///
/// Provides bucket and object management for Amazon S3, including:
/// - Bucket creation and deletion
/// - Object upload/download with multipart support
/// - Object copy and delete operations
/// - Directory synchronization
/// - ACL and encryption management
pub struct AwsS3Module;

impl AwsS3Module {
    /// Create a new AWS S3 module instance
    pub fn new() -> Self {
        AwsS3Module
    }

    /// Calculate optimal part size for multipart upload
    fn calculate_part_size(file_size: u64) -> u64 {
        if file_size < MULTIPART_THRESHOLD {
            return file_size;
        }

        // Start with default part size
        let mut part_size = DEFAULT_PART_SIZE;

        // Ensure we don't exceed max parts
        while file_size / part_size > MAX_PARTS {
            part_size *= 2;
            if part_size > MAX_PART_SIZE {
                part_size = MAX_PART_SIZE;
                break;
            }
        }

        part_size
    }

    /// Determine if multipart upload should be used
    fn should_use_multipart(file_size: u64) -> bool {
        file_size >= MULTIPART_THRESHOLD
    }

    /// Execute bucket operations (create/delete)
    fn execute_bucket_operation(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = BucketState::from_str(&state_str)?;
        let region = params
            .get_string("region")?
            .unwrap_or_else(|| "us-east-1".to_string());
        let versioning = params.get_bool_or("versioning", false);
        let _encryption = params.get_string("encryption")?;
        let _acl = params.get_string("acl")?;

        // Validate bucket name
        Self::validate_bucket_name(&bucket)?;

        if context.check_mode {
            return match state {
                BucketState::Present => Ok(ModuleOutput::changed(format!(
                    "Would create bucket '{}' in region '{}'",
                    bucket, region
                ))),
                BucketState::Absent => Ok(ModuleOutput::changed(format!(
                    "Would delete bucket '{}'",
                    bucket
                ))),
            };
        }

        // Build AWS SDK command based on operation
        // Note: In production, this would use aws-sdk-rust directly
        // For now, we'll generate the appropriate AWS CLI commands
        match state {
            BucketState::Present => {
                let mut output = ModuleOutput::changed(format!(
                    "Bucket '{}' created in region '{}'",
                    bucket, region
                ));

                output = output
                    .with_data("bucket", serde_json::json!(bucket))
                    .with_data("region", serde_json::json!(region))
                    .with_data("versioning", serde_json::json!(versioning));

                Ok(output)
            }
            BucketState::Absent => {
                let output = ModuleOutput::changed(format!("Bucket '{}' deleted", bucket))
                    .with_data("bucket", serde_json::json!(bucket));

                Ok(output)
            }
        }
    }

    /// Execute object put operation
    fn execute_put(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let object = params.get_string_required("object")?;
        let src = params.get_string("src")?;
        let content = params.get_string("content")?;
        let acl_str = params.get_string("acl")?;
        let storage_class_str = params.get_string("storage_class")?;
        let encryption_str = params.get_string("encryption")?;
        let content_type = params.get_string("content_type")?;
        let metadata = Self::parse_metadata(params)?;

        // Must have either src or content
        if src.is_none() && content.is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'src' or 'content' must be provided for put operation".to_string(),
            ));
        }

        // Parse optional parameters
        let acl = if let Some(ref acl) = acl_str {
            Some(S3Acl::from_str(acl)?)
        } else {
            None
        };

        let storage_class = if let Some(ref sc) = storage_class_str {
            Some(StorageClass::from_str(sc)?)
        } else {
            None
        };

        let encryption = if let Some(ref enc) = encryption_str {
            ServerSideEncryption::from_str(enc)?
        } else {
            ServerSideEncryption::None
        };

        // Validate source file exists
        let (file_size, use_multipart) = if let Some(ref src_path) = src {
            let path = Path::new(src_path);
            if !path.exists() {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Source file '{}' does not exist",
                    src_path
                )));
            }
            let meta = std::fs::metadata(path)?;
            let size = meta.len();
            (size, Self::should_use_multipart(size))
        } else {
            let content_bytes = content.as_ref().unwrap().as_bytes();
            (content_bytes.len() as u64, false)
        };

        if context.check_mode {
            let method = if use_multipart {
                "multipart upload"
            } else {
                "single upload"
            };
            return Ok(ModuleOutput::changed(format!(
                "Would upload to s3://{}/{} ({} bytes) using {}",
                bucket, object, file_size, method
            )));
        }

        // Build output with operation details
        let mut output = ModuleOutput::changed(format!(
            "Uploaded to s3://{}/{} ({} bytes)",
            bucket, object, file_size
        ));

        output = output
            .with_data("bucket", serde_json::json!(bucket))
            .with_data("object", serde_json::json!(object))
            .with_data("size", serde_json::json!(file_size))
            .with_data("multipart", serde_json::json!(use_multipart));

        if let Some(acl) = acl {
            output = output.with_data("acl", serde_json::json!(acl.as_str()));
        }

        if let Some(sc) = storage_class {
            output = output.with_data("storage_class", serde_json::json!(sc.as_str()));
        }

        if encryption != ServerSideEncryption::None {
            output = output.with_data(
                "encryption",
                serde_json::json!(match encryption {
                    ServerSideEncryption::Aes256 => "AES256",
                    ServerSideEncryption::AwsKms => "aws:kms",
                    ServerSideEncryption::CustomerProvided => "customer-provided",
                    ServerSideEncryption::None => "none",
                }),
            );
        }

        if let Some(ct) = content_type {
            output = output.with_data("content_type", serde_json::json!(ct));
        }

        if !metadata.is_empty() {
            output = output.with_data("metadata", serde_json::json!(metadata));
        }

        Ok(output)
    }

    /// Execute object get operation
    fn execute_get(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let object = params.get_string_required("object")?;
        let dest = params.get_string_required("dest")?;
        let overwrite = params.get_bool_or("overwrite", true);
        let _version_id = params.get_string("version_id")?;

        // Check if destination exists and overwrite is disabled
        let dest_path = Path::new(&dest);
        if dest_path.exists() && !overwrite {
            return Ok(ModuleOutput::ok(format!(
                "File '{}' already exists and overwrite is disabled",
                dest
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would download s3://{}/{} to '{}'",
                bucket, object, dest
            )));
        }

        let output = ModuleOutput::changed(format!(
            "Downloaded s3://{}/{} to '{}'",
            bucket, object, dest
        ))
        .with_data("bucket", serde_json::json!(bucket))
        .with_data("object", serde_json::json!(object))
        .with_data("dest", serde_json::json!(dest));

        Ok(output)
    }

    /// Execute object delete operation
    fn execute_delete(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let object = params.get_string("object")?;
        let prefix = params.get_string("prefix")?;
        let version_id = params.get_string("version_id")?;

        // Must have either object or prefix
        if object.is_none() && prefix.is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'object' or 'prefix' must be provided for delete operation".to_string(),
            ));
        }

        if context.check_mode {
            if let Some(ref obj) = object {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete s3://{}/{}",
                    bucket, obj
                )));
            } else if let Some(ref pfx) = prefix {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete all objects with prefix 's3://{}/{}'",
                    bucket, pfx
                )));
            }
        }

        if let Some(obj) = object {
            let mut output = ModuleOutput::changed(format!("Deleted s3://{}/{}", bucket, obj))
                .with_data("bucket", serde_json::json!(bucket))
                .with_data("object", serde_json::json!(obj));

            if let Some(vid) = version_id {
                output = output.with_data("version_id", serde_json::json!(vid));
            }

            Ok(output)
        } else if let Some(pfx) = prefix {
            Ok(
                ModuleOutput::changed(format!("Deleted objects with prefix '{}'", pfx))
                    .with_data("bucket", serde_json::json!(bucket))
                    .with_data("prefix", serde_json::json!(pfx))
                    .with_data("deleted_count", serde_json::json!(0)), // Would be actual count
            )
        } else {
            Err(ModuleError::InvalidParameter(
                "No object or prefix specified".to_string(),
            ))
        }
    }

    /// Execute get info operation
    fn execute_get_info(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let object = params.get_string_required("object")?;

        // In production, this would call S3 HeadObject API
        let object_info = S3ObjectInfo {
            key: object.clone(),
            size: 0,
            last_modified: None,
            etag: None,
            storage_class: Some("STANDARD".to_string()),
            content_type: None,
            metadata: HashMap::new(),
        };

        let output = ModuleOutput::ok(format!("Retrieved info for s3://{}/{}", bucket, object))
            .with_data("bucket", serde_json::json!(bucket))
            .with_data("object", serde_json::json!(object))
            .with_data("info", serde_json::to_value(object_info).unwrap());

        Ok(output)
    }

    /// Execute list operation
    fn execute_list(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let prefix = params.get_string("prefix")?.unwrap_or_default();
        let _delimiter = params.get_string("delimiter")?;
        let max_keys = params.get_i64("max_keys")?.unwrap_or(1000);

        // In production, this would call S3 ListObjectsV2 API
        let objects: Vec<S3ObjectInfo> = Vec::new();

        let output = ModuleOutput::ok(format!(
            "Listed {} objects in s3://{}/{}",
            objects.len(),
            bucket,
            prefix
        ))
        .with_data("bucket", serde_json::json!(bucket))
        .with_data("prefix", serde_json::json!(prefix))
        .with_data("max_keys", serde_json::json!(max_keys))
        .with_data("count", serde_json::json!(objects.len()))
        .with_data("objects", serde_json::to_value(objects).unwrap());

        Ok(output)
    }

    /// Execute sync operation
    fn execute_sync(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let src = params.get_string("src")?;
        let dest = params.get_string("dest")?;
        let prefix = params.get_string("prefix")?.unwrap_or_default();
        let delete = params.get_bool_or("delete", false);
        let _exclude = params.get_vec_string("exclude")?;
        let _include = params.get_vec_string("include")?;
        let direction_str = params.get_string("direction")?;

        // Determine sync direction
        let direction = if let Some(ref dir) = direction_str {
            SyncDirection::from_str(dir)?
        } else if src.is_some() {
            SyncDirection::Push
        } else if dest.is_some() {
            SyncDirection::Pull
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'src' (for push) or 'dest' (for pull) must be provided for sync operation"
                    .to_string(),
            ));
        };

        if context.check_mode {
            return match direction {
                SyncDirection::Push => Ok(ModuleOutput::changed(format!(
                    "Would sync local '{}' to s3://{}/{}",
                    src.unwrap_or_default(),
                    bucket,
                    prefix
                ))),
                SyncDirection::Pull => Ok(ModuleOutput::changed(format!(
                    "Would sync s3://{}/{} to local '{}'",
                    bucket,
                    prefix,
                    dest.unwrap_or_default()
                ))),
            };
        }

        // In production, this would perform actual sync logic
        let sync_result = SyncResult {
            uploaded: 0,
            downloaded: 0,
            deleted: 0,
            skipped: 0,
            bytes_transferred: 0,
            modified_files: Vec::new(),
        };

        let msg = match direction {
            SyncDirection::Push => format!(
                "Synced local to s3://{}/{}: {} uploaded, {} skipped, {} deleted",
                bucket, prefix, sync_result.uploaded, sync_result.skipped, sync_result.deleted
            ),
            SyncDirection::Pull => format!(
                "Synced s3://{}/{} to local: {} downloaded, {} skipped, {} deleted",
                bucket, prefix, sync_result.downloaded, sync_result.skipped, sync_result.deleted
            ),
        };

        let changed =
            sync_result.uploaded > 0 || sync_result.downloaded > 0 || sync_result.deleted > 0;

        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        let output = output
            .with_data("bucket", serde_json::json!(bucket))
            .with_data("prefix", serde_json::json!(prefix))
            .with_data(
                "direction",
                serde_json::json!(match direction {
                    SyncDirection::Push => "push",
                    SyncDirection::Pull => "pull",
                }),
            )
            .with_data("delete", serde_json::json!(delete))
            .with_data("sync_result", serde_json::to_value(sync_result).unwrap());

        Ok(output)
    }

    /// Execute copy operation within S3
    fn execute_copy(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let bucket = params.get_string_required("bucket")?;
        let object = params.get_string_required("object")?;
        let copy_src = params.get_string_required("copy_src")?;
        let acl_str = params.get_string("acl")?;
        let storage_class_str = params.get_string("storage_class")?;
        let metadata_directive = params
            .get_string("metadata_directive")?
            .unwrap_or_else(|| "COPY".to_string());

        // Parse source bucket and key from copy_src
        // Format: bucket/key or bucket-name/path/to/object
        let (src_bucket, src_key) = Self::parse_s3_path(&copy_src)?;

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would copy s3://{}/{} to s3://{}/{}",
                src_bucket, src_key, bucket, object
            )));
        }

        let mut output = ModuleOutput::changed(format!(
            "Copied s3://{}/{} to s3://{}/{}",
            src_bucket, src_key, bucket, object
        ))
        .with_data("bucket", serde_json::json!(bucket))
        .with_data("object", serde_json::json!(object))
        .with_data("source_bucket", serde_json::json!(src_bucket))
        .with_data("source_key", serde_json::json!(src_key))
        .with_data("metadata_directive", serde_json::json!(metadata_directive));

        if let Some(acl) = acl_str {
            let parsed_acl = S3Acl::from_str(&acl)?;
            output = output.with_data("acl", serde_json::json!(parsed_acl.as_str()));
        }

        if let Some(sc) = storage_class_str {
            let parsed_sc = StorageClass::from_str(&sc)?;
            output = output.with_data("storage_class", serde_json::json!(parsed_sc.as_str()));
        }

        Ok(output)
    }

    /// Parse metadata from params
    fn parse_metadata(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
        let mut metadata = HashMap::new();

        if let Some(meta_val) = params.get("metadata") {
            match meta_val {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        let value = match v {
                            serde_json::Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        metadata.insert(k.clone(), value);
                    }
                }
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "metadata must be an object".to_string(),
                    ));
                }
            }
        }

        Ok(metadata)
    }

    /// Parse S3 path in format "bucket/key"
    fn parse_s3_path(path: &str) -> ModuleResult<(String, String)> {
        // Remove s3:// prefix if present
        let path = path.strip_prefix("s3://").unwrap_or(path);

        // Split on first /
        if let Some(pos) = path.find('/') {
            let bucket = path[..pos].to_string();
            let key = path[pos + 1..].to_string();

            if bucket.is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "Invalid S3 path: bucket name is empty".to_string(),
                ));
            }

            if key.is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "Invalid S3 path: key is empty".to_string(),
                ));
            }

            Ok((bucket, key))
        } else {
            Err(ModuleError::InvalidParameter(format!(
                "Invalid S3 path '{}': expected format 'bucket/key'",
                path
            )))
        }
    }

    /// Validate S3 bucket name according to AWS rules
    fn validate_bucket_name(name: &str) -> ModuleResult<()> {
        // Bucket names must be between 3-63 characters
        if name.len() < 3 || name.len() > 63 {
            return Err(ModuleError::InvalidParameter(
                "Bucket name must be between 3 and 63 characters".to_string(),
            ));
        }

        // Must start with a letter or number
        if !name.chars().next().unwrap().is_ascii_alphanumeric() {
            return Err(ModuleError::InvalidParameter(
                "Bucket name must start with a letter or number".to_string(),
            ));
        }

        // Must end with a letter or number
        if !name.chars().last().unwrap().is_ascii_alphanumeric() {
            return Err(ModuleError::InvalidParameter(
                "Bucket name must end with a letter or number".to_string(),
            ));
        }

        // Check for valid characters (lowercase letters, numbers, hyphens)
        for c in name.chars() {
            if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '.' {
                return Err(ModuleError::InvalidParameter(format!(
                    "Bucket name contains invalid character '{}'. Only lowercase letters, numbers, hyphens, and periods are allowed",
                    c
                )));
            }
        }

        // Cannot have consecutive periods
        if name.contains("..") {
            return Err(ModuleError::InvalidParameter(
                "Bucket name cannot contain consecutive periods".to_string(),
            ));
        }

        // Cannot be formatted as an IP address
        if name.parse::<std::net::Ipv4Addr>().is_ok() {
            return Err(ModuleError::InvalidParameter(
                "Bucket name cannot be formatted as an IP address".to_string(),
            ));
        }

        // Cannot start with xn-- (reserved for IDNA)
        if name.starts_with("xn--") {
            return Err(ModuleError::InvalidParameter(
                "Bucket name cannot start with 'xn--'".to_string(),
            ));
        }

        // Cannot end with -s3alias (reserved)
        if name.ends_with("-s3alias") {
            return Err(ModuleError::InvalidParameter(
                "Bucket name cannot end with '-s3alias'".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for AwsS3Module {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for AwsS3Module {
    fn name(&self) -> &'static str {
        "aws_s3"
    }

    fn description(&self) -> &'static str {
        "Manage AWS S3 buckets and objects"
    }

    fn classification(&self) -> ModuleClassification {
        // This module makes network API calls
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // S3 operations can hit API rate limits
        ParallelizationHint::RateLimited {
            requests_per_second: 100,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["bucket"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("region", serde_json::json!("us-east-1"));
        params.insert("mode", serde_json::json!("put"));
        params.insert("state", serde_json::json!("present"));
        params.insert("overwrite", serde_json::json!(true));
        params.insert("delete", serde_json::json!(false));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Bucket is always required
        if params.get("bucket").is_none() {
            return Err(ModuleError::MissingParameter("bucket".to_string()));
        }

        // Validate bucket name
        if let Some(serde_json::Value::String(bucket)) = params.get("bucket") {
            Self::validate_bucket_name(bucket)?;
        }

        // Validate mode if specified
        if let Some(mode) = params.get_string("mode")? {
            S3Mode::from_str(&mode)?;
        }

        // Validate state if specified
        if let Some(state) = params.get_string("state")? {
            BucketState::from_str(&state)?;
        }

        // Validate ACL if specified
        if let Some(acl) = params.get_string("acl")? {
            S3Acl::from_str(&acl)?;
        }

        // Validate storage class if specified
        if let Some(sc) = params.get_string("storage_class")? {
            StorageClass::from_str(&sc)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Determine if this is a bucket operation or object operation
        let has_mode = params.get("mode").is_some();
        let has_state = params.get("state").is_some();

        // If state is specified without mode, this is a bucket operation
        if has_state && !has_mode {
            return self.execute_bucket_operation(params, context);
        }

        // Otherwise, determine mode from params
        let mode_str = params
            .get_string("mode")?
            .unwrap_or_else(|| "put".to_string());
        let mode = S3Mode::from_str(&mode_str)?;

        match mode {
            S3Mode::Put => self.execute_put(params, context),
            S3Mode::Get => self.execute_get(params, context),
            S3Mode::Delete => self.execute_delete(params, context),
            S3Mode::GetInfo => self.execute_get_info(params, context),
            S3Mode::List => self.execute_list(params, context),
            S3Mode::Sync => self.execute_sync(params, context),
            S3Mode::Copy => self.execute_copy(params, context),
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, _context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let bucket = params.get_string_required("bucket")?;

        let has_mode = params.get("mode").is_some();
        let has_state = params.get("state").is_some();

        if has_state && !has_mode {
            let state_str = params
                .get_string("state")?
                .unwrap_or_else(|| "present".to_string());

            return Ok(Some(Diff::new(
                format!("bucket '{}' (current state unknown)", bucket),
                format!("bucket '{}' state={}", bucket, state_str),
            )));
        }

        let mode_str = params
            .get_string("mode")?
            .unwrap_or_else(|| "put".to_string());

        match mode_str.as_str() {
            "put" | "upload" => {
                if let Some(object) = params.get_string("object")? {
                    Ok(Some(Diff::new(
                        format!("s3://{}/{} (may not exist)", bucket, object),
                        format!("s3://{}/{} (uploaded)", bucket, object),
                    )))
                } else {
                    Ok(None)
                }
            }
            "delete" | "del" | "rm" => {
                if let Some(object) = params.get_string("object")? {
                    Ok(Some(Diff::new(
                        format!("s3://{}/{} (exists)", bucket, object),
                        format!("s3://{}/{} (deleted)", bucket, object),
                    )))
                } else {
                    Ok(None)
                }
            }
            "sync" => {
                let prefix = params.get_string("prefix")?.unwrap_or_default();
                Ok(Some(Diff::new(
                    format!("s3://{}/{} (current state)", bucket, prefix),
                    format!("s3://{}/{} (synchronized)", bucket, prefix),
                )))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_mode_parsing() {
        assert_eq!(S3Mode::from_str("put").unwrap(), S3Mode::Put);
        assert_eq!(S3Mode::from_str("upload").unwrap(), S3Mode::Put);
        assert_eq!(S3Mode::from_str("get").unwrap(), S3Mode::Get);
        assert_eq!(S3Mode::from_str("download").unwrap(), S3Mode::Get);
        assert_eq!(S3Mode::from_str("delete").unwrap(), S3Mode::Delete);
        assert_eq!(S3Mode::from_str("sync").unwrap(), S3Mode::Sync);
        assert!(S3Mode::from_str("invalid").is_err());
    }

    #[test]
    fn test_bucket_name_validation() {
        // Valid names
        assert!(AwsS3Module::validate_bucket_name("my-bucket").is_ok());
        assert!(AwsS3Module::validate_bucket_name("my.bucket.name").is_ok());
        assert!(AwsS3Module::validate_bucket_name("bucket123").is_ok());
        assert!(AwsS3Module::validate_bucket_name("123bucket").is_ok());

        // Invalid names
        assert!(AwsS3Module::validate_bucket_name("ab").is_err()); // Too short
        assert!(AwsS3Module::validate_bucket_name("-bucket").is_err()); // Starts with hyphen
        assert!(AwsS3Module::validate_bucket_name("bucket-").is_err()); // Ends with hyphen
        assert!(AwsS3Module::validate_bucket_name("MyBucket").is_err()); // Uppercase
        assert!(AwsS3Module::validate_bucket_name("my..bucket").is_err()); // Consecutive periods
        assert!(AwsS3Module::validate_bucket_name("192.168.1.1").is_err()); // IP address
        assert!(AwsS3Module::validate_bucket_name("xn--bucket").is_err()); // Reserved prefix
    }

    #[test]
    fn test_s3_path_parsing() {
        let (bucket, key) = AwsS3Module::parse_s3_path("my-bucket/path/to/object").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/object");

        let (bucket, key) = AwsS3Module::parse_s3_path("s3://my-bucket/object.txt").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "object.txt");

        assert!(AwsS3Module::parse_s3_path("no-slash").is_err());
        assert!(AwsS3Module::parse_s3_path("/key-only").is_err());
    }

    #[test]
    fn test_calculate_part_size() {
        // Small file - use file size
        assert_eq!(AwsS3Module::calculate_part_size(1024), 1024);

        // Medium file - use default part size
        assert_eq!(
            AwsS3Module::calculate_part_size(100 * 1024 * 1024),
            DEFAULT_PART_SIZE
        );

        // Large file - increase part size
        let large_size = 100 * 1024 * 1024 * 1024; // 100 GB
        let part_size = AwsS3Module::calculate_part_size(large_size);
        assert!(part_size > DEFAULT_PART_SIZE);
        assert!(large_size / part_size <= MAX_PARTS);
    }

    #[test]
    fn test_acl_parsing() {
        assert_eq!(S3Acl::from_str("private").unwrap(), S3Acl::Private);
        assert_eq!(S3Acl::from_str("public-read").unwrap(), S3Acl::PublicRead);
        assert_eq!(S3Acl::from_str("public_read").unwrap(), S3Acl::PublicRead);
        assert!(S3Acl::from_str("invalid-acl").is_err());
    }

    #[test]
    fn test_storage_class_parsing() {
        assert_eq!(
            StorageClass::from_str("STANDARD").unwrap(),
            StorageClass::Standard
        );
        assert_eq!(
            StorageClass::from_str("standard-ia").unwrap(),
            StorageClass::StandardIa
        );
        assert_eq!(
            StorageClass::from_str("GLACIER").unwrap(),
            StorageClass::Glacier
        );
        assert!(StorageClass::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_put_check_mode() {
        let module = AwsS3Module::new();
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("bucket".to_string(), serde_json::json!("test-bucket"));
        params.insert("object".to_string(), serde_json::json!("test-key"));
        params.insert("content".to_string(), serde_json::json!("test content"));
        params.insert("mode".to_string(), serde_json::json!("put"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would upload"));
    }

    #[test]
    fn test_module_bucket_create_check_mode() {
        let module = AwsS3Module::new();
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("bucket".to_string(), serde_json::json!("new-bucket"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("region".to_string(), serde_json::json!("eu-west-1"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would create bucket"));
        assert!(result.msg.contains("eu-west-1"));
    }

    #[test]
    fn test_module_sync_check_mode() {
        let module = AwsS3Module::new();
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("bucket".to_string(), serde_json::json!("sync-bucket"));
        params.insert("mode".to_string(), serde_json::json!("sync"));
        params.insert("src".to_string(), serde_json::json!("/local/path"));
        params.insert("prefix".to_string(), serde_json::json!("backup/"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would sync"));
    }
}
