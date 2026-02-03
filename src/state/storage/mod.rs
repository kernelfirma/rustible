//! State storage backends for remote state management
//!
//! This module provides support for storing state remotely in various backends:
//! - S3 (AWS)
//! - GCS (Google Cloud Storage)
//! - Azure Blob Storage
//! - Consul KV

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// State storage backend trait
#[async_trait]
pub trait StateBackend: Send + Sync {
    /// Store state
    async fn store_state(&self, key: &str, state: &StateFile) -> Result<(), StateError>;

    /// Retrieve state
    async fn retrieve_state(&self, key: &str) -> Result<Option<StateFile>, StateError>;

    /// Delete state
    async fn delete_state(&self, key: &str) -> Result<(), StateError>;

    /// List all state keys
    async fn list_states(&self) -> Result<Vec<String>, StateError>;

    /// Check if state exists
    async fn state_exists(&self, key: &str) -> Result<bool, StateError>;
}

/// State storage error
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Storage backend error: {0}")]
    Backend(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("State not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// State file representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    /// State version
    pub version: String,
    /// Terraform-format serial (for compatibility)
    pub serial: u64,
    /// Last modified timestamp
    pub last_modified: chrono::DateTime<chrono::Utc>,
    /// State data
    pub data: serde_json::Value,
    /// Checksum for integrity verification
    pub checksum: String,
}

impl StateFile {
    /// Create a new state file
    pub fn new(data: serde_json::Value) -> Self {
        let checksum = Self::calculate_checksum(&data);

        Self {
            version: "1.0".to_string(),
            serial: 1,
            last_modified: chrono::Utc::now(),
            data,
            checksum,
        }
    }

    /// Calculate checksum of state data
    fn calculate_checksum(data: &serde_json::Value) -> String {
        let serialized = serde_json::to_string(data).unwrap_or_default();
        let hash = blake3::hash(serialized.as_bytes());
        hash.to_string()
    }

    /// Verify checksum
    pub fn verify_checksum(&self) -> bool {
        let expected = Self::calculate_checksum(&self.data);
        self.checksum == expected
    }

    /// Increment serial
    pub fn increment_serial(&mut self) {
        self.serial += 1;
        self.last_modified = chrono::Utc::now();
    }
}

/// S3 backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// AWS region
    pub region: String,
    /// Bucket name
    pub bucket: String,
    /// Key prefix for state files
    pub key_prefix: Option<String>,
    /// AWS access key ID (optional, can use env vars)
    pub access_key_id: Option<String>,
    /// AWS secret access key (optional, can use env vars)
    pub secret_access_key: Option<String>,
}

/// GCS backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcsConfig {
    /// GCS bucket name
    pub bucket: String,
    /// Key prefix for state files
    pub key_prefix: Option<String>,
    /// Google credentials JSON (optional, can use env vars)
    pub credentials: Option<String>,
}

/// Azure backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureConfig {
    /// Storage account name
    pub storage_account: String,
    /// Container name
    pub container: String,
    /// Key prefix for state files
    pub key_prefix: Option<String>,
}

/// Consul backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsulConfig {
    /// Consul address
    pub address: String,
    /// Consul token (optional)
    pub token: Option<String>,
    /// KV path prefix for state files
    pub path_prefix: Option<String>,
    /// Use HTTPS
    pub https: bool,
}

/// State storage backend type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageBackend {
    /// Local filesystem
    Local { path: PathBuf },
    /// S3 backend
    S3 { config: S3Config },
    /// GCS backend
    Gcs { config: GcsConfig },
    /// Azure backend
    Azure { config: AzureConfig },
    /// Consul backend
    Consul { config: ConsulConfig },
}

/// State storage manager
pub struct StateStorage {
    backends: HashMap<String, Box<dyn StateBackend>>,
    default_backend: Option<String>,
}

impl StateStorage {
    /// Create a new state storage manager
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
            default_backend: None,
        }
    }

    /// Add a backend
    pub fn add_backend(&mut self, name: String, backend: Box<dyn StateBackend>) {
        self.backends.insert(name, backend);
    }

    /// Set default backend
    pub fn set_default_backend(&mut self, name: String) {
        self.default_backend = Some(name);
    }

    /// Store state
    pub async fn store_state(
        &self,
        backend: Option<&str>,
        key: &str,
        state: &StateFile,
    ) -> Result<(), StateError> {
        let backend_name = backend
            .or(self.default_backend.as_deref())
            .ok_or_else(|| StateError::Backend("No backend specified".to_string()))?;

        let backend = self
            .backends
            .get(backend_name)
            .ok_or_else(|| StateError::NotFound(format!("Backend '{}'", backend_name)))?;

        backend.store_state(key, state).await
    }

    /// Retrieve state
    pub async fn retrieve_state(
        &self,
        backend: Option<&str>,
        key: &str,
    ) -> Result<Option<StateFile>, StateError> {
        let backend_name = backend
            .or(self.default_backend.as_deref())
            .ok_or_else(|| StateError::Backend("No backend specified".to_string()))?;

        let backend = self
            .backends
            .get(backend_name)
            .ok_or_else(|| StateError::NotFound(format!("Backend '{}'", backend_name)))?;

        backend.retrieve_state(key).await
    }

    /// Delete state
    pub async fn delete_state(&self, backend: Option<&str>, key: &str) -> Result<(), StateError> {
        let backend_name = backend
            .or(self.default_backend.as_deref())
            .ok_or_else(|| StateError::Backend("No backend specified".to_string()))?;

        let backend = self
            .backends
            .get(backend_name)
            .ok_or_else(|| StateError::NotFound(format!("Backend '{}'", backend_name)))?;

        backend.delete_state(key).await
    }
}

impl Default for StateStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Local filesystem backend
pub struct LocalBackend {
    base_path: PathBuf,
}

impl LocalBackend {
    /// Create a new local backend
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get full path for a key
    fn get_path(&self, key: &str) -> PathBuf {
        self.base_path.join(format!("{}.json", key))
    }
}

#[async_trait]
impl StateBackend for LocalBackend {
    async fn store_state(&self, key: &str, state: &StateFile) -> Result<(), StateError> {
        let path = self.get_path(key);

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize and write
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| StateError::Serialization(e.to_string()))?;

        std::fs::write(&path, json)?;

        Ok(())
    }

    async fn retrieve_state(&self, key: &str) -> Result<Option<StateFile>, StateError> {
        let path = self.get_path(key);

        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)?;
        let state: StateFile =
            serde_json::from_str(&content).map_err(|e| StateError::Serialization(e.to_string()))?;

        // Verify checksum
        if !state.verify_checksum() {
            return Err(StateError::Backend("State checksum mismatch".to_string()));
        }

        Ok(Some(state))
    }

    async fn delete_state(&self, key: &str) -> Result<(), StateError> {
        let path = self.get_path(key);

        if path.exists() {
            std::fs::remove_file(&path)?;
        }

        Ok(())
    }

    async fn list_states(&self) -> Result<Vec<String>, StateError> {
        let mut states = Vec::new();

        if !self.base_path.exists() {
            return Ok(states);
        }

        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    states.push(stem.to_string());
                }
            }
        }

        Ok(states)
    }

    async fn state_exists(&self, key: &str) -> Result<bool, StateError> {
        Ok(self.get_path(key).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_state_file() {
        let data = serde_json::json!({"key": "value"});
        let state = StateFile::new(data.clone());

        assert_eq!(state.serial, 1);
        assert!(state.verify_checksum());
    }

    #[test]
    fn test_state_file_increment() {
        let data = serde_json::json!({"key": "value"});
        let mut state = StateFile::new(data);

        assert_eq!(state.serial, 1);
        state.increment_serial();
        assert_eq!(state.serial, 2);
    }

    #[tokio::test]
    async fn test_local_backend() {
        let temp_dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(temp_dir.path().to_path_buf());

        let state = StateFile::new(serde_json::json!({"test": "data"}));

        // Store state
        backend.store_state("test-key", &state).await.unwrap();

        // Retrieve state
        let retrieved = backend.retrieve_state("test-key").await.unwrap();
        assert!(retrieved.is_some());

        // Check existence
        assert!(backend.state_exists("test-key").await.unwrap());

        // Delete state
        backend.delete_state("test-key").await.unwrap();
        assert!(!backend.state_exists("test-key").await.unwrap());
    }
}
