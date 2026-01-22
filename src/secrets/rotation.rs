//! Secret rotation support.
//!
//! This module provides mechanisms for automatic and manual secret rotation,
//! supporting various rotation strategies and policies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::backend::SecretBackend;
use super::error::{SecretError, SecretResult};
use super::types::{Secret, SecretValue, SecretVersion};

/// Configuration for secret rotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RotationConfig {
    /// Enable automatic rotation
    pub enabled: bool,

    /// Default rotation interval
    #[serde(with = "humantime_serde")]
    pub default_interval: Duration,

    /// Per-secret rotation policies
    #[serde(default)]
    pub policies: HashMap<String, RotationPolicy>,

    /// Secret generator configuration
    pub generator: SecretGeneratorConfig,

    /// Number of previous versions to keep
    pub versions_to_keep: u32,

    /// Notification webhook URL for rotation events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_interval: Duration::from_secs(86400 * 30), // 30 days
            policies: HashMap::new(),
            generator: SecretGeneratorConfig::default(),
            versions_to_keep: 3,
            webhook_url: None,
        }
    }
}

impl RotationConfig {
    /// Create a config with automatic rotation enabled.
    pub fn auto(interval: Duration) -> Self {
        Self {
            enabled: true,
            default_interval: interval,
            ..Default::default()
        }
    }

    /// Add a rotation policy for a specific secret pattern.
    pub fn with_policy(mut self, pattern: impl Into<String>, policy: RotationPolicy) -> Self {
        self.policies.insert(pattern.into(), policy);
        self
    }
}

/// Rotation policy for a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationPolicy {
    /// Rotation interval for this secret
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Secret type (determines how to generate new value)
    pub secret_type: SecretType,

    /// Whether rotation is enabled for this secret
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Custom rotation lambda/function ARN (AWS only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_lambda_arn: Option<String>,

    /// Keys to rotate within the secret
    #[serde(default)]
    pub keys_to_rotate: Vec<String>,

    /// Grace period after rotation before old secret expires
    #[serde(with = "humantime_serde", default = "default_grace_period")]
    pub grace_period: Duration,
}

fn default_true() -> bool {
    true
}

fn default_grace_period() -> Duration {
    Duration::from_secs(3600) // 1 hour
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(86400 * 30), // 30 days
            secret_type: SecretType::Password,
            enabled: true,
            rotation_lambda_arn: None,
            keys_to_rotate: vec!["password".to_string()],
            grace_period: Duration::from_secs(3600),
        }
    }
}

/// Type of secret (determines generation strategy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretType {
    /// Random password
    Password,
    /// API key
    ApiKey,
    /// Token (JWT-like)
    Token,
    /// Encryption key (binary)
    EncryptionKey,
    /// Database credentials
    DatabaseCredentials,
    /// SSH key pair
    SshKey,
    /// Certificate
    Certificate,
    /// Custom (uses provided generator)
    Custom,
}

/// Configuration for secret generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretGeneratorConfig {
    /// Default password length
    pub password_length: usize,

    /// Include uppercase letters
    pub include_uppercase: bool,

    /// Include lowercase letters
    pub include_lowercase: bool,

    /// Include numbers
    pub include_numbers: bool,

    /// Include special characters
    pub include_special: bool,

    /// Special characters to use
    pub special_chars: String,

    /// Exclude ambiguous characters (0, O, l, 1, etc.)
    pub exclude_ambiguous: bool,

    /// API key length
    pub api_key_length: usize,

    /// Encryption key size in bytes
    pub encryption_key_size: usize,
}

impl Default for SecretGeneratorConfig {
    fn default() -> Self {
        Self {
            password_length: 32,
            include_uppercase: true,
            include_lowercase: true,
            include_numbers: true,
            include_special: true,
            special_chars: "!@#$%^&*()_+-=[]{}|;:,.<>?".to_string(),
            exclude_ambiguous: true,
            api_key_length: 40,
            encryption_key_size: 32,
        }
    }
}

/// Result of a secret rotation.
#[derive(Debug, Clone)]
pub struct RotationResult {
    /// Path of the rotated secret
    pub path: String,

    /// Old version identifier
    pub old_version: Option<SecretVersion>,

    /// New version identifier
    pub new_version: Option<SecretVersion>,

    /// Keys that were rotated
    pub rotated_keys: Vec<String>,

    /// Rotation timestamp
    pub rotated_at: chrono::DateTime<chrono::Utc>,

    /// Whether rotation was successful
    pub success: bool,

    /// Error message if rotation failed
    pub error: Option<String>,
}

impl RotationResult {
    /// Create a successful rotation result.
    pub fn success(
        path: impl Into<String>,
        old_version: Option<SecretVersion>,
        new_version: Option<SecretVersion>,
        rotated_keys: Vec<String>,
    ) -> Self {
        Self {
            path: path.into(),
            old_version,
            new_version,
            rotated_keys,
            rotated_at: chrono::Utc::now(),
            success: true,
            error: None,
        }
    }

    /// Create a failed rotation result.
    pub fn failure(path: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            old_version: None,
            new_version: None,
            rotated_keys: Vec::new(),
            rotated_at: chrono::Utc::now(),
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Secret rotator implementation.
pub struct SecretRotator {
    /// Rotation configuration
    config: RotationConfig,

    /// Backend for secret operations
    backend: Arc<dyn SecretBackend>,

    /// Secret generator
    generator: SecretGenerator,
}

impl SecretRotator {
    /// Create a new rotator with the given configuration and backend.
    pub fn new(config: RotationConfig, backend: Arc<dyn SecretBackend>) -> Self {
        let generator = SecretGenerator::new(config.generator.clone());

        Self {
            config,
            backend,
            generator,
        }
    }

    /// Rotate a secret.
    pub async fn rotate(&self, path: &str) -> SecretResult<RotationResult> {
        // Get the policy for this secret
        let policy = self.get_policy(path);

        if !policy.enabled {
            return Err(SecretError::rotation(
                path,
                "Rotation is disabled for this secret",
            ));
        }

        // Get the current secret
        let mut current_secret = match self.backend.get_secret(path).await {
            Ok(secret) => secret,
            Err(SecretError::NotFound(_)) => {
                // Secret doesn't exist, create a new one
                return self.create_initial_secret(path, &policy).await;
            }
            Err(e) => return Err(e),
        };

        let old_version = current_secret.metadata().version.clone();

        // Rotate the specified keys
        let rotated_keys = self.rotate_keys(&mut current_secret, &policy)?;

        // Write the updated secret
        self.backend.put_secret(path, &current_secret).await?;

        // Get the new version
        let new_secret = self.backend.get_secret(path).await?;
        let new_version = new_secret.metadata().version.clone();

        tracing::info!(
            path = %path,
            rotated_keys = ?rotated_keys,
            "Secret rotated successfully"
        );

        Ok(RotationResult::success(
            path,
            old_version,
            new_version,
            rotated_keys,
        ))
    }

    /// Get the rotation policy for a secret path.
    fn get_policy(&self, path: &str) -> RotationPolicy {
        // Check for exact match first
        if let Some(policy) = self.config.policies.get(path) {
            return policy.clone();
        }

        // Check for pattern matches
        for (pattern, policy) in &self.config.policies {
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if path.starts_with(prefix) {
                    return policy.clone();
                }
            }
        }

        // Return default policy
        RotationPolicy {
            interval: self.config.default_interval,
            ..Default::default()
        }
    }

    /// Rotate the specified keys in a secret.
    fn rotate_keys(
        &self,
        secret: &mut Secret,
        policy: &RotationPolicy,
    ) -> SecretResult<Vec<String>> {
        let keys_to_rotate = if policy.keys_to_rotate.is_empty() {
            // Rotate all keys
            secret.keys().cloned().collect()
        } else {
            policy.keys_to_rotate.clone()
        };

        let mut rotated = Vec::new();

        for key in &keys_to_rotate {
            if secret.contains_key(key) {
                let new_value = self.generator.generate(&policy.secret_type)?;
                secret.insert(key.clone(), new_value);
                rotated.push(key.clone());
            }
        }

        if rotated.is_empty() {
            return Err(SecretError::rotation(
                secret.path(),
                "No keys found to rotate",
            ));
        }

        Ok(rotated)
    }

    /// Create an initial secret when it doesn't exist.
    async fn create_initial_secret(
        &self,
        path: &str,
        policy: &RotationPolicy,
    ) -> SecretResult<RotationResult> {
        let mut data = HashMap::new();

        for key in &policy.keys_to_rotate {
            let value = self.generator.generate(&policy.secret_type)?;
            data.insert(key.clone(), value);
        }

        if data.is_empty() {
            // Default key
            let value = self.generator.generate(&policy.secret_type)?;
            data.insert("password".to_string(), value);
        }

        let secret = Secret::new(path, data);
        self.backend.put_secret(path, &secret).await?;

        let created_secret = self.backend.get_secret(path).await?;
        let new_version = created_secret.metadata().version.clone();

        Ok(RotationResult::success(
            path,
            None,
            new_version,
            policy.keys_to_rotate.clone(),
        ))
    }

    /// Check if a secret needs rotation.
    pub async fn needs_rotation(&self, path: &str) -> SecretResult<bool> {
        let policy = self.get_policy(path);

        if !policy.enabled {
            return Ok(false);
        }

        let secret = self.backend.get_secret(path).await?;
        let metadata = secret.metadata();

        if let Some(updated_time) = metadata.updated_time {
            let last_rotation =
                chrono::DateTime::from_timestamp(updated_time, 0).unwrap_or_else(chrono::Utc::now);
            let age = chrono::Utc::now().signed_duration_since(last_rotation);
            let threshold = chrono::Duration::from_std(policy.interval)
                .unwrap_or_else(|_| chrono::Duration::days(30));

            return Ok(age > threshold);
        }

        // If no update time, assume rotation is needed
        Ok(true)
    }
}

impl std::fmt::Debug for SecretRotator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRotator")
            .field("config", &self.config)
            .finish()
    }
}

/// Secret value generator.
pub struct SecretGenerator {
    config: SecretGeneratorConfig,
}

impl SecretGenerator {
    /// Create a new generator with the given configuration.
    pub fn new(config: SecretGeneratorConfig) -> Self {
        Self { config }
    }

    /// Generate a new secret value based on the secret type.
    pub fn generate(&self, secret_type: &SecretType) -> SecretResult<SecretValue> {
        match secret_type {
            SecretType::Password => self.generate_password(),
            SecretType::ApiKey => self.generate_api_key(),
            SecretType::Token => self.generate_token(),
            SecretType::EncryptionKey => self.generate_encryption_key(),
            SecretType::DatabaseCredentials => self.generate_password(),
            SecretType::SshKey | SecretType::Certificate | SecretType::Custom => {
                Err(SecretError::Configuration(format!(
                    "Secret type {:?} requires custom generator",
                    secret_type
                )))
            }
        }
    }

    /// Generate a random password.
    fn generate_password(&self) -> SecretResult<SecretValue> {
        use rand::Rng;

        let mut charset = String::new();

        if self.config.include_uppercase {
            charset.push_str(if self.config.exclude_ambiguous {
                "ABCDEFGHJKLMNPQRSTUVWXYZ"
            } else {
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
            });
        }

        if self.config.include_lowercase {
            charset.push_str(if self.config.exclude_ambiguous {
                "abcdefghjkmnpqrstuvwxyz"
            } else {
                "abcdefghijklmnopqrstuvwxyz"
            });
        }

        if self.config.include_numbers {
            charset.push_str(if self.config.exclude_ambiguous {
                "23456789"
            } else {
                "0123456789"
            });
        }

        if self.config.include_special {
            charset.push_str(&self.config.special_chars);
        }

        if charset.is_empty() {
            return Err(SecretError::Configuration(
                "No character set available for password generation".into(),
            ));
        }

        let charset: Vec<char> = charset.chars().collect();
        let mut rng = rand::thread_rng();

        let password: String = (0..self.config.password_length)
            .map(|_| charset[rng.gen_range(0..charset.len())])
            .collect();

        Ok(SecretValue::String(password))
    }

    /// Generate a random API key.
    fn generate_api_key(&self) -> SecretResult<SecretValue> {
        use rand::Rng;

        let charset: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            .chars()
            .collect();
        let mut rng = rand::thread_rng();

        let key: String = (0..self.config.api_key_length)
            .map(|_| charset[rng.gen_range(0..charset.len())])
            .collect();

        Ok(SecretValue::String(key))
    }

    /// Generate a random token.
    fn generate_token(&self) -> SecretResult<SecretValue> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use rand::RngCore;

        let mut bytes = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);

        let token = URL_SAFE_NO_PAD.encode(&bytes);
        Ok(SecretValue::String(token))
    }

    /// Generate a random encryption key.
    fn generate_encryption_key(&self) -> SecretResult<SecretValue> {
        use rand::RngCore;

        let mut key = vec![0u8; self.config.encryption_key_size];
        rand::thread_rng().fill_bytes(&mut key);

        Ok(SecretValue::Binary(key))
    }
}

impl Default for SecretGenerator {
    fn default() -> Self {
        Self::new(SecretGeneratorConfig::default())
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
    fn test_password_generation() {
        let generator = SecretGenerator::default();
        let password = generator.generate(&SecretType::Password).unwrap();

        if let SecretValue::String(pwd) = password {
            assert_eq!(pwd.len(), 32);
            // Should contain mixed characters
            assert!(pwd.chars().any(|c| c.is_uppercase()));
            assert!(pwd.chars().any(|c| c.is_lowercase()));
            assert!(pwd.chars().any(|c| c.is_ascii_digit()));
        } else {
            panic!("Expected string value");
        }
    }

    #[test]
    fn test_api_key_generation() {
        let generator = SecretGenerator::default();
        let key = generator.generate(&SecretType::ApiKey).unwrap();

        if let SecretValue::String(k) = key {
            assert_eq!(k.len(), 40);
            assert!(k.chars().all(|c| c.is_alphanumeric()));
        } else {
            panic!("Expected string value");
        }
    }

    #[test]
    fn test_encryption_key_generation() {
        let generator = SecretGenerator::default();
        let key = generator.generate(&SecretType::EncryptionKey).unwrap();

        if let SecretValue::Binary(k) = key {
            assert_eq!(k.len(), 32);
        } else {
            panic!("Expected binary value");
        }
    }

    #[test]
    fn test_token_generation() {
        let generator = SecretGenerator::default();
        let token = generator.generate(&SecretType::Token).unwrap();

        if let SecretValue::String(t) = token {
            assert!(!t.is_empty());
            // Should be URL-safe base64
            assert!(t
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        } else {
            panic!("Expected string value");
        }
    }

    #[test]
    fn test_rotation_result() {
        let success = RotationResult::success(
            "secret/test",
            Some(SecretVersion::Numeric(1)),
            Some(SecretVersion::Numeric(2)),
            vec!["password".to_string()],
        );

        assert!(success.success);
        assert!(success.error.is_none());

        let failure = RotationResult::failure("secret/test", "Something went wrong");
        assert!(!failure.success);
        assert!(failure.error.is_some());
    }

    #[test]
    fn test_rotation_policy_default() {
        let policy = RotationPolicy::default();
        assert!(policy.enabled);
        assert_eq!(policy.secret_type, SecretType::Password);
        assert_eq!(policy.keys_to_rotate, vec!["password".to_string()]);
    }

    #[test]
    fn test_rotation_config_builder() {
        let config = RotationConfig::auto(Duration::from_secs(3600)).with_policy(
            "secret/database/*",
            RotationPolicy {
                interval: Duration::from_secs(86400),
                secret_type: SecretType::DatabaseCredentials,
                ..Default::default()
            },
        );

        assert!(config.enabled);
        assert!(config.policies.contains_key("secret/database/*"));
    }
}
