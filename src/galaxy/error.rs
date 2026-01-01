//! Error types for the Galaxy module.
//!
//! This module provides comprehensive error handling for all Galaxy operations,
//! including network errors, parsing errors, integrity verification failures,
//! and cache-related issues.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for Galaxy operations.
pub type GalaxyResult<T> = Result<T, GalaxyError>;

/// Comprehensive error type for Galaxy operations.
#[derive(Error, Debug)]
pub enum GalaxyError {
    // ========================================================================
    // Network Errors
    // ========================================================================
    /// HTTP request failed.
    #[error("HTTP request failed: {message}")]
    HttpError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Connection to Galaxy server failed.
    #[error("Failed to connect to Galaxy server '{server}': {message}")]
    ConnectionFailed { server: String, message: String },

    /// Request timed out.
    #[error("Request to '{url}' timed out after {timeout_secs} seconds")]
    Timeout { url: String, timeout_secs: u64 },

    /// Rate limited by Galaxy server.
    #[error("Rate limited by Galaxy server. Retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    /// Authentication failed.
    #[error("Authentication failed for Galaxy server: {message}")]
    AuthenticationFailed { message: String },

    // ========================================================================
    // Collection Errors
    // ========================================================================
    /// Collection not found.
    #[error("Collection '{name}' not found on Galaxy")]
    CollectionNotFound { name: String },

    /// Collection version not found.
    #[error("Version '{version}' not found for collection '{name}'")]
    CollectionVersionNotFound { name: String, version: String },

    /// Invalid collection name format.
    #[error("Invalid collection name '{name}': {reason}")]
    InvalidCollectionName { name: String, reason: String },

    /// Collection installation failed.
    #[error("Failed to install collection '{name}': {message}")]
    CollectionInstallFailed { name: String, message: String },

    /// Collection already installed.
    #[error("Collection '{name}' version '{version}' is already installed at '{path}'")]
    CollectionAlreadyInstalled {
        name: String,
        version: String,
        path: PathBuf,
    },

    /// Collection dependency resolution failed.
    #[error("Failed to resolve dependencies for collection '{name}': {message}")]
    DependencyResolutionFailed { name: String, message: String },

    // ========================================================================
    // Role Errors
    // ========================================================================
    /// Role not found.
    #[error("Role '{name}' not found on Galaxy")]
    RoleNotFound { name: String },

    /// Role version not found.
    #[error("Version '{version}' not found for role '{name}'")]
    RoleVersionNotFound { name: String, version: String },

    /// Invalid role name format.
    #[error("Invalid role name '{name}': {reason}")]
    InvalidRoleName { name: String, reason: String },

    /// Role installation failed.
    #[error("Failed to install role '{name}': {message}")]
    RoleInstallFailed { name: String, message: String },

    // ========================================================================
    // Requirements Errors
    // ========================================================================
    /// Requirements file not found.
    #[error("Requirements file not found: {path}")]
    RequirementsFileNotFound { path: PathBuf },

    /// Requirements file parsing failed.
    #[error("Failed to parse requirements file '{path}': {message}")]
    RequirementsParseError { path: PathBuf, message: String },

    /// Invalid requirement specification.
    #[error("Invalid requirement: {message}")]
    InvalidRequirement { message: String },

    // ========================================================================
    // Cache Errors
    // ========================================================================
    /// Cache directory creation failed.
    #[error("Failed to create cache directory '{path}': {message}")]
    CacheDirectoryError { path: PathBuf, message: String },

    /// Cache read error.
    #[error("Failed to read from cache: {message}")]
    CacheReadError { message: String },

    /// Cache write error.
    #[error("Failed to write to cache: {message}")]
    CacheWriteError { message: String },

    /// Artifact not found in cache.
    #[error("Artifact '{name}' version '{version}' not found in cache")]
    CacheMiss { name: String, version: String },

    /// Cache is corrupted.
    #[error("Cache is corrupted: {message}")]
    CacheCorrupted { message: String },

    // ========================================================================
    // Integrity Errors
    // ========================================================================
    /// Checksum verification failed.
    #[error(
        "Checksum verification failed for '{artifact}': expected '{expected}', got '{actual}'"
    )]
    ChecksumMismatch {
        artifact: String,
        expected: String,
        actual: String,
    },

    /// Signature verification failed.
    #[error("Signature verification failed for '{artifact}': {message}")]
    SignatureVerificationFailed { artifact: String, message: String },

    /// Missing checksum in manifest.
    #[error("Missing checksum for file '{file}' in collection manifest")]
    MissingChecksum { file: String },

    // ========================================================================
    // Archive Errors
    // ========================================================================
    /// Failed to extract archive.
    #[error("Failed to extract archive '{path}': {message}")]
    ExtractionFailed { path: PathBuf, message: String },

    /// Invalid archive format.
    #[error("Invalid archive format for '{path}': {message}")]
    InvalidArchive { path: PathBuf, message: String },

    // ========================================================================
    // Version Errors
    // ========================================================================
    /// Invalid version constraint.
    #[error("Invalid version constraint '{constraint}': {message}")]
    InvalidVersionConstraint { constraint: String, message: String },

    /// Version conflict.
    #[error("Version conflict: {message}")]
    VersionConflict { message: String },

    /// No matching version found.
    #[error("No version of '{name}' matches constraint '{constraint}'")]
    NoMatchingVersion { name: String, constraint: String },

    // ========================================================================
    // Offline Mode Errors
    // ========================================================================
    /// Operation requires network but offline mode is enabled.
    #[error("Operation requires network access but offline mode is enabled")]
    OfflineModeEnabled,

    /// No cached version available for offline installation.
    #[error("No cached version of '{name}' available for offline installation")]
    NoCachedVersion { name: String },

    // ========================================================================
    // IO and System Errors
    // ========================================================================
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// YAML parsing error.
    #[error("YAML parsing error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    JsonParse(#[from] serde_json::Error),

    /// Generic error with message.
    #[error("{0}")]
    Other(String),
}

impl GalaxyError {
    /// Create an HTTP error.
    pub fn http_error(message: impl Into<String>) -> Self {
        Self::HttpError {
            message: message.into(),
            source: None,
        }
    }

    /// Create an HTTP error with source.
    pub fn http_error_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HttpError {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a connection failed error.
    pub fn connection_failed(server: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ConnectionFailed {
            server: server.into(),
            message: message.into(),
        }
    }

    /// Create a collection not found error.
    pub fn collection_not_found(name: impl Into<String>) -> Self {
        Self::CollectionNotFound { name: name.into() }
    }

    /// Create a role not found error.
    pub fn role_not_found(name: impl Into<String>) -> Self {
        Self::RoleNotFound { name: name.into() }
    }

    /// Create a cache miss error.
    pub fn cache_miss(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self::CacheMiss {
            name: name.into(),
            version: version.into(),
        }
    }

    /// Create a checksum mismatch error.
    pub fn checksum_mismatch(
        artifact: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::ChecksumMismatch {
            artifact: artifact.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Check if the error is recoverable (e.g., by retrying).
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            GalaxyError::HttpError { .. }
                | GalaxyError::ConnectionFailed { .. }
                | GalaxyError::Timeout { .. }
                | GalaxyError::RateLimited { .. }
        )
    }

    /// Check if the error is a cache-related error.
    pub fn is_cache_error(&self) -> bool {
        matches!(
            self,
            GalaxyError::CacheDirectoryError { .. }
                | GalaxyError::CacheReadError { .. }
                | GalaxyError::CacheWriteError { .. }
                | GalaxyError::CacheMiss { .. }
                | GalaxyError::CacheCorrupted { .. }
        )
    }

    /// Check if the error is related to offline mode.
    pub fn is_offline_error(&self) -> bool {
        matches!(
            self,
            GalaxyError::OfflineModeEnabled | GalaxyError::NoCachedVersion { .. }
        )
    }

    /// Get a hint for resolving the error.
    pub fn hint(&self) -> String {
        match self {
            GalaxyError::ConnectionFailed { server, .. } => {
                format!(
                    "Check your network connection and verify the Galaxy server URL: {}",
                    server
                )
            }
            GalaxyError::CollectionNotFound { name } => {
                format!(
                    "Verify the collection name '{}' is correct. Use 'rustible-galaxy search {}' to find similar collections.",
                    name,
                    name.split('.').next().unwrap_or(name)
                )
            }
            GalaxyError::RoleNotFound { name } => {
                format!(
                    "Verify the role name '{}' is correct. Use 'rustible-galaxy search {}' to find similar roles.",
                    name, name
                )
            }
            GalaxyError::CacheMiss { name, .. } => {
                format!(
                    "The artifact '{}' is not in the cache. Run without --offline to download it.",
                    name
                )
            }
            GalaxyError::ChecksumMismatch { artifact, .. } => {
                format!(
                    "The file '{}' failed integrity verification. Try clearing the cache and re-downloading.",
                    artifact
                )
            }
            GalaxyError::RateLimited { retry_after_secs } => {
                format!(
                    "Galaxy is rate limiting requests. Wait {} seconds before retrying.",
                    retry_after_secs
                )
            }
            GalaxyError::AuthenticationFailed { .. } => {
                "Check your Galaxy API token. You can set it using the ANSIBLE_GALAXY_TOKEN environment variable.".to_string()
            }
            GalaxyError::OfflineModeEnabled => {
                "Network access is required for this operation. Remove the --offline flag to proceed.".to_string()
            }
            GalaxyError::InvalidCollectionName { reason, .. } => {
                format!("Collection names must be in 'namespace.name' format. {}", reason)
            }
            GalaxyError::RequirementsParseError { path, .. } => {
                format!(
                    "Check the syntax of {}. Ensure it follows the requirements.yml format.",
                    path.display()
                )
            }
            _ => "Check the error message for details.".to_string(),
        }
    }
}

impl From<reqwest::Error> for GalaxyError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            GalaxyError::Timeout {
                url: err.url().map(|u| u.to_string()).unwrap_or_default(),
                timeout_secs: 30,
            }
        } else if err.is_connect() {
            GalaxyError::ConnectionFailed {
                server: err
                    .url()
                    .map(|u| u.host_str().unwrap_or("unknown").to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                message: err.to_string(),
            }
        } else {
            GalaxyError::HttpError {
                message: err.to_string(),
                source: Some(Box::new(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = GalaxyError::collection_not_found("community.general");
        assert!(
            matches!(err, GalaxyError::CollectionNotFound { name } if name == "community.general")
        );
    }

    #[test]
    fn test_error_is_recoverable() {
        let timeout = GalaxyError::Timeout {
            url: "https://galaxy.ansible.com".to_string(),
            timeout_secs: 30,
        };
        assert!(timeout.is_recoverable());

        let not_found = GalaxyError::collection_not_found("test");
        assert!(!not_found.is_recoverable());
    }

    #[test]
    fn test_error_hints() {
        let err = GalaxyError::CacheMiss {
            name: "test.collection".to_string(),
            version: "1.0.0".to_string(),
        };
        let hint = err.hint();
        assert!(hint.contains("not in the cache"));
    }

    #[test]
    fn test_error_display() {
        let err = GalaxyError::ChecksumMismatch {
            artifact: "test.tar.gz".to_string(),
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
    }
}
