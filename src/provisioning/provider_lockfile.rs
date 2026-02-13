//! Provider lockfile for reproducible infrastructure provisioning
//!
//! This module provides a lockfile mechanism similar to Terraform's `.terraform.lock.hcl`,
//! ensuring that provider versions and integrity hashes are pinned across team members
//! and CI/CD environments.
//!
//! ## Features
//!
//! - **Version Pinning**: Lock provider versions to exact versions used during init
//! - **Integrity Verification**: Store and verify provider hashes for supply-chain security
//! - **Constraint Tracking**: Record version constraints from configuration
//! - **Atomic Updates**: Safe concurrent lockfile updates via temp file + rename
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use rustible::provisioning::provider_lockfile::ProviderLockfile;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Load existing lockfile
//! let lockfile = ProviderLockfile::load(".rustible/provider.lock.json").await?;
//!
//! // Validate current providers match lockfile
//! let mismatches = lockfile.validate(&current_providers);
//! if !mismatches.is_empty() {
//!     eprintln!("Provider version mismatch detected!");
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing;

use super::error::{ProvisioningError, ProvisioningResult};

// ============================================================================
// Provider Lock Entry
// ============================================================================

/// Lock information for a single provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderLock {
    /// Exact version that was resolved and used
    pub version: String,

    /// Version constraint string from configuration (e.g., "~> 5.0", ">= 3.0, < 4.0")
    pub constraints: String,

    /// Integrity hashes for verifying the provider binary/package
    /// Format follows SRI (Subresource Integrity): "h1:xxxx", "zh:xxxx"
    pub hashes: Vec<String>,
}

impl ProviderLock {
    /// Create a new provider lock entry
    pub fn new(
        version: impl Into<String>,
        constraints: impl Into<String>,
        hashes: Vec<String>,
    ) -> Self {
        Self {
            version: version.into(),
            constraints: constraints.into(),
            hashes,
        }
    }

    /// Check if a given version satisfies this lock entry
    pub fn matches_version(&self, version: &str) -> bool {
        self.version == version
    }

    /// Check if a hash is present in the lock entry
    pub fn contains_hash(&self, hash: &str) -> bool {
        self.hashes.iter().any(|h| h == hash)
    }
}

// ============================================================================
// Provider Info (for generation/validation)
// ============================================================================

/// Information about a currently configured provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Provider name (e.g., "aws", "azure", "gcp")
    pub name: String,

    /// Current version of the provider
    pub version: String,

    /// Version constraint from configuration
    pub constraints: String,

    /// Computed hashes for the provider
    pub hashes: Vec<String>,
}

impl ProviderInfo {
    /// Create a new provider info
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        constraints: impl Into<String>,
        hashes: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            constraints: constraints.into(),
            hashes,
        }
    }
}

// ============================================================================
// Validation Result
// ============================================================================

/// Result of validating current providers against the lockfile
#[derive(Debug, Clone)]
pub struct LockfileValidationError {
    /// Provider that failed validation
    pub provider: String,

    /// Kind of mismatch
    pub kind: LockfileMismatchKind,
}

/// Kind of lockfile mismatch
#[derive(Debug, Clone, PartialEq)]
pub enum LockfileMismatchKind {
    /// Provider is in lockfile but not in current configuration
    MissingProvider,

    /// Provider version does not match lockfile
    VersionMismatch { locked: String, current: String },

    /// Provider hash does not match lockfile
    HashMismatch,

    /// Provider is in current config but not in lockfile
    NewProvider,
}

impl std::fmt::Display for LockfileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            LockfileMismatchKind::MissingProvider => {
                write!(
                    f,
                    "Provider '{}' is locked but not configured",
                    self.provider
                )
            }
            LockfileMismatchKind::VersionMismatch { locked, current } => {
                write!(
                    f,
                    "Provider '{}' version mismatch: locked={}, current={}",
                    self.provider, locked, current
                )
            }
            LockfileMismatchKind::HashMismatch => {
                write!(
                    f,
                    "Provider '{}' hash mismatch: integrity check failed",
                    self.provider
                )
            }
            LockfileMismatchKind::NewProvider => {
                write!(
                    f,
                    "Provider '{}' is configured but not in lockfile",
                    self.provider
                )
            }
        }
    }
}

// ============================================================================
// Provider Lockfile
// ============================================================================

/// Provider lockfile for pinning provider versions and hashes
///
/// Analogous to Terraform's `.terraform.lock.hcl`, this file records
/// the exact provider versions and integrity hashes used during initialization.
/// Subsequent runs validate against this lockfile to ensure consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLockfile {
    /// Lockfile format version
    pub version: u32,

    /// Locked provider entries keyed by provider name
    pub providers: HashMap<String, ProviderLock>,

    /// When the lockfile was last updated
    pub updated_at: DateTime<Utc>,
}

impl Default for ProviderLockfile {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderLockfile {
    /// Current lockfile format version
    pub const FORMAT_VERSION: u32 = 1;

    /// Default lockfile path relative to project root
    pub const DEFAULT_PATH: &'static str = ".rustible/provider.lock.json";

    /// Create a new empty lockfile
    pub fn new() -> Self {
        Self {
            version: Self::FORMAT_VERSION,
            providers: HashMap::new(),
            updated_at: Utc::now(),
        }
    }

    /// Generate a lockfile from current provider information
    ///
    /// This creates or updates the lockfile based on the providers
    /// that are currently configured and resolved.
    pub fn generate(providers: &[ProviderInfo]) -> Self {
        let mut lockfile = Self::new();

        for info in providers {
            let lock = ProviderLock::new(&info.version, &info.constraints, info.hashes.clone());
            lockfile.providers.insert(info.name.clone(), lock);

            tracing::debug!(
                provider = %info.name,
                version = %info.version,
                hashes = info.hashes.len(),
                "Locked provider"
            );
        }

        tracing::info!(
            count = lockfile.providers.len(),
            "Generated provider lockfile"
        );

        lockfile
    }

    /// Validate current providers against this lockfile
    ///
    /// Returns a list of validation errors. An empty list indicates
    /// all providers match the lockfile.
    pub fn validate(&self, current_providers: &[ProviderInfo]) -> Vec<LockfileValidationError> {
        let mut errors = Vec::new();

        // Check each locked provider against current
        for (name, lock) in &self.providers {
            match current_providers.iter().find(|p| &p.name == name) {
                None => {
                    errors.push(LockfileValidationError {
                        provider: name.clone(),
                        kind: LockfileMismatchKind::MissingProvider,
                    });
                }
                Some(current) => {
                    // Check version
                    if !lock.matches_version(&current.version) {
                        errors.push(LockfileValidationError {
                            provider: name.clone(),
                            kind: LockfileMismatchKind::VersionMismatch {
                                locked: lock.version.clone(),
                                current: current.version.clone(),
                            },
                        });
                    }

                    // Check hashes (if both have hashes)
                    if !lock.hashes.is_empty() && !current.hashes.is_empty() {
                        let has_matching_hash =
                            current.hashes.iter().any(|h| lock.contains_hash(h));

                        if !has_matching_hash {
                            errors.push(LockfileValidationError {
                                provider: name.clone(),
                                kind: LockfileMismatchKind::HashMismatch,
                            });
                        }
                    }
                }
            }
        }

        // Check for new providers not in lockfile
        for current in current_providers {
            if !self.providers.contains_key(&current.name) {
                errors.push(LockfileValidationError {
                    provider: current.name.clone(),
                    kind: LockfileMismatchKind::NewProvider,
                });
            }
        }

        if errors.is_empty() {
            tracing::debug!("Provider lockfile validation passed");
        } else {
            tracing::warn!(
                error_count = errors.len(),
                "Provider lockfile validation failed"
            );
        }

        errors
    }

    /// Update a single provider entry in the lockfile
    pub fn update_provider(&mut self, info: &ProviderInfo) {
        let lock = ProviderLock::new(&info.version, &info.constraints, info.hashes.clone());
        self.providers.insert(info.name.clone(), lock);
        self.updated_at = Utc::now();

        tracing::debug!(
            provider = %info.name,
            version = %info.version,
            "Updated provider in lockfile"
        );
    }

    /// Remove a provider entry from the lockfile
    pub fn remove_provider(&mut self, name: &str) -> Option<ProviderLock> {
        let removed = self.providers.remove(name);
        if removed.is_some() {
            self.updated_at = Utc::now();
            tracing::debug!(provider = %name, "Removed provider from lockfile");
        }
        removed
    }

    /// Load a lockfile from disk
    ///
    /// Returns a new empty lockfile if the file does not exist.
    pub async fn load(path: impl AsRef<Path>) -> ProvisioningResult<Self> {
        let path = path.as_ref();

        if !path.exists() {
            tracing::debug!(path = %path.display(), "Lockfile not found, creating new");
            return Ok(Self::new());
        }

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to read provider lockfile: {}",
                e
            ))
        })?;

        let lockfile: Self = serde_json::from_str(&content).map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to parse provider lockfile: {}",
                e
            ))
        })?;

        tracing::info!(
            path = %path.display(),
            providers = lockfile.providers.len(),
            "Loaded provider lockfile"
        );

        Ok(lockfile)
    }

    /// Save the lockfile to disk
    ///
    /// Uses atomic write (temp file + rename) to prevent corruption.
    pub async fn save(&mut self, path: impl AsRef<Path>) -> ProvisioningResult<()> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to create lockfile directory: {}",
                    e
                ))
            })?;
        }

        // Update timestamp
        self.updated_at = Utc::now();

        // Serialize to pretty JSON
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize provider lockfile: {}",
                e
            ))
        })?;

        // Write atomically using temp file
        let temp_path = path.with_extension("lock.tmp");

        tokio::fs::write(&temp_path, &content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to write lockfile temp file: {}",
                e
            ))
        })?;

        tokio::fs::rename(&temp_path, path).await.map_err(|e| {
            // Clean up temp file on failure
            let _ = std::fs::remove_file(&temp_path);
            ProvisioningError::StatePersistenceError(format!(
                "Failed to rename lockfile temp file: {}",
                e
            ))
        })?;

        tracing::info!(
            path = %path.display(),
            providers = self.providers.len(),
            "Saved provider lockfile"
        );

        Ok(())
    }

    /// Get the number of locked providers
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Check if the lockfile is empty
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Get a locked provider by name
    pub fn get(&self, name: &str) -> Option<&ProviderLock> {
        self.providers.get(name)
    }

    /// List all locked provider names
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_providers() -> Vec<ProviderInfo> {
        vec![
            ProviderInfo::new(
                "aws",
                "5.31.0",
                "~> 5.0",
                vec!["h1:abc123def456".to_string(), "zh:789ghi012jkl".to_string()],
            ),
            ProviderInfo::new(
                "azure",
                "3.75.0",
                ">= 3.0, < 4.0",
                vec!["h1:mno345pqr678".to_string()],
            ),
        ]
    }

    #[test]
    fn test_provider_lock_new() {
        let lock = ProviderLock::new("5.31.0", "~> 5.0", vec!["h1:abc123".to_string()]);
        assert_eq!(lock.version, "5.31.0");
        assert_eq!(lock.constraints, "~> 5.0");
        assert_eq!(lock.hashes.len(), 1);
    }

    #[test]
    fn test_provider_lock_matches_version() {
        let lock = ProviderLock::new("5.31.0", "~> 5.0", vec![]);
        assert!(lock.matches_version("5.31.0"));
        assert!(!lock.matches_version("5.32.0"));
    }

    #[test]
    fn test_provider_lock_contains_hash() {
        let lock = ProviderLock::new(
            "5.31.0",
            "~> 5.0",
            vec!["h1:abc123".to_string(), "zh:def456".to_string()],
        );
        assert!(lock.contains_hash("h1:abc123"));
        assert!(lock.contains_hash("zh:def456"));
        assert!(!lock.contains_hash("h1:unknown"));
    }

    #[test]
    fn test_lockfile_new() {
        let lockfile = ProviderLockfile::new();
        assert_eq!(lockfile.version, ProviderLockfile::FORMAT_VERSION);
        assert!(lockfile.is_empty());
        assert_eq!(lockfile.len(), 0);
    }

    #[test]
    fn test_lockfile_generate() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);

        assert_eq!(lockfile.len(), 2);
        assert!(lockfile.get("aws").is_some());
        assert!(lockfile.get("azure").is_some());

        let aws = lockfile.get("aws").unwrap();
        assert_eq!(aws.version, "5.31.0");
        assert_eq!(aws.constraints, "~> 5.0");
        assert_eq!(aws.hashes.len(), 2);

        let azure = lockfile.get("azure").unwrap();
        assert_eq!(azure.version, "3.75.0");
    }

    #[test]
    fn test_lockfile_validate_success() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);
        let errors = lockfile.validate(&providers);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_lockfile_validate_version_mismatch() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);

        let mut changed_providers = providers.clone();
        changed_providers[0].version = "5.32.0".to_string();

        let errors = lockfile.validate(&changed_providers);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].provider, "aws");
        assert_eq!(
            errors[0].kind,
            LockfileMismatchKind::VersionMismatch {
                locked: "5.31.0".to_string(),
                current: "5.32.0".to_string(),
            }
        );
    }

    #[test]
    fn test_lockfile_validate_missing_provider() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);

        // Validate with only one provider
        let partial = vec![providers[0].clone()];
        let errors = lockfile.validate(&partial);

        let missing: Vec<_> = errors
            .iter()
            .filter(|e| e.kind == LockfileMismatchKind::MissingProvider)
            .collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].provider, "azure");
    }

    #[test]
    fn test_lockfile_validate_new_provider() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);

        let mut extended = providers.clone();
        extended.push(ProviderInfo::new("gcp", "4.0.0", "~> 4.0", vec![]));

        let errors = lockfile.validate(&extended);

        let new_providers: Vec<_> = errors
            .iter()
            .filter(|e| e.kind == LockfileMismatchKind::NewProvider)
            .collect();
        assert_eq!(new_providers.len(), 1);
        assert_eq!(new_providers[0].provider, "gcp");
    }

    #[test]
    fn test_lockfile_validate_hash_mismatch() {
        let providers = sample_providers();
        let lockfile = ProviderLockfile::generate(&providers);

        let mut changed = providers.clone();
        changed[0].hashes = vec!["h1:tampered_hash".to_string()];

        let errors = lockfile.validate(&changed);

        let hash_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.kind == LockfileMismatchKind::HashMismatch)
            .collect();
        assert_eq!(hash_errors.len(), 1);
        assert_eq!(hash_errors[0].provider, "aws");
    }

    #[test]
    fn test_lockfile_update_provider() {
        let mut lockfile = ProviderLockfile::generate(&sample_providers());

        let updated = ProviderInfo::new("aws", "5.32.0", "~> 5.0", vec!["h1:new_hash".to_string()]);
        lockfile.update_provider(&updated);

        let aws = lockfile.get("aws").unwrap();
        assert_eq!(aws.version, "5.32.0");
        assert_eq!(aws.hashes, vec!["h1:new_hash".to_string()]);
    }

    #[test]
    fn test_lockfile_remove_provider() {
        let mut lockfile = ProviderLockfile::generate(&sample_providers());
        assert_eq!(lockfile.len(), 2);

        let removed = lockfile.remove_provider("azure");
        assert!(removed.is_some());
        assert_eq!(lockfile.len(), 1);
        assert!(lockfile.get("azure").is_none());

        // Removing non-existent returns None
        let removed = lockfile.remove_provider("gcp");
        assert!(removed.is_none());
    }

    #[test]
    fn test_lockfile_provider_names() {
        let lockfile = ProviderLockfile::generate(&sample_providers());
        let mut names = lockfile.provider_names();
        names.sort();
        assert_eq!(names, vec!["aws", "azure"]);
    }

    #[tokio::test]
    async fn test_lockfile_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join(".rustible/provider.lock.json");

        let providers = sample_providers();
        let mut lockfile = ProviderLockfile::generate(&providers);
        lockfile.save(&lock_path).await.unwrap();

        let loaded = ProviderLockfile::load(&lock_path).await.unwrap();

        assert_eq!(loaded.version, lockfile.version);
        assert_eq!(loaded.len(), lockfile.len());

        let aws = loaded.get("aws").unwrap();
        assert_eq!(aws.version, "5.31.0");
        assert_eq!(aws.hashes.len(), 2);

        let azure = loaded.get("azure").unwrap();
        assert_eq!(azure.version, "3.75.0");
    }

    #[tokio::test]
    async fn test_lockfile_load_nonexistent() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join(".rustible/nonexistent.lock.json");

        let lockfile = ProviderLockfile::load(&lock_path).await.unwrap();
        assert!(lockfile.is_empty());
    }

    #[test]
    fn test_lockfile_serialization() {
        let lockfile = ProviderLockfile::generate(&sample_providers());
        let json = serde_json::to_string_pretty(&lockfile).unwrap();
        let deserialized: ProviderLockfile = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.len(), lockfile.len());
        assert_eq!(
            deserialized.get("aws").unwrap().version,
            lockfile.get("aws").unwrap().version
        );
    }

    #[test]
    fn test_validation_error_display() {
        let err = LockfileValidationError {
            provider: "aws".to_string(),
            kind: LockfileMismatchKind::VersionMismatch {
                locked: "5.31.0".to_string(),
                current: "5.32.0".to_string(),
            },
        };
        let display = format!("{}", err);
        assert!(display.contains("aws"));
        assert!(display.contains("5.31.0"));
        assert!(display.contains("5.32.0"));
    }

    #[test]
    fn test_empty_lockfile_validate_with_providers() {
        let lockfile = ProviderLockfile::new();
        let providers = sample_providers();

        let errors = lockfile.validate(&providers);

        // All providers should be "new"
        assert_eq!(errors.len(), 2);
        assert!(errors
            .iter()
            .all(|e| e.kind == LockfileMismatchKind::NewProvider));
    }

    #[test]
    fn test_lockfile_validate_empty_hashes_skip_check() {
        let providers = vec![ProviderInfo::new("aws", "5.31.0", "~> 5.0", vec![])];
        let lockfile = ProviderLockfile::generate(&providers);

        // Validate with different hashes but empty locked hashes should pass
        let current = vec![ProviderInfo::new(
            "aws",
            "5.31.0",
            "~> 5.0",
            vec!["h1:anything".to_string()],
        )];

        let errors = lockfile.validate(&current);
        assert!(
            errors.is_empty(),
            "Empty locked hashes should skip hash check"
        );
    }
}
