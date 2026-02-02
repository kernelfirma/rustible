//! HashiCorp Vault Backend Implementation
//!
//! This module provides comprehensive integration with HashiCorp Vault including:
//!
//! - **KV Secrets Engine**: Full support for both v1 and v2 with versioning
//! - **Dynamic Credentials**: Database, AWS, and other dynamic secret engines
//! - **Transit Engine**: Encryption-as-a-service for data protection
//! - **Authentication Methods**: Token, AppRole, Kubernetes, LDAP, AWS IAM
//!
//! ## Architecture
//!
//! ```text
//! +------------------+
//! |  VaultBackend    |
//! +------------------+
//!         |
//!    +----+----+----+----+
//!    |    |    |    |    |
//!    v    v    v    v    v
//! +---+ +---+ +---+ +---+ +---+
//! |KV | |DB | |AWS| |TR | |AU |
//! |v1 | |dy | |dy | |an | |th |
//! |v2 | |na | |na | |si | |   |
//! +---+ +---+ +---+ +---+ +---+
//! ```
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::secrets::{SecretBackend, VaultAuthMethod, VaultBackend, VaultConfig};
//!
//! // Token authentication
//! let config = VaultConfig {
//!     address: "https://vault.example.com:8200".to_string(),
//!     auth: VaultAuthMethod::Token {
//!         token: "hvs.example_token".to_string(),
//!     },
//!     namespace: Some("my-namespace".to_string()),
//!     ..VaultConfig::default()
//! };
//!
//! let vault = VaultBackend::new(config.into()).await?;
//!
//! // KV v2 secret access
//! let secret = vault.get_secret("secret/data/myapp/database").await?;
//!
//! // Transit encryption
//! let encrypted = vault.transit_encrypt("my-key", "sensitive data").await?;
//! let decrypted = vault.transit_decrypt("my-key", &encrypted).await?;
//!
//! // Dynamic database credentials
//! let creds = vault.generate_database_credentials("my-role").await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::backend::{BackendCapabilities, BackendCapability, SecretBackend, SecretBackendType};
use super::error::{SecretError, SecretResult};
use super::types::{Secret, SecretMetadata, SecretValue, SecretVersion};

// ============================================================================
// Constants
// ============================================================================

/// Default Vault address
const DEFAULT_VAULT_ADDR: &str = "http://127.0.0.1:8200";

/// Default timeout for Vault requests
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Token refresh threshold (refresh when 20% of TTL remains)
const TOKEN_REFRESH_THRESHOLD: f64 = 0.2;

// ============================================================================
// Configuration Types
// ============================================================================

/// Vault authentication method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VaultAuthMethod {
    /// Token-based authentication (simplest)
    Token {
        /// The Vault token
        token: String,
    },

    /// AppRole authentication (recommended for applications)
    AppRole {
        /// Role ID
        role_id: String,
        /// Secret ID
        secret_id: String,
        /// Mount path for AppRole (default: "approle")
        #[serde(default = "default_approle_mount")]
        mount_path: String,
    },

    /// Kubernetes authentication (for K8s workloads)
    Kubernetes {
        /// Kubernetes auth role
        role: String,
        /// Path to service account JWT token
        #[serde(default = "default_k8s_token_path")]
        jwt_path: String,
        /// Mount path for Kubernetes auth (default: "kubernetes")
        #[serde(default = "default_kubernetes_mount")]
        mount_path: String,
    },

    /// LDAP authentication
    Ldap {
        /// LDAP username
        username: String,
        /// LDAP password
        password: String,
        /// Mount path for LDAP auth (default: "ldap")
        #[serde(default = "default_ldap_mount")]
        mount_path: String,
    },

    /// AWS IAM authentication
    AwsIam {
        /// AWS IAM role in Vault
        role: String,
        /// AWS region
        #[serde(default)]
        region: Option<String>,
        /// Mount path for AWS auth (default: "aws")
        #[serde(default = "default_aws_mount")]
        mount_path: String,
    },

    /// Userpass authentication
    Userpass {
        /// Username
        username: String,
        /// Password
        password: String,
        /// Mount path (default: "userpass")
        #[serde(default = "default_userpass_mount")]
        mount_path: String,
    },

    /// TLS certificate authentication
    Cert {
        /// Client certificate path
        cert_path: String,
        /// Client key path
        key_path: String,
        /// Mount path (default: "cert")
        #[serde(default = "default_cert_mount")]
        mount_path: String,
    },
}

fn default_approle_mount() -> String {
    "approle".to_string()
}

fn default_kubernetes_mount() -> String {
    "kubernetes".to_string()
}

fn default_ldap_mount() -> String {
    "ldap".to_string()
}

fn default_aws_mount() -> String {
    "aws".to_string()
}

fn default_userpass_mount() -> String {
    "userpass".to_string()
}

fn default_cert_mount() -> String {
    "cert".to_string()
}

fn default_k8s_token_path() -> String {
    "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string()
}

impl Default for VaultAuthMethod {
    fn default() -> Self {
        VaultAuthMethod::Token {
            token: String::new(),
        }
    }
}

impl From<super::config::VaultAuthMethod> for VaultAuthMethod {
    fn from(auth: super::config::VaultAuthMethod) -> Self {
        match auth {
            super::config::VaultAuthMethod::Token { token } => VaultAuthMethod::Token { token },
            super::config::VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path,
            } => VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path,
            },
            super::config::VaultAuthMethod::Kubernetes {
                role,
                jwt_path,
                mount_path,
            } => VaultAuthMethod::Kubernetes {
                role,
                jwt_path: jwt_path.to_string_lossy().to_string(),
                mount_path,
            },
            super::config::VaultAuthMethod::AwsIam { role, mount_path } => {
                VaultAuthMethod::AwsIam {
                    role,
                    region: None,
                    mount_path,
                }
            }
            super::config::VaultAuthMethod::Ldap {
                username,
                password,
                mount_path,
            } => VaultAuthMethod::Ldap {
                username,
                password,
                mount_path,
            },
            super::config::VaultAuthMethod::Userpass {
                username,
                password,
                mount_path,
            } => VaultAuthMethod::Userpass {
                username,
                password,
                mount_path,
            },
        }
    }
}

/// Configuration for the HashiCorp Vault backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Vault server address
    #[serde(default = "default_vault_addr")]
    pub address: String,

    /// Authentication method
    pub auth: VaultAuthMethod,

    /// Vault namespace (Enterprise feature)
    #[serde(default)]
    pub namespace: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Whether to verify TLS certificates
    #[serde(default = "default_tls_verify")]
    pub tls_verify: bool,

    /// Path to CA certificate for TLS
    #[serde(default)]
    pub ca_cert_path: Option<String>,

    /// Client certificate path for mutual TLS
    #[serde(default)]
    pub client_cert_path: Option<String>,

    /// Client key path for mutual TLS
    #[serde(default)]
    pub client_key_path: Option<String>,

    /// Maximum number of retries
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Retry delay in milliseconds
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// Default KV engine version (1 or 2)
    #[serde(default = "default_kv_version")]
    pub default_kv_version: u8,
}

impl From<super::config::VaultConfig> for VaultConfig {
    fn from(config: super::config::VaultConfig) -> Self {
        Self {
            address: config.address,
            auth: config.auth.into(),
            namespace: config.namespace,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            tls_verify: !config.skip_verify,
            ca_cert_path: config
                .ca_cert
                .map(|path| path.to_string_lossy().to_string()),
            client_cert_path: None,
            client_key_path: None,
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            default_kv_version: config.kv_version,
        }
    }
}

fn default_vault_addr() -> String {
    std::env::var("VAULT_ADDR").unwrap_or_else(|_| DEFAULT_VAULT_ADDR.to_string())
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_tls_verify() -> bool {
    true
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_delay_ms() -> u64 {
    1000
}

fn default_kv_version() -> u8 {
    2
}

impl VaultConfig {
    /// Create a new Vault configuration with the given address.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            auth: VaultAuthMethod::default(),
            namespace: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            tls_verify: true,
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            max_retries: 3,
            retry_delay_ms: 1000,
            default_kv_version: 2,
        }
    }

    /// Create configuration from environment variables.
    pub fn from_env() -> SecretResult<Self> {
        let address =
            std::env::var("VAULT_ADDR").unwrap_or_else(|_| DEFAULT_VAULT_ADDR.to_string());
        let namespace = std::env::var("VAULT_NAMESPACE").ok();

        // Determine auth method from environment
        let auth = if let Ok(token) = std::env::var("VAULT_TOKEN") {
            VaultAuthMethod::Token { token }
        } else if let (Ok(role_id), Ok(secret_id)) = (
            std::env::var("VAULT_ROLE_ID"),
            std::env::var("VAULT_SECRET_ID"),
        ) {
            VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path: std::env::var("VAULT_APPROLE_MOUNT")
                    .unwrap_or_else(|_| "approle".to_string()),
            }
        } else {
            return Err(SecretError::Configuration(
                "No Vault authentication configured. Set VAULT_TOKEN or VAULT_ROLE_ID/VAULT_SECRET_ID".into()
            ));
        };

        let tls_verify = std::env::var("VAULT_SKIP_VERIFY")
            .map(|v| v != "true" && v != "1")
            .unwrap_or(true);

        Ok(Self {
            address,
            auth,
            namespace,
            timeout_secs: std::env::var("VAULT_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
            tls_verify,
            ca_cert_path: std::env::var("VAULT_CACERT").ok(),
            client_cert_path: std::env::var("VAULT_CLIENT_CERT").ok(),
            client_key_path: std::env::var("VAULT_CLIENT_KEY").ok(),
            max_retries: 3,
            retry_delay_ms: 1000,
            default_kv_version: 2,
        })
    }

    /// Set the authentication method.
    pub fn with_auth(mut self, auth: VaultAuthMethod) -> Self {
        self.auth = auth;
        self
    }

    /// Set token authentication.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.auth = VaultAuthMethod::Token {
            token: token.into(),
        };
        self
    }

    /// Set the namespace.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set TLS verification.
    pub fn with_tls_verify(mut self, verify: bool) -> Self {
        self.tls_verify = verify;
        self
    }

    /// Set the CA certificate path.
    pub fn with_ca_cert(mut self, path: impl Into<String>) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }

    /// Set the default KV version.
    pub fn with_kv_version(mut self, version: u8) -> Self {
        self.default_kv_version = version;
        self
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self::new(DEFAULT_VAULT_ADDR)
    }
}

// ============================================================================
// Token Management
// ============================================================================

/// Represents an active Vault token with its metadata.
#[derive(Debug, Clone)]
struct VaultToken {
    /// The actual token value
    token: String,
    /// When the token was obtained
    obtained_at: Instant,
    /// Token TTL in seconds
    ttl_secs: Option<u64>,
    /// Whether the token is renewable
    renewable: bool,
    /// Token policies
    policies: Vec<String>,
}

impl VaultToken {
    fn new(token: String) -> Self {
        Self {
            token,
            obtained_at: Instant::now(),
            ttl_secs: None,
            renewable: false,
            policies: Vec::new(),
        }
    }

    fn with_metadata(
        mut self,
        ttl_secs: Option<u64>,
        renewable: bool,
        policies: Vec<String>,
    ) -> Self {
        self.ttl_secs = ttl_secs;
        self.renewable = renewable;
        self.policies = policies;
        self
    }

    fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl_secs {
            let elapsed = self.obtained_at.elapsed().as_secs();
            elapsed >= ttl
        } else {
            false
        }
    }

    fn should_refresh(&self) -> bool {
        if let Some(ttl) = self.ttl_secs {
            let elapsed = self.obtained_at.elapsed().as_secs();
            let threshold = (ttl as f64 * (1.0 - TOKEN_REFRESH_THRESHOLD)) as u64;
            self.renewable && elapsed >= threshold
        } else {
            false
        }
    }
}

// ============================================================================
// Vault API Response Types
// ============================================================================

/// Generic Vault API response wrapper.
#[derive(Debug, Deserialize)]
struct VaultResponse<T> {
    request_id: Option<String>,
    lease_id: Option<String>,
    renewable: Option<bool>,
    lease_duration: Option<u64>,
    data: Option<T>,
    wrap_info: Option<serde_json::Value>,
    warnings: Option<Vec<String>>,
    auth: Option<VaultAuthResponse>,
    errors: Option<Vec<String>>,
}

/// Vault auth response.
#[derive(Debug, Deserialize)]
struct VaultAuthResponse {
    client_token: String,
    accessor: Option<String>,
    policies: Vec<String>,
    token_policies: Option<Vec<String>>,
    metadata: Option<HashMap<String, String>>,
    lease_duration: u64,
    renewable: bool,
}

/// KV v2 secret data wrapper.
#[derive(Debug, Deserialize)]
struct KvV2Data {
    data: HashMap<String, serde_json::Value>,
    metadata: KvV2Metadata,
}

/// KV v2 metadata.
#[derive(Debug, Deserialize)]
struct KvV2Metadata {
    created_time: String,
    deletion_time: Option<String>,
    destroyed: bool,
    version: u64,
    custom_metadata: Option<HashMap<String, String>>,
}

/// KV v1 secret data (just key-value pairs).
#[derive(Debug, Deserialize)]
struct KvV1Data {
    #[serde(flatten)]
    data: HashMap<String, serde_json::Value>,
}

/// List response.
#[derive(Debug, Deserialize)]
struct ListData {
    keys: Vec<String>,
}

/// Transit encrypt response.
#[derive(Debug, Deserialize)]
struct TransitEncryptData {
    ciphertext: String,
    key_version: Option<u64>,
}

/// Transit decrypt response.
#[derive(Debug, Deserialize)]
struct TransitDecryptData {
    plaintext: String,
}

/// Database credentials response.
#[derive(Debug, Deserialize)]
pub struct DatabaseCredentials {
    pub username: String,
    pub password: String,
}

/// Dynamic credentials with lease information.
#[derive(Debug, Clone)]
pub struct DynamicCredentials {
    /// The credentials data
    pub data: HashMap<String, String>,
    /// Lease ID for renewal/revocation
    pub lease_id: String,
    /// Lease duration in seconds
    pub lease_duration: u64,
    /// Whether the lease is renewable
    pub renewable: bool,
}

// ============================================================================
// Vault Backend Implementation
// ============================================================================

/// HashiCorp Vault secret backend implementation.
///
/// Provides comprehensive access to Vault's secret engines including:
/// - KV secrets engine (v1 and v2)
/// - Transit encryption engine
/// - Dynamic database credentials
/// - AWS IAM credentials
///
/// ## Thread Safety
///
/// The `VaultBackend` is thread-safe and can be shared across tasks using `Arc`.
/// Token refresh is handled automatically in a thread-safe manner.
pub struct VaultBackend {
    /// HTTP client for Vault API calls
    client: Client,
    /// Vault configuration
    config: VaultConfig,
    /// Current authentication token
    token: Arc<RwLock<VaultToken>>,
    /// Cache of transit key metadata
    transit_keys: Arc<RwLock<HashMap<String, TransitKeyInfo>>>,
}

/// Transit key information cache.
#[derive(Debug, Clone)]
struct TransitKeyInfo {
    key_type: String,
    latest_version: u64,
    min_decryption_version: u64,
    supports_encryption: bool,
    supports_decryption: bool,
}

impl VaultBackend {
    /// Create a new Vault backend with the given configuration.
    ///
    /// This will authenticate with Vault using the configured auth method
    /// and obtain a client token.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Authentication fails
    /// - Vault is unreachable
    /// - Configuration is invalid
    pub async fn new(config: VaultConfig) -> SecretResult<Self> {
        let client = Self::build_client(&config)?;

        let backend = Self {
            client,
            config: config.clone(),
            token: Arc::new(RwLock::new(VaultToken::new(String::new()))),
            transit_keys: Arc::new(RwLock::new(HashMap::new())),
        };

        // Authenticate and obtain token
        backend.authenticate().await?;

        info!(
            address = %config.address,
            "Successfully connected to HashiCorp Vault"
        );

        Ok(backend)
    }

    /// Build the HTTP client with TLS configuration.
    fn build_client(config: &VaultConfig) -> SecretResult<Client> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .danger_accept_invalid_certs(!config.tls_verify);

        // Add CA certificate if specified
        if let Some(ca_path) = &config.ca_cert_path {
            let ca_cert = std::fs::read(ca_path).map_err(|e| {
                SecretError::Configuration(format!("Failed to read CA cert: {}", e))
            })?;
            let cert = reqwest::Certificate::from_pem(&ca_cert)
                .map_err(|e| SecretError::Configuration(format!("Invalid CA cert: {}", e)))?;
            builder = builder.add_root_certificate(cert);
        }

        // Add client certificate for mutual TLS
        if let (Some(cert_path), Some(key_path)) =
            (&config.client_cert_path, &config.client_key_path)
        {
            let cert = std::fs::read(cert_path).map_err(|e| {
                SecretError::Configuration(format!("Failed to read client cert: {}", e))
            })?;
            let key = std::fs::read(key_path).map_err(|e| {
                SecretError::Configuration(format!("Failed to read client key: {}", e))
            })?;

            let mut pem = cert;
            pem.extend_from_slice(&key);

            let identity = reqwest::Identity::from_pem(&pem).map_err(|e| {
                SecretError::Configuration(format!("Invalid client cert/key: {}", e))
            })?;
            builder = builder.identity(identity);
        }

        builder
            .build()
            .map_err(|e| SecretError::Configuration(format!("Failed to build HTTP client: {}", e)))
    }

    /// Authenticate with Vault using the configured method.
    async fn authenticate(&self) -> SecretResult<()> {
        let token = match &self.config.auth {
            VaultAuthMethod::Token { token } => {
                // For direct token auth, just validate and use the token
                self.validate_token(token).await?;
                VaultToken::new(token.clone())
            }
            VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path,
            } => self.auth_approle(role_id, secret_id, mount_path).await?,
            VaultAuthMethod::Kubernetes {
                role,
                jwt_path,
                mount_path,
            } => self.auth_kubernetes(role, jwt_path, mount_path).await?,
            VaultAuthMethod::Ldap {
                username,
                password,
                mount_path,
            } => self.auth_ldap(username, password, mount_path).await?,
            VaultAuthMethod::AwsIam {
                role,
                region,
                mount_path,
            } => {
                self.auth_aws_iam(role, region.as_deref(), mount_path)
                    .await?
            }
            VaultAuthMethod::Userpass {
                username,
                password,
                mount_path,
            } => self.auth_userpass(username, password, mount_path).await?,
            VaultAuthMethod::Cert {
                cert_path: _,
                key_path: _,
                mount_path,
            } => self.auth_cert(mount_path).await?,
        };

        let mut token_guard = self.token.write().await;
        *token_guard = token;
        Ok(())
    }

    /// Validate a token and get its metadata.
    async fn validate_token(&self, token: &str) -> SecretResult<VaultToken> {
        let url = format!("{}/v1/auth/token/lookup-self", self.config.address);

        let response = self
            .client
            .get(&url)
            .header("X-Vault-Token", token)
            .send()
            .await
            .map_err(|e| SecretError::Connection(format!("Failed to validate token: {}", e)))?;

        if !response.status().is_success() {
            return Err(SecretError::Authentication(
                "Invalid Vault token".to_string(),
            ));
        }

        let vault_resp: VaultResponse<serde_json::Value> = response.json().await?;

        if let Some(data) = vault_resp.data {
            let ttl = data.get("ttl").and_then(|v| v.as_u64());
            let renewable = data
                .get("renewable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let policies = data
                .get("policies")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            Ok(VaultToken::new(token.to_string()).with_metadata(ttl, renewable, policies))
        } else {
            Ok(VaultToken::new(token.to_string()))
        }
    }

    /// Authenticate using AppRole.
    async fn auth_approle(
        &self,
        role_id: &str,
        secret_id: &str,
        mount_path: &str,
    ) -> SecretResult<VaultToken> {
        let url = format!("{}/v1/auth/{}/login", self.config.address, mount_path);

        let body = serde_json::json!({
            "role_id": role_id,
            "secret_id": secret_id
        });

        let response = self.post_auth(&url, &body).await?;
        self.parse_auth_response(response).await
    }

    /// Authenticate using Kubernetes.
    async fn auth_kubernetes(
        &self,
        role: &str,
        jwt_path: &str,
        mount_path: &str,
    ) -> SecretResult<VaultToken> {
        let jwt = std::fs::read_to_string(jwt_path)
            .map_err(|e| SecretError::Configuration(format!("Failed to read K8s JWT: {}", e)))?;

        let url = format!("{}/v1/auth/{}/login", self.config.address, mount_path);

        let body = serde_json::json!({
            "role": role,
            "jwt": jwt.trim()
        });

        let response = self.post_auth(&url, &body).await?;
        self.parse_auth_response(response).await
    }

    /// Authenticate using LDAP.
    async fn auth_ldap(
        &self,
        username: &str,
        password: &str,
        mount_path: &str,
    ) -> SecretResult<VaultToken> {
        let url = format!(
            "{}/v1/auth/{}/login/{}",
            self.config.address, mount_path, username
        );

        let body = serde_json::json!({
            "password": password
        });

        let response = self.post_auth(&url, &body).await?;
        self.parse_auth_response(response).await
    }

    /// Authenticate using AWS IAM.
    async fn auth_aws_iam(
        &self,
        role: &str,
        region: Option<&str>,
        mount_path: &str,
    ) -> SecretResult<VaultToken> {
        // AWS IAM auth requires signing a request to STS GetCallerIdentity
        // This is a simplified implementation - full implementation would use AWS SDK
        let url = format!("{}/v1/auth/{}/login", self.config.address, mount_path);

        // For production, this should use proper AWS STS signing
        let body = serde_json::json!({
            "role": role,
            "iam_http_request_method": "POST",
            "iam_request_url": "aHR0cHM6Ly9zdHMuYW1hem9uYXdzLmNvbS8=", // base64("https://sts.amazonaws.com/")
            "iam_request_body": "QWN0aW9uPUdldENhbGxlcklkZW50aXR5JlZlcnNpb249MjAxMS0wNi0xNQ==", // base64
            "iam_request_headers": {} // Would need proper AWS Signature V4 headers
        });

        warn!("AWS IAM auth requires proper STS signing. This is a placeholder implementation.");

        let response = self.post_auth(&url, &body).await?;
        self.parse_auth_response(response).await
    }

    /// Authenticate using userpass.
    async fn auth_userpass(
        &self,
        username: &str,
        password: &str,
        mount_path: &str,
    ) -> SecretResult<VaultToken> {
        let url = format!(
            "{}/v1/auth/{}/login/{}",
            self.config.address, mount_path, username
        );

        let body = serde_json::json!({
            "password": password
        });

        let response = self.post_auth(&url, &body).await?;
        self.parse_auth_response(response).await
    }

    /// Authenticate using TLS certificate.
    async fn auth_cert(&self, mount_path: &str) -> SecretResult<VaultToken> {
        let url = format!("{}/v1/auth/{}/login", self.config.address, mount_path);

        // Certificate is sent as part of TLS handshake
        let response = self.post_auth(&url, &serde_json::json!({})).await?;
        self.parse_auth_response(response).await
    }

    /// Send an authentication POST request.
    async fn post_auth(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> SecretResult<reqwest::Response> {
        let mut request = self.client.post(url).json(body);

        if let Some(ns) = &self.config.namespace {
            request = request.header("X-Vault-Namespace", ns);
        }

        request
            .send()
            .await
            .map_err(|e| SecretError::Connection(format!("Authentication request failed: {}", e)))
    }

    /// Parse an authentication response.
    async fn parse_auth_response(&self, response: reqwest::Response) -> SecretResult<VaultToken> {
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Authentication(format!(
                "Authentication failed ({}): {}",
                status, body
            )));
        }

        let vault_resp: VaultResponse<serde_json::Value> = response.json().await?;

        if let Some(auth) = vault_resp.auth {
            Ok(VaultToken::new(auth.client_token).with_metadata(
                Some(auth.lease_duration),
                auth.renewable,
                auth.policies,
            ))
        } else {
            Err(SecretError::Authentication(
                "No auth data in response".to_string(),
            ))
        }
    }

    /// Get the current token, refreshing if necessary.
    async fn get_token(&self) -> SecretResult<String> {
        {
            let token = self.token.read().await;
            if token.is_expired() {
                drop(token);
                self.authenticate().await?;
            } else if token.should_refresh() {
                drop(token);
                if let Err(e) = self.refresh_token().await {
                    warn!("Token refresh failed, will re-authenticate: {}", e);
                    self.authenticate().await?;
                }
            }
        }

        let token = self.token.read().await;
        Ok(token.token.clone())
    }

    /// Refresh the current token.
    async fn refresh_token(&self) -> SecretResult<()> {
        let current_token = self.token.read().await.token.clone();

        let url = format!("{}/v1/auth/token/renew-self", self.config.address);

        let response = self
            .client
            .post(&url)
            .header("X-Vault-Token", &current_token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SecretError::Authentication(
                "Token refresh failed".to_string(),
            ));
        }

        let vault_resp: VaultResponse<serde_json::Value> = response.json().await?;

        if let Some(auth) = vault_resp.auth {
            let mut token_guard = self.token.write().await;
            *token_guard = VaultToken::new(auth.client_token).with_metadata(
                Some(auth.lease_duration),
                auth.renewable,
                auth.policies,
            );
            debug!("Vault token refreshed successfully");
        }

        Ok(())
    }

    /// Make an authenticated request to Vault.
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> SecretResult<reqwest::RequestBuilder> {
        let token = self.get_token().await?;
        let url = format!(
            "{}/v1/{}",
            self.config.address,
            path.trim_start_matches('/')
        );

        let mut request = self
            .client
            .request(method, &url)
            .header("X-Vault-Token", token);

        if let Some(ns) = &self.config.namespace {
            request = request.header("X-Vault-Namespace", ns);
        }

        Ok(request)
    }

    /// Execute a request with retry logic.
    async fn execute_with_retry(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> SecretResult<reqwest::Response> {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    self.config.retry_delay_ms * 2u64.pow(attempt - 1),
                ))
                .await;
            }

            match request_builder
                .try_clone()
                .ok_or_else(|| SecretError::Other {
                    message: "Failed to clone request".into(),
                    source: None,
                })?
                .send()
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    debug!(attempt = attempt, error = %e, "Request failed, retrying");
                    last_error = Some(e);
                }
            }
        }

        Err(SecretError::Connection(format!(
            "Request failed after {} retries: {}",
            self.config.max_retries,
            last_error.map(|e| e.to_string()).unwrap_or_default()
        )))
    }

    // ========================================================================
    // KV Secrets Engine
    // ========================================================================

    /// Read a secret from KV v2 engine.
    pub async fn kv_v2_read(&self, mount: &str, path: &str) -> SecretResult<Secret> {
        let api_path = format!("{}/data/{}", mount, path.trim_start_matches('/'));
        let request = self.request(reqwest::Method::GET, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        self.handle_response::<KvV2Data>(response, &api_path)
            .await
            .map(|data| {
                let mut secret_data = HashMap::new();
                for (k, v) in data.data {
                    secret_data.insert(k, json_to_secret_value(v));
                }

                let metadata = SecretMetadata {
                    version: Some(SecretVersion::Numeric(data.metadata.version)),
                    created_time: parse_rfc3339_timestamp(&data.metadata.created_time),
                    updated_time: None,
                    custom: data.metadata.custom_metadata.unwrap_or_default(),
                    deletion_time: data
                        .metadata
                        .deletion_time
                        .as_ref()
                        .and_then(|t| parse_rfc3339_timestamp(t)),
                    destroyed: data.metadata.destroyed,
                };

                Secret::with_metadata(path, secret_data, metadata)
            })
    }

    /// Read a specific version of a secret from KV v2.
    pub async fn kv_v2_read_version(
        &self,
        mount: &str,
        path: &str,
        version: u64,
    ) -> SecretResult<Secret> {
        let api_path = format!(
            "{}/data/{}?version={}",
            mount,
            path.trim_start_matches('/'),
            version
        );
        let request = self.request(reqwest::Method::GET, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        self.handle_response::<KvV2Data>(response, &api_path)
            .await
            .map(|data| {
                let mut secret_data = HashMap::new();
                for (k, v) in data.data {
                    secret_data.insert(k, json_to_secret_value(v));
                }

                let metadata = SecretMetadata {
                    version: Some(SecretVersion::Numeric(data.metadata.version)),
                    created_time: parse_rfc3339_timestamp(&data.metadata.created_time),
                    updated_time: None,
                    custom: data.metadata.custom_metadata.unwrap_or_default(),
                    deletion_time: None,
                    destroyed: data.metadata.destroyed,
                };

                Secret::with_metadata(path, secret_data, metadata)
            })
    }

    /// Write a secret to KV v2 engine.
    pub async fn kv_v2_write(
        &self,
        mount: &str,
        path: &str,
        data: HashMap<String, SecretValue>,
    ) -> SecretResult<()> {
        let api_path = format!("{}/data/{}", mount, path.trim_start_matches('/'));

        let json_data: HashMap<String, serde_json::Value> = data
            .into_iter()
            .map(|(k, v)| (k, secret_value_to_json(v)))
            .collect();

        let body = serde_json::json!({
            "data": json_data
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to write secret: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    /// Delete a secret from KV v2 (soft delete).
    pub async fn kv_v2_delete(&self, mount: &str, path: &str) -> SecretResult<()> {
        let api_path = format!("{}/data/{}", mount, path.trim_start_matches('/'));
        let request = self.request(reqwest::Method::DELETE, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        if !response.status().is_success() && response.status() != StatusCode::NO_CONTENT {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to delete secret: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    /// Destroy specific versions of a secret (permanent deletion).
    pub async fn kv_v2_destroy(
        &self,
        mount: &str,
        path: &str,
        versions: &[u64],
    ) -> SecretResult<()> {
        let api_path = format!("{}/destroy/{}", mount, path.trim_start_matches('/'));

        let body = serde_json::json!({
            "versions": versions
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        if !response.status().is_success() && response.status() != StatusCode::NO_CONTENT {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to destroy secret versions: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    /// Undelete (recover) specific versions of a secret.
    pub async fn kv_v2_undelete(
        &self,
        mount: &str,
        path: &str,
        versions: &[u64],
    ) -> SecretResult<()> {
        let api_path = format!("{}/undelete/{}", mount, path.trim_start_matches('/'));

        let body = serde_json::json!({
            "versions": versions
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        if !response.status().is_success() && response.status() != StatusCode::NO_CONTENT {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to undelete secret: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    /// List secrets at a path in KV v2.
    pub async fn kv_v2_list(&self, mount: &str, path: &str) -> SecretResult<Vec<String>> {
        let api_path = format!("{}/metadata/{}", mount, path.trim_start_matches('/'));
        let request = self
            .request(reqwest::Method::from_bytes(b"LIST").unwrap(), &api_path)
            .await?;
        let response = self.execute_with_retry(request).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }

        self.handle_response::<ListData>(response, &api_path)
            .await
            .map(|data| data.keys)
    }

    /// Read a secret from KV v1 engine.
    pub async fn kv_v1_read(&self, mount: &str, path: &str) -> SecretResult<Secret> {
        let api_path = format!("{}/{}", mount, path.trim_start_matches('/'));
        let request = self.request(reqwest::Method::GET, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        self.handle_response::<HashMap<String, serde_json::Value>>(response, &api_path)
            .await
            .map(|data| {
                let mut secret_data = HashMap::new();
                for (k, v) in data {
                    secret_data.insert(k, json_to_secret_value(v));
                }
                Secret::new(path, secret_data)
            })
    }

    /// Write a secret to KV v1 engine.
    pub async fn kv_v1_write(
        &self,
        mount: &str,
        path: &str,
        data: HashMap<String, SecretValue>,
    ) -> SecretResult<()> {
        let api_path = format!("{}/{}", mount, path.trim_start_matches('/'));

        let json_data: HashMap<String, serde_json::Value> = data
            .into_iter()
            .map(|(k, v)| (k, secret_value_to_json(v)))
            .collect();

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&json_data);
        let response = self.execute_with_retry(request).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to write secret: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    // ========================================================================
    // Transit Secrets Engine (Encryption as a Service)
    // ========================================================================

    /// Encrypt data using the Transit secrets engine.
    ///
    /// # Arguments
    ///
    /// * `key_name` - Name of the encryption key in Transit
    /// * `plaintext` - Data to encrypt (will be base64 encoded)
    ///
    /// # Returns
    ///
    /// Returns the ciphertext in Vault's format: `vault:v1:...`
    pub async fn transit_encrypt(&self, key_name: &str, plaintext: &str) -> SecretResult<String> {
        self.transit_encrypt_with_mount("transit", key_name, plaintext)
            .await
    }

    /// Encrypt data using a specific Transit mount.
    pub async fn transit_encrypt_with_mount(
        &self,
        mount: &str,
        key_name: &str,
        plaintext: &str,
    ) -> SecretResult<String> {
        let api_path = format!("{}/encrypt/{}", mount, key_name);

        let body = serde_json::json!({
            "plaintext": BASE64.encode(plaintext.as_bytes())
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        self.handle_response::<TransitEncryptData>(response, &api_path)
            .await
            .map(|data| data.ciphertext)
    }

    /// Decrypt data using the Transit secrets engine.
    ///
    /// # Arguments
    ///
    /// * `key_name` - Name of the encryption key in Transit
    /// * `ciphertext` - Data to decrypt (Vault format: `vault:v1:...`)
    ///
    /// # Returns
    ///
    /// Returns the decrypted plaintext.
    pub async fn transit_decrypt(&self, key_name: &str, ciphertext: &str) -> SecretResult<String> {
        self.transit_decrypt_with_mount("transit", key_name, ciphertext)
            .await
    }

    /// Decrypt data using a specific Transit mount.
    pub async fn transit_decrypt_with_mount(
        &self,
        mount: &str,
        key_name: &str,
        ciphertext: &str,
    ) -> SecretResult<String> {
        let api_path = format!("{}/decrypt/{}", mount, key_name);

        let body = serde_json::json!({
            "ciphertext": ciphertext
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        let data = self
            .handle_response::<TransitDecryptData>(response, &api_path)
            .await?;

        let plaintext_bytes = BASE64.decode(&data.plaintext).map_err(|e| {
            SecretError::InvalidFormat(format!("Failed to decode base64 plaintext: {}", e))
        })?;

        String::from_utf8(plaintext_bytes).map_err(|e| {
            SecretError::InvalidFormat(format!("Decrypted data is not valid UTF-8: {}", e))
        })
    }

    /// Encrypt multiple items in a batch using Transit.
    pub async fn transit_encrypt_batch(
        &self,
        key_name: &str,
        plaintexts: &[&str],
    ) -> SecretResult<Vec<String>> {
        let api_path = format!("transit/encrypt/{}", key_name);

        let batch_input: Vec<serde_json::Value> = plaintexts
            .iter()
            .map(|p| serde_json::json!({"plaintext": BASE64.encode(p.as_bytes())}))
            .collect();

        let body = serde_json::json!({
            "batch_input": batch_input
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct BatchResult {
            ciphertext: String,
        }

        #[derive(Deserialize)]
        struct BatchData {
            batch_results: Vec<BatchResult>,
        }

        self.handle_response::<BatchData>(response, &api_path)
            .await
            .map(|data| {
                data.batch_results
                    .into_iter()
                    .map(|r| r.ciphertext)
                    .collect()
            })
    }

    /// Decrypt multiple items in a batch using Transit.
    pub async fn transit_decrypt_batch(
        &self,
        key_name: &str,
        ciphertexts: &[&str],
    ) -> SecretResult<Vec<String>> {
        let api_path = format!("transit/decrypt/{}", key_name);

        let batch_input: Vec<serde_json::Value> = ciphertexts
            .iter()
            .map(|c| serde_json::json!({"ciphertext": c}))
            .collect();

        let body = serde_json::json!({
            "batch_input": batch_input
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct BatchResult {
            plaintext: String,
        }

        #[derive(Deserialize)]
        struct BatchData {
            batch_results: Vec<BatchResult>,
        }

        let data = self
            .handle_response::<BatchData>(response, &api_path)
            .await?;

        data.batch_results
            .into_iter()
            .map(|r| {
                let bytes = BASE64.decode(&r.plaintext).map_err(|e| {
                    SecretError::InvalidFormat(format!("Failed to decode base64: {}", e))
                })?;
                String::from_utf8(bytes)
                    .map_err(|e| SecretError::InvalidFormat(format!("Invalid UTF-8: {}", e)))
            })
            .collect()
    }

    /// Rewrap ciphertext with the latest version of the key.
    pub async fn transit_rewrap(&self, key_name: &str, ciphertext: &str) -> SecretResult<String> {
        let api_path = format!("transit/rewrap/{}", key_name);

        let body = serde_json::json!({
            "ciphertext": ciphertext
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        self.handle_response::<TransitEncryptData>(response, &api_path)
            .await
            .map(|data| data.ciphertext)
    }

    /// Generate a data key for client-side encryption.
    pub async fn transit_generate_data_key(
        &self,
        key_name: &str,
        key_type: &str, // "plaintext" or "wrapped"
    ) -> SecretResult<(String, Option<String>)> {
        let api_path = format!("transit/datakey/{}/{}", key_type, key_name);

        let request = self.request(reqwest::Method::POST, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct DataKeyData {
            ciphertext: String,
            plaintext: Option<String>,
        }

        self.handle_response::<DataKeyData>(response, &api_path)
            .await
            .map(|data| (data.ciphertext, data.plaintext))
    }

    /// Generate random bytes using Transit.
    pub async fn transit_random(&self, bytes: u32, format: &str) -> SecretResult<String> {
        let api_path = format!("transit/random/{}", bytes);

        let body = serde_json::json!({
            "format": format // "base64" or "hex"
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct RandomData {
            random_bytes: String,
        }

        self.handle_response::<RandomData>(response, &api_path)
            .await
            .map(|data| data.random_bytes)
    }

    /// Compute HMAC of data.
    pub async fn transit_hmac(
        &self,
        key_name: &str,
        algorithm: &str, // "sha2-256", "sha2-384", "sha2-512"
        input: &str,
    ) -> SecretResult<String> {
        let api_path = format!("transit/hmac/{}/{}", key_name, algorithm);

        let body = serde_json::json!({
            "input": BASE64.encode(input.as_bytes())
        });

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct HmacData {
            hmac: String,
        }

        self.handle_response::<HmacData>(response, &api_path)
            .await
            .map(|data| data.hmac)
    }

    /// Sign data using Transit.
    pub async fn transit_sign(
        &self,
        key_name: &str,
        input: &str,
        hash_algorithm: Option<&str>,
    ) -> SecretResult<String> {
        let api_path = format!("transit/sign/{}", key_name);

        let mut body = serde_json::json!({
            "input": BASE64.encode(input.as_bytes())
        });

        if let Some(alg) = hash_algorithm {
            body["hash_algorithm"] = serde_json::Value::String(alg.to_string());
        }

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct SignData {
            signature: String,
        }

        self.handle_response::<SignData>(response, &api_path)
            .await
            .map(|data| data.signature)
    }

    /// Verify a signature using Transit.
    pub async fn transit_verify(
        &self,
        key_name: &str,
        input: &str,
        signature: &str,
        hash_algorithm: Option<&str>,
    ) -> SecretResult<bool> {
        let api_path = format!("transit/verify/{}", key_name);

        let mut body = serde_json::json!({
            "input": BASE64.encode(input.as_bytes()),
            "signature": signature
        });

        if let Some(alg) = hash_algorithm {
            body["hash_algorithm"] = serde_json::Value::String(alg.to_string());
        }

        let request = self
            .request(reqwest::Method::POST, &api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        #[derive(Deserialize)]
        struct VerifyData {
            valid: bool,
        }

        self.handle_response::<VerifyData>(response, &api_path)
            .await
            .map(|data| data.valid)
    }

    // ========================================================================
    // Dynamic Credentials
    // ========================================================================

    /// Generate database credentials.
    ///
    /// # Arguments
    ///
    /// * `role` - The database role name configured in Vault
    ///
    /// # Returns
    ///
    /// Returns dynamic credentials with lease information.
    pub async fn generate_database_credentials(
        &self,
        role: &str,
    ) -> SecretResult<DynamicCredentials> {
        self.generate_database_credentials_with_mount("database", role)
            .await
    }

    /// Generate database credentials from a specific mount.
    pub async fn generate_database_credentials_with_mount(
        &self,
        mount: &str,
        role: &str,
    ) -> SecretResult<DynamicCredentials> {
        let api_path = format!("{}/creds/{}", mount, role);

        let request = self.request(reqwest::Method::GET, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to generate database credentials: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        let vault_resp: VaultResponse<DatabaseCredentials> = response.json().await?;

        let data = vault_resp
            .data
            .ok_or_else(|| SecretError::InvalidFormat("No data in response".into()))?;

        let mut creds_map = HashMap::new();
        creds_map.insert("username".to_string(), data.username);
        creds_map.insert("password".to_string(), data.password);

        Ok(DynamicCredentials {
            data: creds_map,
            lease_id: vault_resp.lease_id.unwrap_or_default(),
            lease_duration: vault_resp.lease_duration.unwrap_or(0),
            renewable: vault_resp.renewable.unwrap_or(false),
        })
    }

    /// Generate AWS credentials.
    pub async fn generate_aws_credentials(&self, role: &str) -> SecretResult<DynamicCredentials> {
        self.generate_aws_credentials_with_mount("aws", role).await
    }

    /// Generate AWS credentials from a specific mount.
    pub async fn generate_aws_credentials_with_mount(
        &self,
        mount: &str,
        role: &str,
    ) -> SecretResult<DynamicCredentials> {
        let api_path = format!("{}/creds/{}", mount, role);

        let request = self.request(reqwest::Method::GET, &api_path).await?;
        let response = self.execute_with_retry(request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to generate AWS credentials: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        #[derive(Deserialize)]
        struct AwsCreds {
            access_key: String,
            secret_key: String,
            security_token: Option<String>,
        }

        let vault_resp: VaultResponse<AwsCreds> = response.json().await?;

        let data = vault_resp
            .data
            .ok_or_else(|| SecretError::InvalidFormat("No data in response".into()))?;

        let mut creds_map = HashMap::new();
        creds_map.insert("access_key".to_string(), data.access_key);
        creds_map.insert("secret_key".to_string(), data.secret_key);
        if let Some(token) = data.security_token {
            creds_map.insert("security_token".to_string(), token);
        }

        Ok(DynamicCredentials {
            data: creds_map,
            lease_id: vault_resp.lease_id.unwrap_or_default(),
            lease_duration: vault_resp.lease_duration.unwrap_or(0),
            renewable: vault_resp.renewable.unwrap_or(false),
        })
    }

    /// Renew a lease.
    pub async fn renew_lease(&self, lease_id: &str, increment: Option<u64>) -> SecretResult<u64> {
        let api_path = "sys/leases/renew";

        let mut body = serde_json::json!({
            "lease_id": lease_id
        });

        if let Some(inc) = increment {
            body["increment"] = serde_json::Value::Number(inc.into());
        }

        let request = self
            .request(reqwest::Method::PUT, api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to renew lease: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        let vault_resp: VaultResponse<serde_json::Value> = response.json().await?;
        Ok(vault_resp.lease_duration.unwrap_or(0))
    }

    /// Revoke a lease.
    pub async fn revoke_lease(&self, lease_id: &str) -> SecretResult<()> {
        let api_path = "sys/leases/revoke";

        let body = serde_json::json!({
            "lease_id": lease_id
        });

        let request = self
            .request(reqwest::Method::PUT, api_path)
            .await?
            .json(&body);
        let response = self.execute_with_retry(request).await?;

        let status = response.status();
        if !status.is_success() && status != StatusCode::NO_CONTENT {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: format!("Failed to revoke lease: {}", body),
                status_code: Some(status.as_u16()),
            });
        }

        Ok(())
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Handle a Vault API response.
    async fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
        path: &str,
    ) -> SecretResult<T> {
        let status = response.status();

        if status == StatusCode::NOT_FOUND {
            return Err(SecretError::NotFound(path.to_string()));
        }

        if status == StatusCode::FORBIDDEN {
            return Err(SecretError::Authorization(format!(
                "Access denied to path: {}",
                path
            )));
        }

        if status == StatusCode::UNAUTHORIZED {
            return Err(SecretError::Authentication(
                "Token expired or invalid".into(),
            ));
        }

        if status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::BAD_GATEWAY {
            return Err(SecretError::Sealed("Vault is sealed or unavailable".into()));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SecretError::Backend {
                message: body,
                status_code: Some(status.as_u16()),
            });
        }

        let vault_resp: VaultResponse<T> = response.json().await?;

        // Check for errors in response
        if let Some(errors) = vault_resp.errors {
            if !errors.is_empty() {
                return Err(SecretError::Backend {
                    message: errors.join(", "),
                    status_code: Some(status.as_u16()),
                });
            }
        }

        // Log warnings if present
        if let Some(warnings) = vault_resp.warnings {
            for warning in warnings {
                warn!(path = %path, warning = %warning, "Vault warning");
            }
        }

        vault_resp
            .data
            .ok_or_else(|| SecretError::InvalidFormat("No data in response".into()))
    }
}

// ============================================================================
// SecretBackend Trait Implementation
// ============================================================================

#[async_trait]
impl SecretBackend for VaultBackend {
    fn backend_type(&self) -> SecretBackendType {
        SecretBackendType::Vault
    }

    async fn get_secret(&self, path: &str) -> SecretResult<Secret> {
        // Determine if this is KV v1 or v2 based on path structure
        // KV v2 paths typically include /data/ segment
        if path.contains("/data/") {
            // KV v2 path
            let parts: Vec<&str> = path.splitn(3, "/data/").collect();
            if parts.len() == 2 {
                self.kv_v2_read(parts[0], parts[1]).await
            } else {
                self.kv_v2_read("secret", path).await
            }
        } else if self.config.default_kv_version == 2 {
            // Assume KV v2 with default mount
            self.kv_v2_read("secret", path).await
        } else {
            // KV v1
            self.kv_v1_read("secret", path).await
        }
    }

    async fn get_secret_version(&self, path: &str, version: &str) -> SecretResult<Secret> {
        let version_num: u64 = version.parse().map_err(|_| {
            SecretError::InvalidFormat(format!("Invalid version number: {}", version))
        })?;

        // Extract mount from path
        let parts: Vec<&str> = path.splitn(3, "/data/").collect();
        if parts.len() == 2 {
            self.kv_v2_read_version(parts[0], parts[1], version_num)
                .await
        } else {
            self.kv_v2_read_version("secret", path, version_num).await
        }
    }

    async fn list_secrets(&self, path: &str) -> SecretResult<Vec<String>> {
        // Determine mount and path
        if path.contains("/metadata/") {
            let parts: Vec<&str> = path.splitn(3, "/metadata/").collect();
            if parts.len() == 2 {
                self.kv_v2_list(parts[0], parts[1]).await
            } else {
                self.kv_v2_list("secret", path).await
            }
        } else {
            self.kv_v2_list("secret", path).await
        }
    }

    async fn put_secret(&self, path: &str, secret: &Secret) -> SecretResult<()> {
        // Determine if KV v1 or v2
        if path.contains("/data/") || self.config.default_kv_version == 2 {
            let parts: Vec<&str> = path.splitn(3, "/data/").collect();
            if parts.len() == 2 {
                self.kv_v2_write(parts[0], parts[1], secret.data().clone())
                    .await
            } else {
                self.kv_v2_write("secret", path, secret.data().clone())
                    .await
            }
        } else {
            self.kv_v1_write("secret", path, secret.data().clone())
                .await
        }
    }

    async fn delete_secret(&self, path: &str) -> SecretResult<()> {
        let parts: Vec<&str> = path.splitn(3, "/data/").collect();
        if parts.len() == 2 {
            self.kv_v2_delete(parts[0], parts[1]).await
        } else {
            self.kv_v2_delete("secret", path).await
        }
    }

    async fn health_check(&self) -> SecretResult<bool> {
        let url = format!("{}/v1/sys/health", self.config.address);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| SecretError::Connection(format!("Health check failed: {}", e)))?;

        // Vault returns different status codes for different states
        // 200: initialized, unsealed, active
        // 429: unsealed, standby
        // 472: disaster recovery mode replication secondary and target
        // 473: performance standby
        // 501: not initialized
        // 503: sealed
        Ok(response.status().is_success() || response.status() == StatusCode::TOO_MANY_REQUESTS)
    }
}

impl BackendCapabilities for VaultBackend {
    fn capabilities(&self) -> Vec<BackendCapability> {
        vec![
            BackendCapability::List,
            BackendCapability::Versioning,
            BackendCapability::Rotation,
            BackendCapability::SoftDelete,
            BackendCapability::Metadata,
            BackendCapability::BinaryData,
            BackendCapability::Generation,
        ]
    }
}

impl std::fmt::Debug for VaultBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultBackend")
            .field("address", &self.config.address)
            .field("namespace", &self.config.namespace)
            .field("kv_version", &self.config.default_kv_version)
            .finish()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert JSON value to SecretValue.
fn json_to_secret_value(value: serde_json::Value) -> SecretValue {
    match value {
        serde_json::Value::String(s) => SecretValue::String(s),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SecretValue::Integer(i)
            } else {
                SecretValue::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => SecretValue::Boolean(b),
        serde_json::Value::Null => SecretValue::Null,
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            SecretValue::String(value.to_string())
        }
    }
}

/// Convert SecretValue to JSON value.
fn secret_value_to_json(value: SecretValue) -> serde_json::Value {
    match value {
        SecretValue::String(s) => serde_json::Value::String(s),
        SecretValue::Integer(i) => serde_json::Value::Number(i.into()),
        SecretValue::Boolean(b) => serde_json::Value::Bool(b),
        SecretValue::Binary(b) => serde_json::Value::String(BASE64.encode(&b)),
        SecretValue::Null => serde_json::Value::Null,
    }
}

/// Parse RFC 3339 timestamp to Unix timestamp.
fn parse_rfc3339_timestamp(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_config_default() {
        let config = VaultConfig::default();
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert!(config.tls_verify);
        assert_eq!(config.default_kv_version, 2);
    }

    #[test]
    fn test_vault_config_builder() {
        let config = VaultConfig::new("https://vault.example.com:8200")
            .with_token("my-token")
            .with_namespace("my-ns")
            .with_kv_version(1);

        assert_eq!(config.address, "https://vault.example.com:8200");
        assert_eq!(config.namespace, Some("my-ns".to_string()));
        assert_eq!(config.default_kv_version, 1);

        if let VaultAuthMethod::Token { token } = config.auth {
            assert_eq!(token, "my-token");
        } else {
            panic!("Expected token auth");
        }
    }

    #[test]
    fn test_vault_auth_method_default() {
        let auth = VaultAuthMethod::default();
        if let VaultAuthMethod::Token { token } = auth {
            assert!(token.is_empty());
        } else {
            panic!("Expected token auth as default");
        }
    }

    #[test]
    fn test_json_to_secret_value() {
        assert_eq!(
            json_to_secret_value(serde_json::json!("hello")),
            SecretValue::String("hello".to_string())
        );
        assert_eq!(
            json_to_secret_value(serde_json::json!(42)),
            SecretValue::Integer(42)
        );
        assert_eq!(
            json_to_secret_value(serde_json::json!(true)),
            SecretValue::Boolean(true)
        );
        assert_eq!(
            json_to_secret_value(serde_json::json!(null)),
            SecretValue::Null
        );
    }

    #[test]
    fn test_secret_value_to_json() {
        assert_eq!(
            secret_value_to_json(SecretValue::String("hello".to_string())),
            serde_json::json!("hello")
        );
        assert_eq!(
            secret_value_to_json(SecretValue::Integer(42)),
            serde_json::json!(42)
        );
        assert_eq!(
            secret_value_to_json(SecretValue::Boolean(true)),
            serde_json::json!(true)
        );
        assert_eq!(
            secret_value_to_json(SecretValue::Null),
            serde_json::json!(null)
        );
    }

    #[test]
    fn test_token_expiration() {
        let token = VaultToken::new("test".to_string()).with_metadata(Some(60), true, vec![]);
        assert!(!token.is_expired());
        assert!(!token.should_refresh()); // 0 seconds elapsed, well below threshold

        let expired_token = VaultToken {
            token: "test".to_string(),
            obtained_at: Instant::now() - Duration::from_secs(61),
            ttl_secs: Some(60),
            renewable: true,
            policies: vec![],
        };
        assert!(expired_token.is_expired());
    }

    #[test]
    fn test_dynamic_credentials() {
        let creds = DynamicCredentials {
            data: {
                let mut m = HashMap::new();
                m.insert("username".to_string(), "user123".to_string());
                m.insert("password".to_string(), "pass456".to_string());
                m
            },
            lease_id: "database/creds/my-role/abcd1234".to_string(),
            lease_duration: 3600,
            renewable: true,
        };

        assert_eq!(creds.data.get("username"), Some(&"user123".to_string()));
        assert_eq!(creds.lease_duration, 3600);
        assert!(creds.renewable);
    }
}
