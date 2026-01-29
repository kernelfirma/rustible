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
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
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
//! # Ok(())
//! # }
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
    /// DynamoDB client for locking (optional)
    dynamodb_client: Option<aws_sdk_dynamodb::Client>,
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
        let dynamodb_client = dynamodb_table
            .as_ref()
            .map(|_| aws_sdk_dynamodb::Client::new(&config));

        Ok(Self {
            bucket,
            key,
            region,
            encrypt,
            dynamodb_table,
            dynamodb_client,
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
            if let Some(ref client) = self.dynamodb_client {
                Some(Arc::new(super::state_lock::DynamoDbLock::new(
                    table.clone(),
                    format!("{}/{}", self.bucket, self.key),
                    client.clone(),
                )))
            } else {
                None
            }
        } else {
            None
        }
    }
}

// ============================================================================
// GCS Backend (GCP feature-gated)
// ============================================================================

/// Google Cloud Storage state backend using JSON API
///
/// Authentication is handled via:
/// 1. GOOGLE_APPLICATION_CREDENTIALS environment variable (service account JSON)
/// 2. gcloud CLI default credentials
/// 3. GCE metadata service (when running on GCP)
#[cfg(feature = "gcp")]
pub struct GcsBackend {
    /// GCS bucket name
    bucket: String,
    /// Object key (path within bucket)
    key: String,
    /// HTTP client
    client: reqwest::Client,
    /// Request timeout
    timeout: Duration,
    /// Access token (cached)
    access_token: Arc<RwLock<Option<String>>>,
}

#[cfg(feature = "gcp")]
impl GcsBackend {
    /// Create a new GCS backend
    pub fn new(bucket: String, key: String) -> Self {
        Self {
            bucket,
            key,
            client: reqwest::Client::new(),
            timeout: Duration::from_secs(30),
            access_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get the bucket name
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Get the object key
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Get GCS object URL
    fn object_url(&self) -> String {
        format!(
            "https://storage.googleapis.com/storage/v1/b/{}/o/{}",
            self.bucket,
            urlencoding::encode(&self.key)
        )
    }

    /// Get GCS upload URL
    fn upload_url(&self) -> String {
        format!(
            "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}",
            self.bucket,
            urlencoding::encode(&self.key)
        )
    }

    /// Get GCS download URL
    fn download_url(&self) -> String {
        format!("{}?alt=media", self.object_url())
    }

    /// Get access token from environment or metadata service
    async fn get_access_token(&self) -> ProvisioningResult<String> {
        // Check cached token
        if let Some(token) = self.access_token.read().as_ref() {
            return Ok(token.clone());
        }

        // Try GOOGLE_APPLICATION_CREDENTIALS
        if let Ok(creds_path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            if let Ok(token) = self.get_token_from_service_account(&creds_path).await {
                *self.access_token.write() = Some(token.clone());
                return Ok(token);
            }
        }

        // Try gcloud CLI default credentials
        if let Ok(token) = self.get_token_from_gcloud().await {
            *self.access_token.write() = Some(token.clone());
            return Ok(token);
        }

        // Try GCE metadata service
        if let Ok(token) = self.get_token_from_metadata().await {
            *self.access_token.write() = Some(token.clone());
            return Ok(token);
        }

        Err(ProvisioningError::CloudApiError(
            "Failed to get GCS access token. Set GOOGLE_APPLICATION_CREDENTIALS or run 'gcloud auth application-default login'".to_string()
        ))
    }

    /// Get token from service account JSON file
    async fn get_token_from_service_account(
        &self,
        path: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let content = tokio::fs::read_to_string(path).await?;
        let creds: serde_json::Value = serde_json::from_str(&content)?;

        // For service accounts, we'd normally use JWT authentication
        // For simplicity, return the client_email to indicate service account is configured
        // In production, this would involve creating a signed JWT
        if creds.get("type").and_then(|v| v.as_str()) == Some("service_account") {
            // Use token endpoint with JWT assertion
            // This is a simplified version - full implementation would use JWT signing
            Err("Service account JWT authentication requires additional implementation".into())
        } else {
            Err("Invalid credentials file format".into())
        }
    }

    /// Get token from gcloud CLI
    async fn get_token_from_gcloud(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let output = tokio::process::Command::new("gcloud")
            .args(["auth", "application-default", "print-access-token"])
            .output()
            .await?;

        if output.status.success() {
            let token = String::from_utf8(output.stdout)?.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        Err("gcloud command failed".into())
    }

    /// Get token from GCE metadata service
    async fn get_token_from_metadata(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .timeout(Duration::from_secs(2))
            .send()
            .await?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json().await?;
            if let Some(token) = data.get("access_token").and_then(|v| v.as_str()) {
                return Ok(token.to_string());
            }
        }

        Err("Metadata service unavailable".into())
    }

    /// Build authenticated request
    async fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> ProvisioningResult<reqwest::RequestBuilder> {
        let token = self.get_access_token().await?;
        Ok(self
            .client
            .request(method, url)
            .timeout(self.timeout)
            .bearer_auth(token))
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl StateBackend for GcsBackend {
    fn name(&self) -> &str {
        "gcs"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        let request = self
            .build_request(reqwest::Method::GET, &self.download_url())
            .await?;
        let response = request
            .send()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("GCS GET failed: {}", e)))?;

        match response.status() {
            status if status.is_success() => {
                let content = response.text().await.map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to read GCS response: {}",
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
            status => Err(ProvisioningError::CloudApiError(format!(
                "GCS GET returned status {}",
                status
            ))),
        }
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        let content = serde_json::to_string_pretty(state)?;

        let request = self
            .build_request(reqwest::Method::POST, &self.upload_url())
            .await?
            .header("Content-Type", "application/json")
            .body(content);

        let response = request
            .send()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("GCS upload failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProvisioningError::CloudApiError(format!(
                "GCS upload returned status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        let request = self
            .build_request(reqwest::Method::DELETE, &self.object_url())
            .await?;
        let response = request
            .send()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("GCS DELETE failed: {}", e)))?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(ProvisioningError::CloudApiError(format!(
                "GCS DELETE returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        let request = self
            .build_request(reqwest::Method::GET, &self.object_url())
            .await?;
        let response = request
            .send()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("GCS HEAD failed: {}", e)))?;

        Ok(response.status().is_success())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        // GCS uses generation numbers for optimistic locking
        // For now, return None - could implement using a separate lock object
        None
    }
}

// ============================================================================
// Azure Blob Backend (Azure feature-gated)
// ============================================================================

/// Azure Blob Storage state backend using REST API
///
/// Authentication is handled via:
/// 1. AZURE_STORAGE_KEY environment variable (storage account key)
/// 2. AZURE_STORAGE_CONNECTION_STRING environment variable
/// 3. AZURE_STORAGE_SAS_TOKEN environment variable (SAS token)
/// 4. Azure CLI default credentials (via `az account get-access-token`)
#[cfg(feature = "azure")]
pub struct AzureBlobBackend {
    /// Storage account name
    storage_account: String,
    /// Storage container name
    container: String,
    /// Blob name
    blob_name: String,
    /// HTTP client
    client: reqwest::Client,
    /// Request timeout
    timeout: Duration,
    /// Use Azure Blob lease for locking
    use_lease_lock: bool,
    /// Current lease ID (if locked)
    lease_id: Arc<RwLock<Option<String>>>,
}

#[cfg(feature = "azure")]
impl AzureBlobBackend {
    /// Create a new Azure Blob backend
    pub fn new(storage_account: String, container: String, blob_name: String) -> Self {
        Self {
            storage_account,
            container,
            blob_name,
            client: reqwest::Client::new(),
            timeout: Duration::from_secs(30),
            use_lease_lock: true,
            lease_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Disable lease-based locking
    pub fn without_lease_lock(mut self) -> Self {
        self.use_lease_lock = false;
        self
    }

    /// Get the storage account name
    pub fn storage_account(&self) -> &str {
        &self.storage_account
    }

    /// Get the container name
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Get the blob name
    pub fn blob_name(&self) -> &str {
        &self.blob_name
    }

    /// Get blob URL
    fn blob_url(&self) -> String {
        format!(
            "https://{}.blob.core.windows.net/{}/{}",
            self.storage_account, self.container, self.blob_name
        )
    }

    /// Get authorization header using available credentials
    async fn get_auth_header(&self) -> ProvisioningResult<(String, String)> {
        // Try SAS token first (simplest)
        if let Ok(sas_token) = std::env::var("AZURE_STORAGE_SAS_TOKEN") {
            // SAS token is appended to URL, not in header
            return Ok(("x-ms-version".to_string(), "2021-06-08".to_string()));
        }

        // Try storage account key
        if let Ok(storage_key) = std::env::var("AZURE_STORAGE_KEY") {
            // Would need to compute HMAC-SHA256 signature
            // For now, return version header - real implementation needs SharedKey auth
            return Ok(("x-ms-version".to_string(), "2021-06-08".to_string()));
        }

        // Try Azure CLI
        if let Ok(token) = self.get_token_from_azure_cli().await {
            return Ok(("Authorization".to_string(), format!("Bearer {}", token)));
        }

        Err(ProvisioningError::CloudApiError(
            "Failed to get Azure credentials. Set AZURE_STORAGE_KEY, AZURE_STORAGE_SAS_TOKEN, or run 'az login'".to_string()
        ))
    }

    /// Get SAS token suffix for URL
    fn get_sas_suffix(&self) -> String {
        if let Ok(sas_token) = std::env::var("AZURE_STORAGE_SAS_TOKEN") {
            if sas_token.starts_with('?') {
                sas_token
            } else {
                format!("?{}", sas_token)
            }
        } else {
            String::new()
        }
    }

    /// Get token from Azure CLI
    async fn get_token_from_azure_cli(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let output = tokio::process::Command::new("az")
            .args([
                "account",
                "get-access-token",
                "--resource",
                "https://storage.azure.com/",
                "--query",
                "accessToken",
                "-o",
                "tsv",
            ])
            .output()
            .await?;

        if output.status.success() {
            let token = String::from_utf8(output.stdout)?.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        Err("Azure CLI command failed".into())
    }

    /// Build authenticated request
    async fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> ProvisioningResult<reqwest::RequestBuilder> {
        let url_with_sas = format!("{}{}", url, self.get_sas_suffix());
        let (header_name, header_value) = self.get_auth_header().await?;

        let mut request = self
            .client
            .request(method, &url_with_sas)
            .timeout(self.timeout)
            .header("x-ms-version", "2021-06-08");

        if header_name == "Authorization" {
            request = request.header("Authorization", header_value);
        }

        // Add lease ID if we have one
        if let Some(ref lease_id) = *self.lease_id.read() {
            request = request.header("x-ms-lease-id", lease_id);
        }

        Ok(request)
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl StateBackend for AzureBlobBackend {
    fn name(&self) -> &str {
        "azurerm"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        let request = self
            .build_request(reqwest::Method::GET, &self.blob_url())
            .await?;
        let response = request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Azure Blob GET failed: {}", e))
        })?;

        match response.status() {
            status if status.is_success() => {
                let content = response.text().await.map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to read Azure response: {}",
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
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(ProvisioningError::CloudApiError(format!(
                    "Azure Blob GET returned status {}: {}",
                    status, body
                )))
            }
        }
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        let content = serde_json::to_string_pretty(state)?;

        let request = self
            .build_request(reqwest::Method::PUT, &self.blob_url())
            .await?
            .header("Content-Type", "application/json")
            .header("x-ms-blob-type", "BlockBlob")
            .body(content);

        let response = request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Azure Blob PUT failed: {}", e))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProvisioningError::CloudApiError(format!(
                "Azure Blob PUT returned status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        let request = self
            .build_request(reqwest::Method::DELETE, &self.blob_url())
            .await?;
        let response = request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Azure Blob DELETE failed: {}", e))
        })?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(ProvisioningError::CloudApiError(format!(
                "Azure Blob DELETE returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        let request = self
            .build_request(reqwest::Method::HEAD, &self.blob_url())
            .await?;
        let response = request.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Azure Blob HEAD failed: {}", e))
        })?;

        Ok(response.status().is_success())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        if self.use_lease_lock {
            Some(Arc::new(AzureBlobLeaseLock {
                storage_account: self.storage_account.clone(),
                container: self.container.clone(),
                blob_name: self.blob_name.clone(),
                client: self.client.clone(),
                timeout: self.timeout,
                lease_id: Arc::clone(&self.lease_id),
            }))
        } else {
            None
        }
    }
}

/// Azure Blob lease-based lock backend
#[cfg(feature = "azure")]
struct AzureBlobLeaseLock {
    storage_account: String,
    container: String,
    blob_name: String,
    client: reqwest::Client,
    timeout: Duration,
    lease_id: Arc<RwLock<Option<String>>>,
}

#[cfg(feature = "azure")]
impl AzureBlobLeaseLock {
    fn blob_url(&self) -> String {
        format!(
            "https://{}.blob.core.windows.net/{}/{}",
            self.storage_account, self.container, self.blob_name
        )
    }

    fn get_sas_suffix(&self) -> String {
        if let Ok(sas_token) = std::env::var("AZURE_STORAGE_SAS_TOKEN") {
            if sas_token.starts_with('?') {
                sas_token
            } else {
                format!("?{}", sas_token)
            }
        } else {
            String::new()
        }
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl LockBackend for AzureBlobLeaseLock {
    async fn acquire(&self, info: &LockInfo, timeout: Duration) -> ProvisioningResult<bool> {
        let url = format!(
            "{}?comp=lease{}",
            self.blob_url(),
            self.get_sas_suffix().replace('?', "&")
        );

        let response = self
            .client
            .put(&url)
            .timeout(self.timeout)
            .header("x-ms-version", "2021-06-08")
            .header("x-ms-lease-action", "acquire")
            .header(
                "x-ms-lease-duration",
                timeout.as_secs().min(60).max(15).to_string(),
            )
            .header("x-ms-proposed-lease-id", &info.id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lease acquire failed: {}", e))
            })?;

        if response.status().is_success() {
            // Extract lease ID from response
            if let Some(lease_id) = response.headers().get("x-ms-lease-id") {
                if let Ok(id) = lease_id.to_str() {
                    *self.lease_id.write() = Some(id.to_string());
                    return Ok(true);
                }
            }
            *self.lease_id.write() = Some(info.id.clone());
            return Ok(true);
        }

        if response.status() == reqwest::StatusCode::CONFLICT {
            return Ok(false); // Already leased
        }

        Err(ProvisioningError::ConcurrencyError(format!(
            "Lease acquire returned status {}",
            response.status()
        )))
    }

    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool> {
        let url = format!(
            "{}?comp=lease{}",
            self.blob_url(),
            self.get_sas_suffix().replace('?', "&")
        );

        let response = self
            .client
            .put(&url)
            .timeout(self.timeout)
            .header("x-ms-version", "2021-06-08")
            .header("x-ms-lease-action", "release")
            .header("x-ms-lease-id", lock_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lease release failed: {}", e))
            })?;

        if response.status().is_success() {
            *self.lease_id.write() = None;
            return Ok(true);
        }

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            *self.lease_id.write() = None;
            return Ok(false);
        }

        Err(ProvisioningError::ConcurrencyError(format!(
            "Lease release returned status {}",
            response.status()
        )))
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        // Azure doesn't store arbitrary lock info, just check if leased
        let url = format!(
            "{}?comp=lease{}",
            self.blob_url(),
            self.get_sas_suffix().replace('?', "&")
        );

        let response = self
            .client
            .head(&url)
            .timeout(self.timeout)
            .header("x-ms-version", "2021-06-08")
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lease check failed: {}", e))
            })?;

        if let Some(lease_state) = response.headers().get("x-ms-lease-state") {
            if lease_state.to_str().unwrap_or("") == "leased" {
                let lease_id = response
                    .headers()
                    .get("x-ms-lease-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown")
                    .to_string();

                return Ok(Some(LockInfo {
                    id: lease_id,
                    operation: "unknown".to_string(),
                    who: "unknown".to_string(),
                    version: "1".to_string(),
                    created: chrono::Utc::now(),
                    info: "Azure Blob Lease".to_string(),
                }));
            }
        }

        Ok(None)
    }

    async fn force_unlock(&self, lock_id: &str) -> ProvisioningResult<()> {
        let url = format!(
            "{}?comp=lease{}",
            self.blob_url(),
            self.get_sas_suffix().replace('?', "&")
        );

        let response = self
            .client
            .put(&url)
            .timeout(self.timeout)
            .header("x-ms-version", "2021-06-08")
            .header("x-ms-lease-action", "break")
            .header("x-ms-lease-break-period", "0")
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lease break failed: {}", e))
            })?;

        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            *self.lease_id.write() = None;
            return Ok(());
        }

        Err(ProvisioningError::ConcurrencyError(format!(
            "Lease break returned status {}",
            response.status()
        )))
    }

    fn backend_name(&self) -> &str {
        "azure-lease"
    }
}

// ============================================================================
// Consul Backend
// ============================================================================

/// Consul KV state backend
///
/// Stores state in Consul's key-value store with optional session-based locking.
///
/// Configuration via environment variables:
/// - CONSUL_HTTP_ADDR: Consul address (default: http://127.0.0.1:8500)
/// - CONSUL_HTTP_TOKEN: ACL token for authentication
/// - CONSUL_HTTP_SSL: Enable TLS (set to "true")
/// - CONSUL_CACERT: CA certificate path for TLS
pub struct ConsulBackend {
    /// Consul address
    address: String,
    /// KV path for state
    path: String,
    /// ACL token (optional)
    token: Option<String>,
    /// HTTP client
    client: reqwest::Client,
    /// Request timeout
    timeout: Duration,
    /// Session ID for locking (if acquired)
    session_id: Arc<RwLock<Option<String>>>,
    /// Enable session-based locking
    use_session_lock: bool,
}

impl ConsulBackend {
    /// Create a new Consul backend
    pub fn new(path: String) -> Self {
        let address = std::env::var("CONSUL_HTTP_ADDR")
            .unwrap_or_else(|_| "http://127.0.0.1:8500".to_string());
        let token = std::env::var("CONSUL_HTTP_TOKEN").ok();

        Self {
            address,
            path,
            token,
            client: reqwest::Client::new(),
            timeout: Duration::from_secs(30),
            session_id: Arc::new(RwLock::new(None)),
            use_session_lock: true,
        }
    }

    /// Create with specific address
    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = address.into();
        self
    }

    /// Create with ACL token
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Create with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Disable session-based locking
    pub fn without_session_lock(mut self) -> Self {
        self.use_session_lock = false;
        self
    }

    /// Get the Consul address
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Get the KV path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get KV URL
    fn kv_url(&self) -> String {
        format!("{}/v1/kv/{}", self.address, self.path)
    }

    /// Build request with optional auth
    fn build_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.client.request(method, url).timeout(self.timeout);

        if let Some(ref token) = self.token {
            request = request.header("X-Consul-Token", token);
        }

        // Add session ID if we have one for CAS operations
        if let Some(ref session_id) = *self.session_id.read() {
            request = request.query(&[("acquire", session_id.as_str())]);
        }

        request
    }
}

#[async_trait]
impl StateBackend for ConsulBackend {
    fn name(&self) -> &str {
        "consul"
    }

    async fn load(&self) -> ProvisioningResult<Option<ProvisioningState>> {
        let response = self
            .build_request(reqwest::Method::GET, &self.kv_url())
            .query(&[("raw", "true")])
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("Consul GET failed: {}", e))
            })?;

        match response.status() {
            status if status.is_success() => {
                let content = response.text().await.map_err(|e| {
                    ProvisioningError::StatePersistenceError(format!(
                        "Failed to read Consul response: {}",
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
                "Consul GET returned status {}",
                status
            ))),
        }
    }

    async fn save(&self, state: &ProvisioningState) -> ProvisioningResult<()> {
        let content = serde_json::to_string(state)?;

        let response = self
            .build_request(reqwest::Method::PUT, &self.kv_url())
            .body(content)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("Consul PUT failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ProvisioningError::StatePersistenceError(format!(
                "Consul PUT returned status {}",
                response.status()
            )));
        }

        // Check response body for CAS success (returns "true" or "false")
        let body = response.text().await.unwrap_or_default();
        if body.trim() == "false" {
            return Err(ProvisioningError::ConcurrencyError(
                "Consul CAS operation failed - state may have changed".to_string(),
            ));
        }

        Ok(())
    }

    async fn delete(&self) -> ProvisioningResult<()> {
        let response = self
            .build_request(reqwest::Method::DELETE, &self.kv_url())
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("Consul DELETE failed: {}", e))
            })?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(ProvisioningError::StatePersistenceError(format!(
                "Consul DELETE returned status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn exists(&self) -> ProvisioningResult<bool> {
        let response = self
            .build_request(reqwest::Method::GET, &self.kv_url())
            .query(&[("keys", "")])
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("Consul GET failed: {}", e))
            })?;

        Ok(response.status().is_success())
    }

    fn lock_backend(&self) -> Option<Arc<dyn LockBackend>> {
        if self.use_session_lock {
            Some(Arc::new(ConsulSessionLock {
                address: self.address.clone(),
                path: self.path.clone(),
                token: self.token.clone(),
                client: self.client.clone(),
                timeout: self.timeout,
                session_id: Arc::clone(&self.session_id),
            }))
        } else {
            None
        }
    }
}

/// Consul session-based lock backend
struct ConsulSessionLock {
    address: String,
    path: String,
    token: Option<String>,
    client: reqwest::Client,
    timeout: Duration,
    session_id: Arc<RwLock<Option<String>>>,
}

impl ConsulSessionLock {
    fn session_url(&self) -> String {
        format!("{}/v1/session", self.address)
    }

    fn kv_url(&self) -> String {
        format!("{}/v1/kv/{}", self.address, self.path)
    }

    fn build_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.client.request(method, url).timeout(self.timeout);

        if let Some(ref token) = self.token {
            request = request.header("X-Consul-Token", token);
        }

        request
    }
}

#[async_trait]
impl LockBackend for ConsulSessionLock {
    async fn acquire(&self, info: &LockInfo, timeout: Duration) -> ProvisioningResult<bool> {
        // Create a session for locking
        let session_config = serde_json::json!({
            "Name": format!("rustible-lock-{}", info.operation),
            "TTL": format!("{}s", timeout.as_secs().max(10)),
            "Behavior": "delete",
            "LockDelay": "0s"
        });

        let response = self
            .build_request(
                reqwest::Method::PUT,
                &format!("{}/create", self.session_url()),
            )
            .json(&session_config)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Session create failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Session create returned status {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| {
            ProvisioningError::ConcurrencyError(format!("Failed to parse session response: {}", e))
        })?;

        let session_id = body
            .get("ID")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ConcurrencyError("No session ID in response".to_string())
            })?
            .to_string();

        // Try to acquire lock with session
        let lock_info_json = serde_json::to_string(info)?;
        let lock_url = format!("{}/.lock?acquire={}", self.kv_url(), session_id);

        let response = self
            .build_request(reqwest::Method::PUT, &lock_url)
            .body(lock_info_json)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lock acquire failed: {}", e))
            })?;

        if !response.status().is_success() {
            // Clean up session
            let _ = self
                .build_request(
                    reqwest::Method::PUT,
                    &format!("{}/destroy/{}", self.session_url(), session_id),
                )
                .send()
                .await;
            return Ok(false);
        }

        let body = response.text().await.unwrap_or_default();
        if body.trim() == "true" {
            *self.session_id.write() = Some(session_id);
            return Ok(true);
        }

        // Clean up session
        let _ = self
            .build_request(
                reqwest::Method::PUT,
                &format!("{}/destroy/{}", self.session_url(), session_id),
            )
            .send()
            .await;

        Ok(false)
    }

    async fn release(&self, _lock_id: &str) -> ProvisioningResult<bool> {
        let session_id = match self.session_id.read().clone() {
            Some(id) => id,
            None => return Ok(false),
        };

        // Release lock
        let lock_url = format!("{}/.lock?release={}", self.kv_url(), session_id);
        let _ = self
            .build_request(reqwest::Method::PUT, &lock_url)
            .send()
            .await;

        // Destroy session
        let response = self
            .build_request(
                reqwest::Method::PUT,
                &format!("{}/destroy/{}", self.session_url(), session_id),
            )
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Session destroy failed: {}", e))
            })?;

        *self.session_id.write() = None;

        Ok(response.status().is_success())
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        let lock_url = format!("{}/.lock?raw=true", self.kv_url());

        let response = self
            .build_request(reqwest::Method::GET, &lock_url)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::ConcurrencyError(format!("Lock check failed: {}", e))
            })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(ProvisioningError::ConcurrencyError(format!(
                "Lock check returned status {}",
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

    async fn force_unlock(&self, _lock_id: &str) -> ProvisioningResult<()> {
        // Delete the lock key
        let lock_url = format!("{}/.lock", self.kv_url());
        let response = self
            .build_request(reqwest::Method::DELETE, &lock_url)
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

        *self.session_id.write() = None;
        Ok(())
    }

    fn backend_name(&self) -> &str {
        "consul"
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
        /// Object key in bucket
        key: String,
    },
    /// Azure Blob Storage backend
    AzureBlob {
        /// Storage account name
        storage_account: String,
        /// Storage container
        container: String,
        /// Blob name
        name: String,
    },
    /// Consul KV backend
    Consul {
        /// Consul address (default from CONSUL_HTTP_ADDR)
        address: Option<String>,
        /// KV path
        path: String,
        /// ACL token (default from CONSUL_HTTP_TOKEN)
        token: Option<String>,
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
            BackendConfig::Gcs { bucket, key } => {
                Ok(Box::new(GcsBackend::new(bucket.clone(), key.clone())))
            }
            #[cfg(not(feature = "gcp"))]
            BackendConfig::Gcs { .. } => Err(ProvisioningError::ProviderConfigError {
                provider: "gcp".to_string(),
                message: "GCS backend requires the 'gcp' feature".to_string(),
            }),
            #[cfg(feature = "azure")]
            BackendConfig::AzureBlob {
                storage_account,
                container,
                name,
            } => Ok(Box::new(AzureBlobBackend::new(
                storage_account.clone(),
                container.clone(),
                name.clone(),
            ))),
            #[cfg(not(feature = "azure"))]
            BackendConfig::AzureBlob { .. } => Err(ProvisioningError::ProviderConfigError {
                provider: "azure".to_string(),
                message: "Azure Blob backend requires the 'azure' feature".to_string(),
            }),
            BackendConfig::Consul {
                address,
                path,
                token,
            } => {
                let mut backend = ConsulBackend::new(path.clone());
                if let Some(addr) = address {
                    backend = backend.with_address(addr);
                }
                if let Some(tok) = token {
                    backend = backend.with_token(tok);
                }
                Ok(Box::new(backend))
            }
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

    #[tokio::test]
    async fn test_consul_backend_creation() {
        let backend = ConsulBackend::new("rustible/state".to_string())
            .with_address("http://localhost:8500")
            .with_token("test-token")
            .with_timeout(Duration::from_secs(60));

        assert_eq!(backend.name(), "consul");
        assert_eq!(backend.address(), "http://localhost:8500");
        assert_eq!(backend.path(), "rustible/state");
        assert!(backend.lock_backend().is_some());
    }

    #[tokio::test]
    async fn test_consul_backend_no_lock() {
        let backend = ConsulBackend::new("rustible/state".to_string()).without_session_lock();

        assert_eq!(backend.name(), "consul");
        assert!(backend.lock_backend().is_none());
    }

    #[tokio::test]
    async fn test_backend_config_consul() {
        let config = BackendConfig::Consul {
            address: Some("http://consul.example.com:8500".to_string()),
            path: "terraform/state".to_string(),
            token: Some("secret-token".to_string()),
        };

        let backend = config.create_backend().await.unwrap();
        assert_eq!(backend.name(), "consul");
        assert!(backend.lock_backend().is_some());
    }

    #[test]
    fn test_consul_config_serialization() {
        let config = BackendConfig::Consul {
            address: Some("http://consul.example.com:8500".to_string()),
            path: "terraform/state".to_string(),
            token: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

        match deserialized {
            BackendConfig::Consul {
                address,
                path,
                token,
            } => {
                assert_eq!(address, Some("http://consul.example.com:8500".to_string()));
                assert_eq!(path, "terraform/state");
                assert!(token.is_none());
            }
            _ => panic!("Expected Consul config"),
        }
    }

    #[test]
    fn test_gcs_config_serialization() {
        let config = BackendConfig::Gcs {
            bucket: "my-gcs-bucket".to_string(),
            key: "state/terraform.tfstate".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

        match deserialized {
            BackendConfig::Gcs { bucket, key } => {
                assert_eq!(bucket, "my-gcs-bucket");
                assert_eq!(key, "state/terraform.tfstate");
            }
            _ => panic!("Expected GCS config"),
        }
    }

    #[test]
    fn test_azure_config_serialization() {
        let config = BackendConfig::AzureBlob {
            storage_account: "myaccount".to_string(),
            container: "tfstate".to_string(),
            name: "terraform.tfstate".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

        match deserialized {
            BackendConfig::AzureBlob {
                storage_account,
                container,
                name,
            } => {
                assert_eq!(storage_account, "myaccount");
                assert_eq!(container, "tfstate");
                assert_eq!(name, "terraform.tfstate");
            }
            _ => panic!("Expected Azure config"),
        }
    }
}
