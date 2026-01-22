//! HashiCorp Vault Integration Module
//!
//! This module provides a simplified interface for HashiCorp Vault integration,
//! supporting common authentication methods and secret retrieval patterns.
//!
//! ## Features
//!
//! - **Multiple Auth Methods**: Token, AppRole, and Kubernetes authentication
//! - **Namespace Support**: Vault Enterprise namespace isolation
//! - **Environment-based Configuration**: Token retrieval from environment variables
//!
//! ## Configuration
//!
//! ```toml
//! [vault]
//! provider = "hashicorp"
//! address = "https://vault.example.com"
//! auth = "token"
//! token_env = "VAULT_TOKEN"
//! namespace = "team-a"
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::secrets::vault::{VaultAuthMethod, VaultConfig, VaultProvider};
//!
//! let config = VaultConfig::new("https://vault.example.com:8200")
//!     .with_auth_method(VaultAuthMethod::Token)
//!     .with_namespace("my-namespace")
//!     .with_token_env("VAULT_TOKEN");
//!
//! let provider = VaultProvider::new(config).await?;
//! let secret = provider.get_secret("secret/data/myapp").await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::error::{SecretError, SecretResult};
use super::types::Secret;

// ============================================================================
// Constants - AWX/Tower API Compatibility (Issue #87)
// ============================================================================

/// Default Vault address when not specified
pub const DEFAULT_VAULT_ADDRESS: &str = "http://127.0.0.1:8200";

/// Default environment variable for Vault token
pub const DEFAULT_TOKEN_ENV: &str = "VAULT_TOKEN";

/// Vault API version prefix
pub const VAULT_API_V1: &str = "/v1";

// ============================================================================
// VaultAuthMethod
// ============================================================================

/// Authentication method for connecting to Vault.
///
/// Supports the most common authentication methods:
/// - Token: Direct token authentication (simplest)
/// - AppRole: Recommended for applications and automation
/// - Kubernetes: For workloads running in Kubernetes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VaultAuthMethod {
    /// Token-based authentication.
    ///
    /// The token can be provided directly or read from an environment variable.
    #[default]
    Token,

    /// AppRole authentication (recommended for applications).
    ///
    /// Requires `role_id` and `secret_id` to be configured.
    AppRole {
        /// The Role ID for AppRole authentication
        role_id: String,
        /// The Secret ID for AppRole authentication
        secret_id: String,
        /// Mount path for AppRole auth (default: "approle")
        #[serde(default = "default_approle_mount")]
        mount_path: String,
    },

    /// Kubernetes authentication for K8s workloads.
    ///
    /// Uses the service account JWT token for authentication.
    Kubernetes {
        /// The Kubernetes auth role
        role: String,
        /// Path to the JWT token file (default: K8s service account path)
        #[serde(default = "default_k8s_jwt_path")]
        jwt_path: String,
        /// Mount path for Kubernetes auth (default: "kubernetes")
        #[serde(default = "default_kubernetes_mount")]
        mount_path: String,
    },
}

fn default_approle_mount() -> String {
    "approle".to_string()
}

fn default_kubernetes_mount() -> String {
    "kubernetes".to_string()
}

fn default_k8s_jwt_path() -> String {
    "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string()
}

// ============================================================================
// VaultConfig
// ============================================================================

/// Configuration for connecting to HashiCorp Vault.
///
/// This struct holds all configuration needed to establish a connection
/// to a Vault server, including address, authentication, and optional
/// namespace for Vault Enterprise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Vault server address (e.g., "https://vault.example.com:8200")
    #[serde(default = "default_vault_address")]
    pub address: String,

    /// Authentication method to use
    #[serde(default)]
    pub auth_method: VaultAuthMethod,

    /// Vault namespace (Vault Enterprise feature)
    #[serde(default)]
    pub namespace: Option<String>,

    /// Environment variable name containing the Vault token
    ///
    /// Used when `auth_method` is `Token`. Defaults to "VAULT_TOKEN".
    #[serde(default = "default_token_env")]
    pub token_env: String,
}

fn default_vault_address() -> String {
    std::env::var("VAULT_ADDR").unwrap_or_else(|_| DEFAULT_VAULT_ADDRESS.to_string())
}

fn default_token_env() -> String {
    DEFAULT_TOKEN_ENV.to_string()
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            address: default_vault_address(),
            auth_method: VaultAuthMethod::default(),
            namespace: None,
            token_env: default_token_env(),
        }
    }
}

impl VaultConfig {
    /// Create a new VaultConfig with the specified address.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            ..Default::default()
        }
    }

    /// Set the authentication method.
    pub fn with_auth_method(mut self, auth_method: VaultAuthMethod) -> Self {
        self.auth_method = auth_method;
        self
    }

    /// Set the namespace (Vault Enterprise).
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the token environment variable name.
    pub fn with_token_env(mut self, token_env: impl Into<String>) -> Self {
        self.token_env = token_env.into();
        self
    }
}

// ============================================================================
// VaultClient Trait
// ============================================================================

/// Trait for Vault client implementations.
///
/// This trait defines the interface for interacting with HashiCorp Vault.
/// It can be implemented by different HTTP clients or mocked for testing.
#[async_trait]
pub trait VaultClient: Send + Sync {
    /// Retrieve a secret from the specified path.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path (e.g., "secret/data/myapp/database")
    ///
    /// # Returns
    ///
    /// The secret data as a key-value map.
    async fn get_secret(&self, path: &str) -> SecretResult<HashMap<String, String>>;

    /// List secrets at the specified path.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to list (e.g., "secret/metadata/myapp/")
    ///
    /// # Returns
    ///
    /// A list of secret keys at the path.
    async fn list_secrets(&self, path: &str) -> SecretResult<Vec<String>>;

    /// Write a secret to the specified path.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path
    /// * `data` - The secret data to write
    async fn put_secret(&self, path: &str, data: HashMap<String, String>) -> SecretResult<()>;

    /// Delete a secret at the specified path.
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path to delete
    async fn delete_secret(&self, path: &str) -> SecretResult<()>;

    /// Check if the Vault server is healthy and reachable.
    async fn health_check(&self) -> SecretResult<bool>;
}

// ============================================================================
// HttpVaultClient
// ============================================================================

/// HTTP-based Vault client implementation.
///
/// This client uses the Vault HTTP API to perform secret operations.
/// It handles authentication, token renewal, and request/response processing.
pub struct HttpVaultClient {
    config: VaultConfig,
    token: Option<String>,
}

impl HttpVaultClient {
    /// Create a new HTTP Vault client with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Vault configuration
    ///
    /// # Returns
    ///
    /// A new `HttpVaultClient` instance.
    pub async fn new(config: VaultConfig) -> SecretResult<Self> {
        let mut client = Self {
            config,
            token: None,
        };
        client.authenticate().await?;
        Ok(client)
    }

    /// Authenticate with Vault using the configured auth method.
    async fn authenticate(&mut self) -> SecretResult<()> {
        match &self.config.auth_method {
            VaultAuthMethod::Token => {
                let token = std::env::var(&self.config.token_env).map_err(|_| {
                    SecretError::Authentication(format!(
                        "Token environment variable '{}' not set",
                        self.config.token_env
                    ))
                })?;
                self.token = Some(token);
            }
            VaultAuthMethod::AppRole { .. } => {
                // TODO: Implement AppRole authentication
                return Err(SecretError::Authentication(
                    "AppRole authentication not yet implemented".into(),
                ));
            }
            VaultAuthMethod::Kubernetes { .. } => {
                // TODO: Implement Kubernetes authentication
                return Err(SecretError::Authentication(
                    "Kubernetes authentication not yet implemented".into(),
                ));
            }
        }
        Ok(())
    }

    /// Get the current authentication token.
    fn get_token(&self) -> SecretResult<&str> {
        self.token
            .as_deref()
            .ok_or_else(|| SecretError::Authentication("Not authenticated".into()))
    }
}

#[async_trait]
impl VaultClient for HttpVaultClient {
    async fn get_secret(&self, path: &str) -> SecretResult<HashMap<String, String>> {
        let _token = self.get_token()?;
        // TODO: Implement HTTP request to Vault
        // For now, return a placeholder error
        Err(SecretError::NotFound(format!(
            "Secret retrieval not yet implemented for path: {}",
            path
        )))
    }

    async fn list_secrets(&self, path: &str) -> SecretResult<Vec<String>> {
        let _token = self.get_token()?;
        // TODO: Implement HTTP request to Vault
        Err(SecretError::NotFound(format!(
            "Secret listing not yet implemented for path: {}",
            path
        )))
    }

    async fn put_secret(&self, path: &str, _data: HashMap<String, String>) -> SecretResult<()> {
        let _token = self.get_token()?;
        // TODO: Implement HTTP request to Vault
        Err(SecretError::NotFound(format!(
            "Secret write not yet implemented for path: {}",
            path
        )))
    }

    async fn delete_secret(&self, path: &str) -> SecretResult<()> {
        let _token = self.get_token()?;
        // TODO: Implement HTTP request to Vault
        Err(SecretError::NotFound(format!(
            "Secret delete not yet implemented for path: {}",
            path
        )))
    }

    async fn health_check(&self) -> SecretResult<bool> {
        // TODO: Implement health check via /v1/sys/health
        Ok(false)
    }
}

impl std::fmt::Debug for HttpVaultClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpVaultClient")
            .field("address", &self.config.address)
            .field("namespace", &self.config.namespace)
            .field("has_token", &self.token.is_some())
            .finish()
    }
}

// ============================================================================
// VaultProvider
// ============================================================================

/// High-level Vault provider for secret management.
///
/// This provider wraps a `VaultClient` and provides additional features
/// such as path resolution for lookups and integration with the secret
/// management system.
pub struct VaultProvider<C: VaultClient = HttpVaultClient> {
    client: C,
    config: VaultConfig,
}

impl VaultProvider<HttpVaultClient> {
    /// Create a new VaultProvider with the default HTTP client.
    ///
    /// # Arguments
    ///
    /// * `config` - Vault configuration
    ///
    /// # Returns
    ///
    /// A new `VaultProvider` instance with HTTP client.
    pub async fn new(config: VaultConfig) -> SecretResult<Self> {
        let client = HttpVaultClient::new(config.clone()).await?;
        Ok(Self { client, config })
    }
}

impl<C: VaultClient> VaultProvider<C> {
    /// Create a new VaultProvider with a custom client.
    ///
    /// This is useful for testing or for using alternative HTTP clients.
    pub fn with_client(config: VaultConfig, client: C) -> Self {
        Self { client, config }
    }

    /// Get a secret from Vault.
    ///
    /// Supports the following path formats:
    /// - `secret/path#key` - Get a specific key from the secret
    /// - `secret/path` - Get all keys from the secret
    ///
    /// # Arguments
    ///
    /// * `path` - The secret path, optionally with a `#key` suffix
    pub async fn get_secret(&self, path: &str) -> SecretResult<Secret> {
        let (secret_path, key) = parse_secret_path(path);
        let data = self.client.get_secret(secret_path).await?;

        if let Some(key) = key {
            // Return only the specified key
            let value = data.get(key).ok_or_else(|| {
                SecretError::NotFound(format!("Key '{}' not found in secret '{}'", key, secret_path))
            })?;
            let mut filtered = HashMap::new();
            filtered.insert(key.to_string(), value.clone());
            Ok(Secret::from_string_map(secret_path, filtered))
        } else {
            Ok(Secret::from_string_map(secret_path, data))
        }
    }

    /// List secrets at a path.
    pub async fn list(&self, path: &str) -> SecretResult<Vec<String>> {
        self.client.list_secrets(path).await
    }

    /// Write a secret to Vault.
    pub async fn put_secret(&self, path: &str, data: HashMap<String, String>) -> SecretResult<()> {
        self.client.put_secret(path, data).await
    }

    /// Delete a secret from Vault.
    pub async fn delete_secret(&self, path: &str) -> SecretResult<()> {
        self.client.delete_secret(path).await
    }

    /// Check Vault health.
    pub async fn health_check(&self) -> SecretResult<bool> {
        self.client.health_check().await
    }

    /// Get the Vault configuration.
    pub fn config(&self) -> &VaultConfig {
        &self.config
    }
}

impl<C: VaultClient> std::fmt::Debug for VaultProvider<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultProvider")
            .field("address", &self.config.address)
            .field("namespace", &self.config.namespace)
            .finish()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse a secret path that may include a key suffix.
///
/// Format: `secret/path#key` or `secret/path`
///
/// # Returns
///
/// A tuple of (path, optional_key)
fn parse_secret_path(path: &str) -> (&str, Option<&str>) {
    if let Some(idx) = path.rfind('#') {
        (&path[..idx], Some(&path[idx + 1..]))
    } else {
        (path, None)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_auth_method_default() {
        let auth = VaultAuthMethod::default();
        assert_eq!(auth, VaultAuthMethod::Token);
    }

    #[test]
    fn test_vault_config_default() {
        let config = VaultConfig::default();
        assert_eq!(config.token_env, DEFAULT_TOKEN_ENV);
        assert!(config.namespace.is_none());
    }

    #[test]
    fn test_vault_config_builder() {
        let config = VaultConfig::new("https://vault.example.com:8200")
            .with_namespace("my-namespace")
            .with_token_env("MY_VAULT_TOKEN");

        assert_eq!(config.address, "https://vault.example.com:8200");
        assert_eq!(config.namespace, Some("my-namespace".to_string()));
        assert_eq!(config.token_env, "MY_VAULT_TOKEN");
    }

    #[test]
    fn test_parse_secret_path_with_key() {
        let (path, key) = parse_secret_path("secret/data/myapp#password");
        assert_eq!(path, "secret/data/myapp");
        assert_eq!(key, Some("password"));
    }

    #[test]
    fn test_parse_secret_path_without_key() {
        let (path, key) = parse_secret_path("secret/data/myapp");
        assert_eq!(path, "secret/data/myapp");
        assert_eq!(key, None);
    }

    #[test]
    fn test_vault_auth_method_approle() {
        let auth = VaultAuthMethod::AppRole {
            role_id: "my-role".to_string(),
            secret_id: "my-secret".to_string(),
            mount_path: "approle".to_string(),
        };

        if let VaultAuthMethod::AppRole {
            role_id,
            secret_id,
            mount_path,
        } = auth
        {
            assert_eq!(role_id, "my-role");
            assert_eq!(secret_id, "my-secret");
            assert_eq!(mount_path, "approle");
        } else {
            panic!("Expected AppRole variant");
        }
    }

    #[test]
    fn test_vault_auth_method_kubernetes() {
        let auth = VaultAuthMethod::Kubernetes {
            role: "my-k8s-role".to_string(),
            jwt_path: "/custom/token/path".to_string(),
            mount_path: "kubernetes".to_string(),
        };

        if let VaultAuthMethod::Kubernetes {
            role,
            jwt_path,
            mount_path,
        } = auth
        {
            assert_eq!(role, "my-k8s-role");
            assert_eq!(jwt_path, "/custom/token/path");
            assert_eq!(mount_path, "kubernetes");
        } else {
            panic!("Expected Kubernetes variant");
        }
    }

    // Async tests would require tokio test runtime
    // #[tokio::test]
    // async fn test_http_vault_client_authentication_error() {
    //     let config = VaultConfig::new("http://localhost:8200");
    //     let result = HttpVaultClient::new(config).await;
    //     assert!(result.is_err());
    // }
}
