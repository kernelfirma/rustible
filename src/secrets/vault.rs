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
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use rustible::secrets::vault::{VaultProvider, VaultConfig, VaultAuthMethod};
//!
//! let config = VaultConfig {
//!     address: "https://vault.example.com:8200".to_string(),
//!     auth_method: VaultAuthMethod::Token,
//!     namespace: Some("my-namespace".to_string()),
//!     token_env: "VAULT_TOKEN".to_string(),
//! };
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
// Vault Auth Response Types (for AppRole/Kubernetes login)
// ============================================================================

/// Minimal Vault login response envelope used for AppRole and Kubernetes auth.
#[derive(Debug, Deserialize)]
struct VaultLoginResponse {
    auth: Option<VaultLoginAuth>,
}

/// Auth block within a Vault login response.
#[derive(Debug, Deserialize)]
struct VaultLoginAuth {
    client_token: String,
}

/// Vault secret response envelope for get operations.
#[derive(Debug, Deserialize)]
struct VaultSecretResponse {
    data: HashMap<String, serde_json::Value>,
}

/// Vault list response envelope.
#[derive(Debug, Deserialize)]
struct VaultListResponse {
    data: Option<VaultListData>,
}

/// Data block for list responses.
#[derive(Debug, Deserialize)]
struct VaultListData {
    keys: Option<Vec<String>>,
}

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
            VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path,
            } => {
                let url = format!(
                    "{}/v1/auth/{}/login",
                    self.config.address.trim_end_matches('/'),
                    mount_path
                );

                let body = serde_json::json!({
                    "role_id": role_id,
                    "secret_id": secret_id
                });

                let client = reqwest::Client::new();
                let response = client.post(&url).json(&body).send().await.map_err(|e| {
                    SecretError::Connection(format!("AppRole auth request failed: {}", e))
                })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    return Err(SecretError::Authentication(format!(
                        "AppRole login failed (HTTP {}): {}",
                        status, body_text
                    )));
                }

                let vault_resp: VaultLoginResponse = response.json().await.map_err(|e| {
                    SecretError::Authentication(format!(
                        "Failed to parse AppRole auth response: {}",
                        e
                    ))
                })?;

                let token = vault_resp
                    .auth
                    .ok_or_else(|| {
                        SecretError::Authentication(
                            "AppRole auth response missing 'auth' block".into(),
                        )
                    })?
                    .client_token;

                self.token = Some(token);
            }
            VaultAuthMethod::Kubernetes {
                role,
                jwt_path,
                mount_path,
            } => {
                let jwt = std::fs::read_to_string(jwt_path).map_err(|e| {
                    SecretError::Configuration(format!(
                        "Failed to read Kubernetes JWT from '{}': {}",
                        jwt_path, e
                    ))
                })?;

                let url = format!(
                    "{}/v1/auth/{}/login",
                    self.config.address.trim_end_matches('/'),
                    mount_path
                );

                let body = serde_json::json!({
                    "role": role,
                    "jwt": jwt.trim()
                });

                let client = reqwest::Client::new();
                let response = client.post(&url).json(&body).send().await.map_err(|e| {
                    SecretError::Connection(format!("Kubernetes auth request failed: {}", e))
                })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    return Err(SecretError::Authentication(format!(
                        "Kubernetes login failed (HTTP {}): {}",
                        status, body_text
                    )));
                }

                let vault_resp: VaultLoginResponse = response.json().await.map_err(|e| {
                    SecretError::Authentication(format!(
                        "Failed to parse Kubernetes auth response: {}",
                        e
                    ))
                })?;

                let token = vault_resp
                    .auth
                    .ok_or_else(|| {
                        SecretError::Authentication(
                            "Kubernetes auth response missing 'auth' block".into(),
                        )
                    })?
                    .client_token;

                self.token = Some(token);
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
        let token = self.get_token()?;
        let url = format!(
            "{}/v1/{}",
            self.config.address.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let client = reqwest::Client::new();
        let mut request = client.get(&url).header("X-Vault-Token", token);

        // Add namespace header if configured
        if let Some(ref namespace) = self.config.namespace {
            request = request.header("X-Vault-Namespace", namespace);
        }

        let response = request.send().await.map_err(|e| {
            SecretError::Connection(format!("Failed to connect to Vault: {}", e))
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(SecretError::NotFound(format!("Secret not found: {}", path)));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(SecretError::Authorization(format!(
                "Permission denied for path: {}",
                path
            )));
        }
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(SecretError::backend(
                format!("Vault request failed: {}", body_text),
                Some(status.as_u16()),
            ));
        }

        let vault_response: VaultSecretResponse = response.json().await.map_err(|e| {
            SecretError::Serialization(format!("Failed to parse Vault response: {}", e))
        })?;

        // Handle both KV v1 and KV v2 response formats
        let data = if let Some(inner_data) = vault_response.data.get("data") {
            // KV v2: data is nested under {"data": {"data": {...}}}
            if let Some(obj) = inner_data.as_object() {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                HashMap::new()
            }
        } else {
            // KV v1: data is directly at {"data": {...}}
            vault_response
                .data
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        };

        Ok(data)
    }

    async fn list_secrets(&self, path: &str) -> SecretResult<Vec<String>> {
        let token = self.get_token()?;

        // For listing, use metadata path for KV v2 or direct path for KV v1
        let list_path = if path.contains("/data/") {
            path.replace("/data/", "/metadata/")
        } else {
            path.to_string()
        };

        let url = format!(
            "{}/v1/{}",
            self.config.address.trim_end_matches('/'),
            list_path.trim_start_matches('/')
        );

        let client = reqwest::Client::new();
        let mut request = client
            .request(reqwest::Method::from_bytes(b"LIST").unwrap_or(reqwest::Method::GET), &url)
            .header("X-Vault-Token", token);

        // Fallback: use GET with list=true query param
        let url_with_list = format!("{}?list=true", url);
        let mut request_fallback = client
            .get(&url_with_list)
            .header("X-Vault-Token", token);

        if let Some(ref namespace) = self.config.namespace {
            request = request.header("X-Vault-Namespace", namespace);
            request_fallback = request_fallback.header("X-Vault-Namespace", namespace);
        }

        // Try LIST method first, fall back to GET with list=true
        let response = match request.send().await {
            Ok(resp) if resp.status().is_success() => resp,
            _ => request_fallback.send().await.map_err(|e| {
                SecretError::Connection(format!("Failed to list secrets: {}", e))
            })?,
        };

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(vec![]); // Empty list for non-existent paths
        }
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(SecretError::backend(
                format!("Vault list request failed: {}", body_text),
                Some(status.as_u16()),
            ));
        }

        let vault_response: VaultListResponse = response.json().await.map_err(|e| {
            SecretError::Serialization(format!("Failed to parse Vault list response: {}", e))
        })?;

        Ok(vault_response
            .data
            .and_then(|d| d.keys)
            .unwrap_or_default())
    }

    async fn put_secret(&self, path: &str, data: HashMap<String, String>) -> SecretResult<()> {
        let token = self.get_token()?;
        let url = format!(
            "{}/v1/{}",
            self.config.address.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        // Determine if this is KV v2 (path contains /data/)
        let body = if path.contains("/data/") {
            // KV v2: wrap data in {"data": {...}}
            serde_json::json!({ "data": data })
        } else {
            // KV v1: send data directly
            serde_json::to_value(&data).map_err(|e| {
                SecretError::Serialization(format!("Failed to serialize secret data: {}", e))
            })?
        };

        let client = reqwest::Client::new();
        let mut request = client
            .post(&url)
            .header("X-Vault-Token", token)
            .json(&body);

        if let Some(ref namespace) = self.config.namespace {
            request = request.header("X-Vault-Namespace", namespace);
        }

        let response = request.send().await.map_err(|e| {
            SecretError::Connection(format!("Failed to write secret: {}", e))
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(SecretError::Authorization(format!(
                "Permission denied for path: {}",
                path
            )));
        }
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(SecretError::backend(
                format!("Vault write request failed: {}", body_text),
                Some(status.as_u16()),
            ));
        }

        Ok(())
    }

    async fn delete_secret(&self, path: &str) -> SecretResult<()> {
        let token = self.get_token()?;

        // For KV v2, use metadata path for permanent deletion
        let delete_path = if path.contains("/data/") {
            path.replace("/data/", "/metadata/")
        } else {
            path.to_string()
        };

        let url = format!(
            "{}/v1/{}",
            self.config.address.trim_end_matches('/'),
            delete_path.trim_start_matches('/')
        );

        let client = reqwest::Client::new();
        let mut request = client.delete(&url).header("X-Vault-Token", token);

        if let Some(ref namespace) = self.config.namespace {
            request = request.header("X-Vault-Namespace", namespace);
        }

        let response = request.send().await.map_err(|e| {
            SecretError::Connection(format!("Failed to delete secret: {}", e))
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            // Already deleted or never existed
            return Ok(());
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(SecretError::Authorization(format!(
                "Permission denied for path: {}",
                path
            )));
        }
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(SecretError::backend(
                format!("Vault delete request failed: {}", body_text),
                Some(status.as_u16()),
            ));
        }

        Ok(())
    }

    async fn health_check(&self) -> SecretResult<bool> {
        let url = format!(
            "{}/v1/sys/health",
            self.config.address.trim_end_matches('/')
        );

        let client = reqwest::Client::new();
        let mut request = client.get(&url);

        if let Some(ref namespace) = self.config.namespace {
            request = request.header("X-Vault-Namespace", namespace);
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                return Err(SecretError::Connection(format!(
                    "Failed to reach Vault health endpoint: {}",
                    e
                )));
            }
        };

        // Vault health endpoint returns:
        // - 200: initialized, unsealed, active
        // - 429: unsealed, standby
        // - 472: disaster recovery secondary, active
        // - 473: performance standby
        // - 501: not initialized
        // - 503: sealed
        let status = response.status();
        match status.as_u16() {
            200 | 429 | 472 | 473 => Ok(true),
            501 => Err(SecretError::Configuration("Vault is not initialized".into())),
            503 => Err(SecretError::Sealed("Vault is sealed".into())),
            _ => {
                let body_text = response.text().await.unwrap_or_default();
                Err(SecretError::backend(
                    format!("Vault health check failed: {}", body_text),
                    Some(status.as_u16()),
                ))
            }
        }
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
                SecretError::NotFound(format!(
                    "Key '{}' not found in secret '{}'",
                    key, secret_path
                ))
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

    // Mock client for testing VaultClient trait
    struct MockVaultClient {
        secrets: std::sync::Mutex<HashMap<String, HashMap<String, String>>>,
    }

    impl MockVaultClient {
        fn new() -> Self {
            Self {
                secrets: std::sync::Mutex::new(HashMap::new()),
            }
        }

        fn with_secret(self, path: &str, data: HashMap<String, String>) -> Self {
            self.secrets.lock().unwrap().insert(path.to_string(), data);
            self
        }
    }

    #[async_trait]
    impl VaultClient for MockVaultClient {
        async fn get_secret(&self, path: &str) -> SecretResult<HashMap<String, String>> {
            self.secrets
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| SecretError::NotFound(path.to_string()))
        }

        async fn list_secrets(&self, _path: &str) -> SecretResult<Vec<String>> {
            Ok(self.secrets.lock().unwrap().keys().cloned().collect())
        }

        async fn put_secret(
            &self,
            path: &str,
            data: HashMap<String, String>,
        ) -> SecretResult<()> {
            self.secrets.lock().unwrap().insert(path.to_string(), data);
            Ok(())
        }

        async fn delete_secret(&self, path: &str) -> SecretResult<()> {
            self.secrets.lock().unwrap().remove(path);
            Ok(())
        }

        async fn health_check(&self) -> SecretResult<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_vault_provider_get_secret() {
        let mut data = HashMap::new();
        data.insert("username".to_string(), "admin".to_string());
        data.insert("password".to_string(), "secret123".to_string());

        let mock = MockVaultClient::new().with_secret("secret/data/myapp", data);
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        let secret = provider.get_secret("secret/data/myapp").await.unwrap();
        assert_eq!(secret.get_string("username").unwrap(), "admin");
        assert_eq!(secret.get_string("password").unwrap(), "secret123");
    }

    #[tokio::test]
    async fn test_vault_provider_get_secret_with_key() {
        let mut data = HashMap::new();
        data.insert("username".to_string(), "admin".to_string());
        data.insert("password".to_string(), "secret123".to_string());

        let mock = MockVaultClient::new().with_secret("secret/data/myapp", data);
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        let secret = provider
            .get_secret("secret/data/myapp#password")
            .await
            .unwrap();
        assert!(secret.contains_key("password"));
        assert!(!secret.contains_key("username"));
    }

    #[tokio::test]
    async fn test_vault_provider_not_found() {
        let mock = MockVaultClient::new();
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        let result = provider.get_secret("secret/data/nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecretError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_vault_provider_put_and_get() {
        let mock = MockVaultClient::new();
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        let mut data = HashMap::new();
        data.insert("api_key".to_string(), "sk-12345".to_string());

        provider
            .put_secret("secret/data/api", data)
            .await
            .unwrap();

        let secret = provider.get_secret("secret/data/api").await.unwrap();
        assert_eq!(secret.get_string("api_key").unwrap(), "sk-12345");
    }

    #[tokio::test]
    async fn test_vault_provider_delete() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), "value".to_string());

        let mock = MockVaultClient::new().with_secret("secret/data/temp", data);
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        // Verify secret exists
        assert!(provider.get_secret("secret/data/temp").await.is_ok());

        // Delete it
        provider.delete_secret("secret/data/temp").await.unwrap();

        // Verify it's gone
        assert!(provider.get_secret("secret/data/temp").await.is_err());
    }

    #[tokio::test]
    async fn test_vault_provider_health_check() {
        let mock = MockVaultClient::new();
        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        assert!(provider.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_vault_provider_list() {
        let mut data1 = HashMap::new();
        data1.insert("key".to_string(), "value1".to_string());
        let mut data2 = HashMap::new();
        data2.insert("key".to_string(), "value2".to_string());

        let mock = MockVaultClient::new()
            .with_secret("secret/data/app1", data1)
            .with_secret("secret/data/app2", data2);

        let config = VaultConfig::new("http://localhost:8200");
        let provider = VaultProvider::with_client(config, mock);

        let keys = provider.list("secret/metadata/").await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_vault_secret_response_parsing() {
        // Test KV v2 format
        let kv2_response = r#"{"data": {"data": {"username": "admin", "password": "secret"}, "metadata": {"version": 1}}}"#;
        let parsed: VaultSecretResponse = serde_json::from_str(kv2_response).unwrap();
        assert!(parsed.data.contains_key("data"));

        // Test KV v1 format
        let kv1_response = r#"{"data": {"username": "admin", "password": "secret"}}"#;
        let parsed: VaultSecretResponse = serde_json::from_str(kv1_response).unwrap();
        assert!(parsed.data.contains_key("username"));
    }

    #[test]
    fn test_vault_list_response_parsing() {
        let response = r#"{"data": {"keys": ["app1", "app2", "app3"]}}"#;
        let parsed: VaultListResponse = serde_json::from_str(response).unwrap();
        let keys = parsed.data.unwrap().keys.unwrap();
        assert_eq!(keys, vec!["app1", "app2", "app3"]);
    }
}
