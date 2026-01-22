//! Configuration for secret management.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::backend::SecretBackendType;
use super::cache::SecretCacheConfig;
use super::rotation::RotationConfig;

/// Main configuration for secret management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretConfig {
    /// The backend type to use
    pub backend_type: SecretBackendType,

    /// HashiCorp Vault configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<VaultConfig>,

    /// AWS Secrets Manager configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws: Option<AwsSecretsManagerConfig>,

    /// Cache configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<SecretCacheConfig>,

    /// Rotation configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationConfig>,

    /// Request timeout
    #[serde(with = "humantime_serde", default = "default_timeout")]
    pub timeout: Duration,

    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Retry delay
    #[serde(with = "humantime_serde", default = "default_retry_delay")]
    pub retry_delay: Duration,
}

fn default_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_delay() -> Duration {
    Duration::from_millis(500)
}

impl Default for SecretConfig {
    fn default() -> Self {
        Self {
            backend_type: SecretBackendType::Vault,
            vault: None,
            aws: None,
            cache: Some(SecretCacheConfig::default()),
            rotation: None,
            timeout: default_timeout(),
            max_retries: default_max_retries(),
            retry_delay: default_retry_delay(),
        }
    }
}

impl SecretConfig {
    /// Create a builder for Vault configuration.
    pub fn vault() -> SecretConfigBuilder {
        SecretConfigBuilder::new(SecretBackendType::Vault)
    }

    /// Create a builder for AWS Secrets Manager configuration.
    pub fn aws_secrets_manager() -> SecretConfigBuilder {
        SecretConfigBuilder::new(SecretBackendType::AwsSecretsManager)
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Detect backend type
        if std::env::var("VAULT_ADDR").is_ok() {
            config.backend_type = SecretBackendType::Vault;
            config.vault = Some(VaultConfig::from_env());
        } else if std::env::var("AWS_REGION").is_ok() || std::env::var("AWS_DEFAULT_REGION").is_ok()
        {
            config.backend_type = SecretBackendType::AwsSecretsManager;
            config.aws = Some(AwsSecretsManagerConfig::from_env());
        }

        config
    }
}

/// Builder for SecretConfig.
#[derive(Debug, Clone)]
pub struct SecretConfigBuilder {
    config: SecretConfig,
}

impl SecretConfigBuilder {
    /// Create a new builder with the specified backend type.
    pub fn new(backend_type: SecretBackendType) -> Self {
        Self {
            config: SecretConfig {
                backend_type,
                ..Default::default()
            },
        }
    }

    /// Set the Vault address.
    pub fn address(mut self, address: impl Into<String>) -> Self {
        let vault = self.config.vault.get_or_insert_with(VaultConfig::default);
        vault.address = address.into();
        self
    }

    /// Set the Vault token.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        let vault = self.config.vault.get_or_insert_with(VaultConfig::default);
        vault.auth = VaultAuthMethod::Token {
            token: token.into(),
        };
        self
    }

    /// Use token from environment variable.
    pub fn token_from_env(mut self) -> Self {
        let vault = self.config.vault.get_or_insert_with(VaultConfig::default);
        if let Ok(token) = std::env::var("VAULT_TOKEN") {
            vault.auth = VaultAuthMethod::Token { token };
        }
        self
    }

    /// Set AppRole authentication.
    pub fn approle(mut self, role_id: impl Into<String>, secret_id: impl Into<String>) -> Self {
        let vault = self.config.vault.get_or_insert_with(VaultConfig::default);
        vault.auth = VaultAuthMethod::AppRole {
            role_id: role_id.into(),
            secret_id: secret_id.into(),
            mount_path: "approle".to_string(),
        };
        self
    }

    /// Set AWS region.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        let aws = self
            .config
            .aws
            .get_or_insert_with(AwsSecretsManagerConfig::default);
        aws.region = Some(region.into());
        self
    }

    /// Set AWS profile.
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        let aws = self
            .config
            .aws
            .get_or_insert_with(AwsSecretsManagerConfig::default);
        aws.profile = Some(profile.into());
        self
    }

    /// Enable caching with default settings.
    pub fn with_cache(mut self) -> Self {
        self.config.cache = Some(SecretCacheConfig::default());
        self
    }

    /// Configure caching.
    pub fn cache(mut self, cache_config: SecretCacheConfig) -> Self {
        self.config.cache = Some(cache_config);
        self
    }

    /// Enable rotation with the given configuration.
    pub fn with_rotation(mut self, rotation_config: RotationConfig) -> Self {
        self.config.rotation = Some(rotation_config);
        self
    }

    /// Set request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set maximum retries.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.config.max_retries = max_retries;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> super::error::SecretResult<SecretConfig> {
        // Validate configuration
        match self.config.backend_type {
            SecretBackendType::Vault => {
                if self.config.vault.is_none() {
                    return Err(super::error::SecretError::Configuration(
                        "Vault configuration required for Vault backend".into(),
                    ));
                }
            }
            SecretBackendType::AwsSecretsManager => {
                if self.config.aws.is_none() {
                    return Err(super::error::SecretError::Configuration(
                        "AWS configuration required for AWS Secrets Manager backend".into(),
                    ));
                }
            }
        }

        Ok(self.config)
    }
}

/// HashiCorp Vault configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    /// Vault server address (e.g., https://vault.example.com:8200)
    pub address: String,

    /// Authentication method
    pub auth: VaultAuthMethod,

    /// Namespace (for Vault Enterprise)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Default mount path for KV secrets
    pub kv_mount: String,

    /// KV version (1 or 2)
    pub kv_version: u8,

    /// TLS CA certificate path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<PathBuf>,

    /// Skip TLS verification (not recommended for production)
    #[serde(default)]
    pub skip_verify: bool,

    /// Automatic token renewal
    #[serde(default = "default_true")]
    pub auto_renew: bool,
}

fn default_true() -> bool {
    true
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            address: "http://127.0.0.1:8200".to_string(),
            auth: VaultAuthMethod::Token {
                token: String::new(),
            },
            namespace: None,
            kv_mount: "secret".to_string(),
            kv_version: 2,
            ca_cert: None,
            skip_verify: false,
            auto_renew: true,
        }
    }
}

impl VaultConfig {
    /// Create VaultConfig from environment variables.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = std::env::var("VAULT_ADDR") {
            config.address = addr;
        }

        if let Ok(token) = std::env::var("VAULT_TOKEN") {
            config.auth = VaultAuthMethod::Token { token };
        } else if let (Ok(role_id), Ok(secret_id)) = (
            std::env::var("VAULT_ROLE_ID"),
            std::env::var("VAULT_SECRET_ID"),
        ) {
            config.auth = VaultAuthMethod::AppRole {
                role_id,
                secret_id,
                mount_path: std::env::var("VAULT_APPROLE_MOUNT")
                    .unwrap_or_else(|_| "approle".to_string()),
            };
        }

        if let Ok(namespace) = std::env::var("VAULT_NAMESPACE") {
            config.namespace = Some(namespace);
        }

        if let Ok(ca_cert) = std::env::var("VAULT_CACERT") {
            config.ca_cert = Some(PathBuf::from(ca_cert));
        }

        if std::env::var("VAULT_SKIP_VERIFY").is_ok() {
            config.skip_verify = true;
        }

        config
    }
}

/// Vault authentication methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum VaultAuthMethod {
    /// Token-based authentication
    Token {
        /// The Vault token
        token: String,
    },

    /// AppRole authentication
    AppRole {
        /// Role ID
        role_id: String,
        /// Secret ID
        secret_id: String,
        /// Mount path for AppRole
        #[serde(default = "default_approle_mount")]
        mount_path: String,
    },

    /// Kubernetes authentication
    Kubernetes {
        /// Kubernetes role
        role: String,
        /// JWT token path (default: /var/run/secrets/kubernetes.io/serviceaccount/token)
        #[serde(default = "default_k8s_token_path")]
        jwt_path: PathBuf,
        /// Mount path
        #[serde(default = "default_k8s_mount")]
        mount_path: String,
    },

    /// AWS IAM authentication
    AwsIam {
        /// AWS role to assume
        role: String,
        /// Mount path
        #[serde(default = "default_aws_mount")]
        mount_path: String,
    },

    /// LDAP authentication
    Ldap {
        /// LDAP username
        username: String,
        /// LDAP password
        password: String,
        /// Mount path
        #[serde(default = "default_ldap_mount")]
        mount_path: String,
    },

    /// Userpass authentication
    Userpass {
        /// Username
        username: String,
        /// Password
        password: String,
        /// Mount path
        #[serde(default = "default_userpass_mount")]
        mount_path: String,
    },
}

fn default_approle_mount() -> String {
    "approle".to_string()
}

fn default_k8s_token_path() -> PathBuf {
    PathBuf::from("/var/run/secrets/kubernetes.io/serviceaccount/token")
}

fn default_k8s_mount() -> String {
    "kubernetes".to_string()
}

fn default_aws_mount() -> String {
    "aws".to_string()
}

fn default_ldap_mount() -> String {
    "ldap".to_string()
}

fn default_userpass_mount() -> String {
    "userpass".to_string()
}

impl Default for VaultAuthMethod {
    fn default() -> Self {
        Self::Token {
            token: String::new(),
        }
    }
}

/// AWS Secrets Manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AwsSecretsManagerConfig {
    /// AWS region
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// AWS profile name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// AWS access key ID (if not using profile/IAM role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,

    /// AWS secret access key (if not using profile/IAM role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,

    /// AWS session token (for temporary credentials)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,

    /// Custom endpoint URL (for LocalStack, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,

    /// Use FIPS endpoints
    #[serde(default)]
    pub use_fips: bool,

    /// Use dual-stack endpoints
    #[serde(default)]
    pub use_dual_stack: bool,
}

impl AwsSecretsManagerConfig {
    /// Create from environment variables.
    pub fn from_env() -> Self {
        Self {
            region: std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .ok(),
            profile: std::env::var("AWS_PROFILE").ok(),
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
            endpoint_url: std::env::var("AWS_ENDPOINT_URL").ok(),
            use_fips: std::env::var("AWS_USE_FIPS_ENDPOINT")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            use_dual_stack: std::env::var("AWS_USE_DUALSTACK_ENDPOINT")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }

    /// Create with explicit credentials.
    pub fn with_credentials(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            region: Some(region.into()),
            access_key_id: Some(access_key_id.into()),
            secret_access_key: Some(secret_access_key.into()),
            ..Default::default()
        }
    }

    /// Create with a profile.
    pub fn with_profile(region: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            region: Some(region.into()),
            profile: Some(profile.into()),
            ..Default::default()
        }
    }
}

/// Duration serialization using humantime.
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&humantime::format_duration(*duration).to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecretConfig::default();
        assert_eq!(config.backend_type, SecretBackendType::Vault);
        assert!(config.cache.is_some());
        assert!(config.rotation.is_none());
    }

    #[test]
    fn test_vault_config_builder() {
        let config = SecretConfig::vault()
            .address("https://vault.example.com:8200")
            .token("hvs.token123")
            .with_cache()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();

        assert_eq!(config.backend_type, SecretBackendType::Vault);
        assert!(config.vault.is_some());
        let vault = config.vault.unwrap();
        assert_eq!(vault.address, "https://vault.example.com:8200");
        assert!(matches!(vault.auth, VaultAuthMethod::Token { .. }));
    }

    #[test]
    fn test_aws_config_builder() {
        let config = SecretConfig::aws_secrets_manager()
            .region("us-west-2")
            .profile("production")
            .build()
            .unwrap();

        assert_eq!(config.backend_type, SecretBackendType::AwsSecretsManager);
        assert!(config.aws.is_some());
        let aws = config.aws.unwrap();
        assert_eq!(aws.region, Some("us-west-2".to_string()));
        assert_eq!(aws.profile, Some("production".to_string()));
    }

    #[test]
    fn test_vault_auth_methods() {
        let token = VaultAuthMethod::Token {
            token: "test".to_string(),
        };
        assert!(matches!(token, VaultAuthMethod::Token { .. }));

        let approle = VaultAuthMethod::AppRole {
            role_id: "role".to_string(),
            secret_id: "secret".to_string(),
            mount_path: "approle".to_string(),
        };
        assert!(matches!(approle, VaultAuthMethod::AppRole { .. }));
    }
}
