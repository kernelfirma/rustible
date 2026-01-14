//! Advanced Secret Management for Rustible
//!
//! This module provides comprehensive secret management capabilities including:
//!
//! - **HashiCorp Vault integration**: Full support for Vault KV v1/v2, AppRole, and Token auth
//! - **AWS Secrets Manager integration**: Native AWS SDK integration with IAM authentication
//! - **Secret rotation**: Automatic and manual secret rotation with configurable policies
//! - **No-log enforcement**: Automatic redaction of sensitive data in logs and output
//! - **Caching**: TTL-based caching to reduce API calls to secret backends
//!
//! ## Architecture
//!
//! ```text
//! +-------------------+
//! |  SecretManager    |
//! +-------------------+
//!         |
//!         v
//! +-------------------+     +-------------------+
//! | SecretBackend     |<--->| SecretCache       |
//! | (trait)           |     | (TTL-based)       |
//! +-------------------+     +-------------------+
//!         ^
//!         |
//!    +----+----+
//!    |         |
//!    v         v
//! +-------+ +--------+
//! | Vault | | AWS SM |
//! +-------+ +--------+
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use rustible::secrets::{SecretManager, VaultBackend, SecretConfig};
//!
//! // Create a secret manager with Vault backend
//! let config = SecretConfig::vault()
//!     .address("https://vault.example.com:8200")
//!     .token_from_env()
//!     .build()?;
//!
//! let manager = SecretManager::new(config).await?;
//!
//! // Fetch a secret
//! let secret = manager.get("secret/data/myapp/database").await?;
//!
//! // Access secret values with no_log protection
//! let password = secret.get_sensitive("password")?;
//! ```

mod backend;
mod cache;
mod config;
mod error;
mod no_log;
mod rotation;
mod types;

// Backend implementations
mod aws_secrets_manager;
mod hashicorp_vault;

// Vault integration module (Issue #87 - AWX/Tower API Compatibility)
pub mod vault;

// Re-exports
pub use backend::{SecretBackend, SecretBackendType};
pub use cache::{SecretCache, SecretCacheConfig};
pub use config::{
    AwsSecretsManagerConfig, SecretConfig, SecretConfigBuilder, VaultAuthMethod, VaultConfig,
};
pub use error::{SecretError, SecretResult};
pub use no_log::{NoLogGuard, NoLogRegistry, SensitiveString};
pub use rotation::{RotationConfig, RotationPolicy, RotationResult, SecretRotator};
pub use types::{Secret, SecretMetadata, SecretValue, SecretVersion};

// Backend implementations
pub use aws_secrets_manager::AwsSecretsManagerBackend;
pub use hashicorp_vault::VaultBackend;

use std::sync::Arc;
use tokio::sync::RwLock;

/// Central manager for secret operations.
///
/// The `SecretManager` provides a unified interface for fetching, caching,
/// and rotating secrets from various backends (HashiCorp Vault, AWS Secrets Manager, etc.).
///
/// ## Features
///
/// - Automatic caching with configurable TTL
/// - Secret rotation with configurable policies
/// - No-log enforcement for sensitive data
/// - Support for multiple backends simultaneously
///
/// ## Example
///
/// ```rust,ignore
/// use rustible::secrets::{SecretManager, SecretConfig};
///
/// let config = SecretConfig::vault()
///     .address("https://vault.example.com:8200")
///     .token("hvs.example_token")
///     .build()?;
///
/// let manager = SecretManager::new(config).await?;
///
/// // Fetch a secret (cached automatically)
/// let db_secret = manager.get("secret/data/database").await?;
/// let password = db_secret.get_sensitive("password")?;
/// ```
pub struct SecretManager {
    /// The secret backend (Vault, AWS, etc.)
    backend: Arc<dyn SecretBackend>,

    /// Secret cache for reducing API calls
    cache: Arc<RwLock<SecretCache>>,

    /// Secret rotator for automatic rotation
    rotator: Option<Arc<SecretRotator>>,

    /// No-log registry for sensitive data redaction
    no_log_registry: Arc<NoLogRegistry>,

    /// Configuration
    config: SecretConfig,
}

impl SecretManager {
    /// Create a new SecretManager with the given configuration.
    ///
    /// This will initialize the appropriate backend based on the configuration
    /// and set up caching and rotation if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The backend cannot be initialized
    /// - Authentication fails
    /// - Configuration is invalid
    pub async fn new(config: SecretConfig) -> SecretResult<Self> {
        let backend: Arc<dyn SecretBackend> = match &config.backend_type {
            SecretBackendType::Vault => {
                let vault_config = config
                    .vault
                    .as_ref()
                    .ok_or_else(|| SecretError::Configuration("Vault config required".into()))?;
                Arc::new(VaultBackend::new(vault_config.clone()).await?)
            }
            SecretBackendType::AwsSecretsManager => {
                let aws_config = config
                    .aws
                    .as_ref()
                    .ok_or_else(|| SecretError::Configuration("AWS config required".into()))?;
                Arc::new(AwsSecretsManagerBackend::new(aws_config.clone()).await?)
            }
        };

        let cache = Arc::new(RwLock::new(SecretCache::new(
            config.cache.clone().unwrap_or_default(),
        )));

        let rotator = if let Some(rotation_config) = &config.rotation {
            Some(Arc::new(SecretRotator::new(
                rotation_config.clone(),
                backend.clone(),
            )))
        } else {
            None
        };

        let no_log_registry = Arc::new(NoLogRegistry::new());

        Ok(Self {
            backend,
            cache,
            rotator,
            no_log_registry,
            config,
        })
    }

    /// Get a secret by path/name.
    ///
    /// This will first check the cache, and if not found or expired,
    /// will fetch from the backend and cache the result.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path (format depends on backend)
    ///   - Vault: `secret/data/myapp/database`
    ///   - AWS: `myapp/database` or ARN
    ///
    /// # Returns
    ///
    /// Returns the secret with all its key-value pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The secret is not found
    /// - Authentication/authorization fails
    /// - Network error occurs
    pub async fn get(&self, path: &str) -> SecretResult<Secret> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(secret) = cache.get(path) {
                tracing::debug!(path = %path, "Secret retrieved from cache");
                return Ok(secret);
            }
        }

        // Fetch from backend
        let secret = self.backend.get_secret(path).await?;

        // Register sensitive values for no_log
        self.register_sensitive_values(&secret);

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(path.to_string(), secret.clone());
        }

        tracing::debug!(path = %path, "Secret fetched from backend and cached");
        Ok(secret)
    }

    /// Get a specific key from a secret.
    ///
    /// Convenience method that fetches the secret and extracts a specific key.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path
    /// * `key` - The key within the secret
    ///
    /// # Returns
    ///
    /// Returns a `SensitiveString` that is automatically redacted in logs.
    pub async fn get_value(&self, path: &str, key: &str) -> SecretResult<SensitiveString> {
        let secret = self.get(path).await?;
        secret.get_sensitive(key)
    }

    /// List secrets at a path.
    ///
    /// Returns a list of secret names/paths under the given path.
    /// Note: This does not return the actual secret values.
    pub async fn list(&self, path: &str) -> SecretResult<Vec<String>> {
        self.backend.list_secrets(path).await
    }

    /// Write a secret to the backend.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path
    /// * `secret` - The secret data to write
    ///
    /// # Note
    ///
    /// This operation invalidates any cached version of the secret.
    pub async fn put(&self, path: &str, secret: Secret) -> SecretResult<()> {
        self.backend.put_secret(path, &secret).await?;

        // Invalidate cache
        {
            let mut cache = self.cache.write().await;
            cache.invalidate(path);
        }

        // Register new sensitive values
        self.register_sensitive_values(&secret);

        tracing::info!(path = %path, "Secret written successfully");
        Ok(())
    }

    /// Delete a secret from the backend.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path to delete
    pub async fn delete(&self, path: &str) -> SecretResult<()> {
        self.backend.delete_secret(path).await?;

        // Invalidate cache
        {
            let mut cache = self.cache.write().await;
            cache.invalidate(path);
        }

        tracing::info!(path = %path, "Secret deleted successfully");
        Ok(())
    }

    /// Rotate a secret.
    ///
    /// This will use the configured rotation policy to generate a new
    /// secret value and update the backend.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path to rotate
    ///
    /// # Returns
    ///
    /// Returns the rotation result including the new secret version.
    pub async fn rotate(&self, path: &str) -> SecretResult<RotationResult> {
        let rotator = self
            .rotator
            .as_ref()
            .ok_or_else(|| SecretError::Configuration("Rotation not configured".into()))?;

        let result = rotator.rotate(path).await?;

        // Invalidate cache after rotation
        {
            let mut cache = self.cache.write().await;
            cache.invalidate(path);
        }

        tracing::info!(
            path = %path,
            old_version = ?result.old_version,
            new_version = ?result.new_version,
            "Secret rotated successfully"
        );

        Ok(result)
    }

    /// Check if a string contains any registered sensitive values.
    ///
    /// This is used for no_log enforcement.
    pub fn contains_sensitive(&self, text: &str) -> bool {
        self.no_log_registry.contains_sensitive(text)
    }

    /// Redact all sensitive values from a string.
    ///
    /// Replaces any registered sensitive values with `[REDACTED]`.
    pub fn redact(&self, text: &str) -> String {
        self.no_log_registry.redact(text)
    }

    /// Get the no_log registry for external use.
    pub fn no_log_registry(&self) -> Arc<NoLogRegistry> {
        self.no_log_registry.clone()
    }

    /// Invalidate all cached secrets.
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        tracing::debug!("Secret cache cleared");
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        cache.stats()
    }

    /// Check backend health.
    ///
    /// Returns `true` if the backend is reachable and authenticated.
    pub async fn health_check(&self) -> SecretResult<bool> {
        self.backend.health_check().await
    }

    /// Register sensitive values from a secret in the no_log registry.
    fn register_sensitive_values(&self, secret: &Secret) {
        for (_, value) in secret.data() {
            if let SecretValue::String(s) = value {
                self.no_log_registry.register(s.clone());
            }
        }
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cached secrets
    pub size: usize,
    /// Cache hits
    pub hits: u64,
    /// Cache misses
    pub misses: u64,
    /// Hit rate (0.0 - 1.0)
    pub hit_rate: f64,
}

impl std::fmt::Debug for SecretManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretManager")
            .field("backend_type", &self.config.backend_type)
            .field("rotation_enabled", &self.rotator.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_manager_debug() {
        // Just ensure Debug trait is implemented correctly
        let config = SecretConfig::default();
        assert!(format!("{:?}", config).contains("SecretConfig"));
    }
}
