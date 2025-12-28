//! Remote state backend implementations
//!
//! This module provides various backends for storing and retrieving provisioning state.
//! It supports local file storage, cloud storage (S3, GCS, Azure Blob), and HTTP backends
//! for Terraform Cloud compatibility.
//!
//! ## Backends
//!
//! - **LocalBackend**: File-based storage with optional file locking
//! - **S3Backend**: AWS S3 storage with optional DynamoDB locking (feature-gated)
//! - **GcsBackend**: Google Cloud Storage (stub, feature-gated)
//! - **AzureBlobBackend**: Azure Blob Storage (stub, feature-gated)
//! - **HttpBackend**: HTTP-based storage for Terraform Cloud compatibility
//!
//! ## Usage
//!
//! ```rust,ignore
//! use rustible::provisioning::state_backends::{BackendConfig, StateBackend};
//!
//! // Create a local backend
//! let config = BackendConfig::Local {
//!     path: PathBuf::from(".rustible/provisioning.state.json"),
//! };
//! let backend = config.create_backend().await?;
//!
//! // Load state
//! if let Some(state) = backend.load().await? {
//!     println!("Loaded {} resources", state.resources.len());
//! }
//!
//! // Save state
//! backend.save(&state).await?;
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::ProvisioningState;
use super::state_lock::{FileLock, LockBackend, LockInfo};

// ============================================================================
// State Backend Trait
// ============================================================================

/// Trait for state storage backends
#[async_trait]
pub trait StateBackend: Send + Sync {
    /// Get the backend name
    fn name(&self) -> &str;

    /// Load state from backend
    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>>;

    /// Save state to backend
    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()>;

    /// Delete state from backend
    async fn delete(&self) -> ProvisioningResult<()>;

    /// Check if state exists
    async fn exists(&self) -> ProvisioningResult<bool>;

    /// Get lock backend (if supported)
    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>>;
}

// ============================================================================
// Local Backend
// ============================================================================

/// Local file-based state backend
pub struct LocalBackend {
    /// Path to the state file
    state_path: PathBuf,
    /// File-based lock
    file_lock: Arc<FileLock>,
}

impl LocalBackend {
    /// Create a new local backend
    pub fn new(state_path: PathBuf) -> Self {
        let file_lock = Arc::new(FileLock::for_state_file(&state_path));
        Self {
            state_path,
            file_lock,
        }
    }

    /// Get the state file path
    pub fn state_path(&self) -> &PathBuf {
        &self.state_path
    }
}

#[async_trait]
impl StateBackend for LocalBackend {
    fn name(&self) -> &str {
        "local"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        if !self.state_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&self.state_path)
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to read state file: {}",
                    e
                ))
            })?;

        let state: ProvisioningState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.state_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to create state directory: {}",
                    e
                ))
            })?;
        }

        // Serialize state
        let content = serde_json::to_string_pretty(state)?;

        // Write atomically using temp file
        let temp_path = self.state_path.with_extension("tmp");
        tokio::fs::write(&temp_path, &content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to write state file: {}", e))
        })?;

        tokio::fs::rename(&temp_path, &self.state_path)
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to finalize state file: {}",
                    e
                ))
            })?;

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        if self.state_path.exists() {
            tokio::fs::remove_file(&self.state_path)
                .await
                .map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to delete state file: {}",
                        e
                    ))
                })?;
        }

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        Ok(self.state_path.exists())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        Some(Arc::clone(&self.file_lock) as Arc<dyn LockBackend>)
    }
}

// ============================================================================
// S3 Backend (AWS feature-gated)
// ============================================================================

/// S3-based state backend for AWS
#[cfg(feature = "aws")]
pub struct S3Backend {
    /// S3 bucket name
    bucket: String,
    /// Object key (path within bucket)
    key: String,
    /// AWS region
    region: String,
    /// Whether to use server-side encryption
    encrypt: bool,
    /// DynamoDB table for state locking (optional)
    dynamodb_table: Option<String>,
    /// AWS S3 client
    client: aws_sdk_s3::Client,
}

#[cfg(feature = "aws")]
impl S3Backend {
    /// Create a new S3 backend
    pub async fn new(
        bucket: String,
        key: String,
        region: String,
        encrypt: bool,
        dynamodb_table: Option<String>,
    ) -> ProvisioningResult<Self> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.clone()))
            .load()
            .await;

        let client = aws_sdk_s3::Client::new(&config);

        Ok(Self {
            bucket,
            key,
            region,
            encrypt,
            dynamodb_table,
            client,
        })
    }

    /// Get the bucket name
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Get the object key
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Get the region
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Get the DynamoDB table name (if configured for locking)
    pub fn dynamodb_table(&self) -> Option<&str> {
        self.dynamodb_table.as_deref()
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl StateBackend for S3Backend {
    fn name(&self) -> &str {
        "s3"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await;

        match result {
            Ok(output) => {
                let bytes = output.body.collect().await.map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to read S3 object body: {}",
                        e
                    ))
                })?;

                let content = String::from_utf8(bytes.to_vec()).map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Invalid UTF-8 in state file: {}",
                        e
                    ))
                })?;

                let state: ProvisioningState = serde_json::from_str(&content)?;
                Ok(Some(state))
            }
            Err(sdk_err) => {
                // Check if it's a "not found" error
                let err_str = sdk_err.to_string();
                if err_str.contains("NoSuchKey") || err_str.contains("Not Found") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to get S3 object: {}",
                        sdk_err
                    )))
                }
            }
        }
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        let content = serde_json::to_string_pretty(state)?;

        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .body(content.into_bytes().into())
            .content_type("application/json");

        if self.encrypt {
            request =
                request.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
        }

        request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to put S3 object: {}", e))
        })?;

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete S3 object: {}", e))
            })?;

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("NotFound") || err_str.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to check S3 object: {}",
                        e
                    )))
                }
            }
        }
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        // DynamoDB locking requires the dynamodb_table to be configured
        if let Some(ref table) = self.dynamodb_table {
            // Return DynamoDB lock backend
            Some(Arc::new(super::state_lock::DynamoDbLock::new(
                table.clone(),
                format!("{}/{}", self.bucket, self.key),
            )))
        } else {
            None
        }
    }
}

// ============================================================================
// GCS Backend (GCP feature-gated stub)
// ============================================================================

/// Google Cloud Storage state backend (stub implementation)
#[cfg(feature = "gcp")]
pub struct GcsBackend {
    /// GCS bucket name
    bucket: String,
    /// Object prefix/path
    prefix: String,
}

#[cfg(feature = "gcp")]
impl GcsBackend {
    /// Create a new GCS backend
    pub fn new(bucket: String, prefix: String) -> Self {
        Self { bucket, prefix }
    }

    /// Get the bucket name
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Get the prefix
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl StateBackend for GcsBackend {
    fn name(&self) -> &str {
        "gcs"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "gcp".to_string(),
            message: "GCS backend not yet implemented".to_string(),
        })
    }

    async fn save(&self, _state: &ProvisioningState) -> ProvisioningResult<()> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "gcp".to_string(),
            message: "GCS backend not yet implemented".to_string(),
        })
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "gcp".to_string(),
            message: "GCS backend not yet implemented".to_string(),
        })
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "gcp".to_string(),
            message: "GCS backend not yet implemented".to_string(),
        })
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        None
    }
}

// ============================================================================
// Azure Blob Backend (Azure feature-gated stub)
// ============================================================================

/// Azure Blob Storage state backend (stub implementation)
#[cfg(feature = "azure")]
pub struct AzureBlobBackend {
    /// Storage container name
    container: String,
    /// Blob name
    blob_name: String,
}

#[cfg(feature = "azure")]
impl AzureBlobBackend {
    /// Create a new Azure Blob backend
    pub fn new(container: String, blob_name: String) -> Self {
        Self {
            container,
            blob_name,
        }
    }

    /// Get the container name
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Get the blob name
    pub fn blob_name(&self) -> &str {
        &self.blob_name
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl StateBackend for AzureBlobBackend {
    fn name(&self) -> &str {
        "azurerm"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "azure".to_string(),
            message: "Azure Blob backend not yet implemented".to_string(),
        })
    }

    async fn save(&self, _state: &ProvisioningState) -> ProvisioningResult<()> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "azure".to_string(),
            message: "Azure Blob backend not yet implemented".to_string(),
        })
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "azure".to_string(),
            message: "Azure Blob backend not yet implemented".to_string(),
        })
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "azure".to_string(),
            message: "Azure Blob backend not yet implemented".to_string(),
        })
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        None
    }
}

// ============================================================================
// HTTP Backend (Terraform Cloud compatible)
// ============================================================================

/// HTTP-based state backend for Terraform Cloud compatibility
pub struct HttpBackend {
    /// Base URL for state operations
    address: String,
    /// URL for lock operations
    lock_address: Option<String>,
    /// URL for unlock operations
    unlock_address: Option<String>,
    /// Basic auth username
    username: Option<String>,
    /// Basic auth password
    password: Option<String>,
    /// HTTP client
    client: reqwest::Client,
    /// Request timeout
    timeout: Duration,
}

impl HttpBackend {
    /// Create a new HTTP backend
    pub fn new(address: String) -> Self {
        Self {
            address,
            lock_address: None,
            unlock_address: None,
            username: None,
            password: None,
            client: reqwest::Client::new(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Set lock URL
    pub fn with_lock_address(mut self, address: impl Into<String>) -> Self {
        self.lock_address = Some(address.into());
        self
    }

    /// Set unlock URL
    pub fn with_unlock_address(mut self, address: impl Into<String>) -> Self {
        self.unlock_address = Some(address.into());
        self
    }

    /// Set basic auth credentials
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build a request with auth if configured
    fn build_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.client.request(method, url).timeout(self.timeout);

        if let (Some(username), Some(password)) = (&self.username, &self.password) {
            request = request.basic_auth(username, Some(password));
        }

        request
    }
}

#[async_trait]
impl StateBackend for HttpBackend {
    fn name(&self) -> &str {
        "http"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        let response = self
            .build_request(reqwest::Method::GET, &self.address)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("HTTP GET failed: {}", e))
            })?;

        match response.status() {
            status if status.is_success() => {
                let content = response.text().await.map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to read response body: {}",
                        e
                    ))
                })?;

                if content.is_empty() {
                    return Ok(None);
                }

                let state: ProvisioningState = serde_json::from_str(&content)?;
                Ok(Some(state))
            }
            status if status == reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => Err(ProvisioningError::StatePersistenceError(format!(
                "HTTP GET returned status {}",
                status
            ))),
        }
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        let content = serde_json::to_string(state)?;

        let response = self
            .build_request(reqwest::Method::POST, &self.address)
            .header("Content-Type", "application/json")
            .body(content)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("HTTP POST failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ProvisioningError::StatePersistenceError(format!(
                "HTTP POST returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        let response = self
            .build_request(reqwest::Method::DELETE, &self.address)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("HTTP DELETE failed: {}", e))
            })?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(ProvisioningError::StatePersistenceError(format!(
                "HTTP DELETE returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        let response = self
            .build_request(reqwest::Method::HEAD, &self.address)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("HTTP HEAD failed: {}", e))
            })?;

        Ok(response.status().is_success())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        if self.lock_address.is_some() {
            Some(Arc::new(HttpLockBackend {
                lock_address: self.lock_address.clone(),
                unlock_address: self.unlock_address.clone(),
                username: self.username.clone(),
                password: self.password.clone(),
                client: self.client.clone(),
                timeout: self.timeout,
            }))
        } else {
            None
        }
    }
}

/// HTTP-based lock backend
struct HttpLockBackend {
    lock_address: Option<String>,
    unlock_address: Option<String>,
    username: Option<String>,
    password: Option<String>,
    client: reqwest::Client,
    timeout: Duration,
}

impl HttpLockBackend {
    fn build_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.client.request(method, url).timeout(self.timeout);

        if let (Some(username), Some(password)) = (&self.username, &self.password) {
            request = request.basic_auth(username, Some(password));
        }

        request
    }
}

#[async_trait]
impl LockBackend for HttpLockBackend {
    async fn acquire(&self, info: &LockInfo, _timeout: Duration) -> ProvisioningResult<bool> {
        let lock_address = self.lock_address.as_ref().ok_or_else(|| {
            ProvisioningError::ConcurrencyError("Lock address not configured".to_string())
        })?;

        let content = serde_json::to_string(info)?;

        let response = self
            .build_request(reqwest::Method::PUT, lock_address)
            .header("Content-Type", "application/json")
            .body(content)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lock request failed: {}", e))
            })?;

        if response.status() == reqwest::StatusCode::CONFLICT
            || response.status() == reqwest::StatusCode::LOCKED
        {
            return Ok(false);
        }

        if !response.status().is_success() {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Lock request returned status {}",
                response.status()
            )));
        }

        Ok(true)
    }

    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool> {
        let unlock_address = self.unlock_address.as_ref().ok_or_else(|| {
            ProvisioningError::ConcurrencyError("Unlock address not configured".to_string())
        })?;

        let response = self
            .build_request(reqwest::Method::DELETE, unlock_address)
            .header("Lock-ID", lock_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Unlock request failed: {}", e))
            })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }

        if !response.status().is_success() {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Unlock request returned status {}",
                response.status()
            )));
        }

        Ok(true)
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        let lock_address = self.lock_address.as_ref().ok_or_else(|| {
            ProvisioningError::ConcurrencyError("Lock address not configured".to_string())
        })?;

        let response = self
            .build_request(reqwest::Method::GET, lock_address)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lock info request failed: {}", e))
            })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Lock info request returned status {}",
                response.status()
            )));
        }

        let content = response.text().await.map_err(|e| {
            ProvisioningError::ConcurrencyError(format!("Failed to read lock info: {}", e))
        })?;

        if content.is_empty() {
            return Ok(None);
        }

        let info: LockInfo = serde_json::from_str(&content)?;
        Ok(Some(info))
    }

    async fn force_unlock(&self, lock_id: &str) -> ProvisioningResult<()> {
        let unlock_address = self.unlock_address.as_ref().ok_or_else(|| {
            ProvisioningError::ConcurrencyError("Unlock address not configured".to_string())
        })?;

        let response = self
            .build_request(reqwest::Method::DELETE, unlock_address)
            .header("Lock-ID", lock_id)
            .header("Force", "true")
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Force unlock failed: {}", e))
            })?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Force unlock returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    fn backend_name(&self) -> &str {
        "http"
    }
}

// ============================================================================
// Backend Configuration
// ============================================================================

/// Configuration for state backends
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendConfig {
    /// Local file-based backend
    Local {
        /// Path to state file
        path: PathBuf,
    },
    /// AWS S3 backend
    S3 {
        /// S3 bucket name
        bucket: String,
        /// Object key in bucket
        key: String,
        /// AWS region
        region: String,
        /// Enable server-side encryption
        #[serde(default)]
        encrypt: bool,
        /// DynamoDB table for locking
        dynamodb_table: Option<String>,
    },
    /// Google Cloud Storage backend
    Gcs {
        /// GCS bucket name
        bucket: String,
        /// Object prefix
        prefix: String,
    },
    /// Azure Blob Storage backend
    AzureBlob {
        /// Storage container
        container: String,
        /// Blob name
        name: String,
    },
    /// HTTP backend (Terraform Cloud compatible)
    Http {
        /// State URL
        address: String,
        /// Lock URL
        lock_address: Option<String>,
        /// Unlock URL
        unlock_address: Option<String>,
        /// Basic auth username
        username: Option<String>,
        /// Basic auth password
        password: Option<String>,
    },
}

impl BackendConfig {
    /// Create a backend from this configuration
    pub async fn create_backend(&self) -> ProvisioningResult<Box<dyn StateBackend>> {
        match self {
            BackendConfig::Local { path } => Ok(Box::new(LocalBackend::new(path.clone()))),
            #[cfg(feature = "aws")]
            BackendConfig::S3 {
                bucket,
                key,
                region,
                encrypt,
                dynamodb_table,
            } => {
                let backend = S3Backend::new(
                    bucket.clone(),
                    key.clone(),
                    region.clone(),
                    *encrypt,
                    dynamodb_table.clone(),
                )
                .await?;
                Ok(Box::new(backend))
            }
            #[cfg(not(feature = "aws"))]
            BackendConfig::S3 { .. } => Err(ProvisioningError::ProviderConfigError {
                provider: "aws".to_string(),
                message: "S3 backend requires the 'aws' feature".to_string(),
            }),
            #[cfg(feature = "gcp")]
            BackendConfig::Gcs { bucket, prefix } => {
                Ok(Box::new(GcsBackend::new(bucket.clone(), prefix.clone())))
            }
            #[cfg(not(feature = "gcp"))]
            BackendConfig::Gcs { .. } => Err(ProvisioningError::ProviderConfigError {
                provider: "gcp".to_string(),
                message: "GCS backend requires the 'gcp' feature".to_string(),
            }),
            #[cfg(feature = "azure")]
            BackendConfig::AzureBlob { container, name } => Ok(Box::new(AzureBlobBackend::new(
                container.clone(),
                name.clone(),
            ))),
            #[cfg(not(feature = "azure"))]
            BackendConfig::AzureBlob { .. } => Err(ProvisioningError::ProviderConfigError {
                provider: "azure".to_string(),
                message: "Azure Blob backend requires the 'azure' feature".to_string(),
            }),
            BackendConfig::Http {
                address,
                lock_address,
                unlock_address,
                username,
                password,
            } => {
                let mut backend = HttpBackend::new(address.clone());

                if let Some(lock_addr) = lock_address {
                    backend = backend.with_lock_address(lock_addr);
                }

                if let Some(unlock_addr) = unlock_address {
                    backend = backend.with_unlock_address(unlock_addr);
                }

                if let (Some(user), Some(pass)) = (username, password) {
                    backend = backend.with_auth(user, pass);
                }

                Ok(Box::new(backend))
            }
        }
    }

    /// Create a default local backend configuration
    pub fn local_default() -> Self {
        BackendConfig::Local {
            path: PathBuf::from(".rustible/provisioning.state.json"),
        }
    }
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::local_default()
    }
}

// ============================================================================
// Memory Backend (for testing)
// ============================================================================

/// In-memory state backend for testing
pub struct MemoryBackend {
    state: Arc<RwLock<Option<ProvisioningState>>>,
    lock: Arc<super::state_lock::InMemoryLock>,
}

impl MemoryBackend {
    /// Create a new in-memory backend
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
            lock: Arc::new(super::state_lock::InMemoryLock::new()),
        }
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateBackend for MemoryBackend {
    fn name(&self) -> &str {
        "memory"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        Ok(self.state.read().clone())
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        *self.state.write() = Some(state.clone());
        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        *self.state.write() = None;
        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        Ok(self.state.read().is_some())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        Some(Arc::clone(&self.lock) as Arc<dyn LockBackend>)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::state::{ResourceId, ResourceState};
    use tempfile::TempDir;
    use tokio::time::Duration;

    fn create_test_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();
        let resource = ResourceState::new(
            ResourceId::new("aws_vpc", "test"),
            "vpc-123",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({"id": "vpc-123"}),
        );
        state.add_resource(resource);
        state
    }

    #[tokio::test]
    async fn test_local_backend_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");
        let backend = LocalBackend::new(state_path.clone());

        // Initially no state exists
        assert!(!backend.exists().await.unwrap());

        // Save state
        let state = create_test_state();
        backend.save(&state).await.unwrap();

        // State should exist now
        assert!(backend.exists().await.unwrap());

        // Load state back
        let loaded = backend.load().await.unwrap();
        assert!(loaded.is_some());

        let loaded_state = loaded.unwrap();
        assert_eq!(loaded_state.lineage, state.lineage);
        assert_eq!(loaded_state.resources.len(), 1);
    }

    #[tokio::test]
    async fn test_local_backend_delete() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");
        let backend = LocalBackend::new(state_path.clone());

        // Save state
        let state = create_test_state();
        backend.save(&state).await.unwrap();
        assert!(backend.exists().await.unwrap());

        // Delete state
        backend.delete().await.unwrap();
        assert!(!backend.exists().await.unwrap());

        // Load returns None
        let loaded = backend.load().await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_local_backend_has_lock() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");
        let backend = LocalBackend::new(state_path);

        let lock_backend = backend.lock_backend();
        assert!(lock_backend.is_some());
        assert_eq!(lock_backend.unwrap().backend_name(), "file");
    }

    #[tokio::test]
    async fn test_memory_backend() {
        let backend = MemoryBackend::new();

        // Initially empty
        assert!(!backend.exists().await.unwrap());
        assert!(backend.load().await.unwrap().is_none());

        // Save state
        let state = create_test_state();
        backend.save(&state).await.unwrap();

        // Load state
        assert!(backend.exists().await.unwrap());
        let loaded = backend.load().await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().resources.len(), 1);

        // Delete state
        backend.delete().await.unwrap();
        assert!(!backend.exists().await.unwrap());
    }

    #[tokio::test]
    async fn test_memory_backend_locking() {
        let backend = MemoryBackend::new();
        let lock_backend = backend.lock_backend().unwrap();

        // Lock
        let lock_info = LockInfo::new("apply");
        assert!(lock_backend
            .acquire(&lock_info, Duration::from_millis(10))
            .await
            .unwrap());

        // Can't double lock
        let lock_info2 = LockInfo::new("destroy");
        assert!(!lock_backend
            .acquire(&lock_info2, Duration::from_millis(10))
            .await
            .unwrap());

        // Release
        assert!(lock_backend.release(&lock_info.id).await.unwrap());

        // Now second lock works
        assert!(lock_backend
            .acquire(&lock_info2, Duration::from_millis(10))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_backend_config_local() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let config = BackendConfig::Local {
            path: state_path.clone(),
        };

        let backend = config.create_backend().await.unwrap();
        assert_eq!(backend.name(), "local");

        // Test it works
        let state = create_test_state();
        backend.save(&state).await.unwrap();
        assert!(backend.exists().await.unwrap());
    }

    #[tokio::test]
    async fn test_backend_config_http() {
        let config = BackendConfig::Http {
            address: "http://example.com/state".to_string(),
            lock_address: Some("http://example.com/lock".to_string()),
            unlock_address: Some("http://example.com/unlock".to_string()),
            username: None,
            password: None,
        };

        let backend = config.create_backend().await.unwrap();
        assert_eq!(backend.name(), "http");

        // Lock backend should be available
        assert!(backend.lock_backend().is_some());
    }

    #[tokio::test]
    async fn test_local_backend_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");
        let backend = LocalBackend::new(state_path.clone());

        // Save state
        let state = create_test_state();
        backend.save(&state).await.unwrap();

        // Verify no .tmp file remains
        let tmp_path = state_path.with_extension("tmp");
        assert!(!tmp_path.exists());

        // State file should exist and be valid JSON
        let content = std::fs::read_to_string(&state_path).unwrap();
        let _: ProvisioningState = serde_json::from_str(&content).unwrap();
    }

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        match config {
            BackendConfig::Local { path } => {
                assert_eq!(path, PathBuf::from(".rustible/provisioning.state.json"));
            }
            _ => panic!("Expected local backend"),
        }
    }

    #[tokio::test]
    async fn test_local_backend_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("nested/dir/state.json");
        let backend = LocalBackend::new(state_path.clone());

        // Parent directory doesn't exist yet
        assert!(!state_path.parent().unwrap().exists());

        // Save should create parent directories
        let state = create_test_state();
        backend.save(&state).await.unwrap();

        // Parent directory should now exist
        assert!(state_path.parent().unwrap().exists());
        assert!(state_path.exists());
    }

    #[test]
    fn test_backend_config_serialization() {
        let config = BackendConfig::S3 {
            bucket: "my-bucket".to_string(),
            key: "terraform.tfstate".to_string(),
            region: "us-east-1".to_string(),
            encrypt: true,
            dynamodb_table: Some("terraform-locks".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

        match deserialized {
            BackendConfig::S3 {
                bucket,
                key,
                region,
                encrypt,
                dynamodb_table,
            } => {
                assert_eq!(bucket, "my-bucket");
                assert_eq!(key, "terraform.tfstate");
                assert_eq!(region, "us-east-1");
                assert!(encrypt);
                assert_eq!(dynamodb_table, Some("terraform-locks".to_string()));
            }
            _ => panic!("Expected S3 config"),
        }
    }

    #[tokio::test]
    async fn test_http_backend_creation() {
        let backend = HttpBackend::new("http://localhost:8080/state".to_string())
            .with_lock_address("http://localhost:8080/lock")
            .with_unlock_address("http://localhost:8080/unlock")
            .with_auth("user", "pass")
            .with_timeout(Duration::from_secs(60));

        assert_eq!(backend.name(), "http");
        assert!(backend.lock_backend().is_some());
    }
}
