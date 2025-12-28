//! AWS Credentials Chain Implementation
//!
//! This module provides AWS credential resolution following the standard credential chain:
//! 1. Explicit configuration (access_key, secret_key, session_token)
//! 2. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN)
//! 3. Shared credentials file (~/.aws/credentials) with profile support
//! 4. Instance metadata (IMDS) for EC2 instances
//!
//! # Example
//!
//! ```rust,ignore
//! use serde_json::json;
//! use rustible::provisioning::providers::aws::credentials::resolve_credentials;
//!
//! // From explicit config
//! let config = json!({
//!     "access_key": "AKIAIOSFODNN7EXAMPLE",
//!     "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
//! });
//! let creds = resolve_credentials(&config).await?;
//!
//! // Using default credential chain
//! let config = json!({});
//! let creds = resolve_credentials(&config).await?;
//! ```

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::ProviderCredentials;

// ============================================================================
// Credential Source Types
// ============================================================================

/// Source of AWS credentials
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    /// Explicitly provided in configuration
    Explicit,
    /// Loaded from environment variables
    Environment,
    /// Loaded from shared credentials file
    SharedCredentials {
        /// Profile name used
        profile: String,
    },
    /// Loaded from EC2 instance metadata service (IMDS)
    InstanceMetadata,
}

impl fmt::Display for CredentialSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Explicit => write!(f, "explicit configuration"),
            Self::Environment => write!(f, "environment variables"),
            Self::SharedCredentials { profile } => {
                write!(f, "shared credentials file (profile: {})", profile)
            }
            Self::InstanceMetadata => write!(f, "EC2 instance metadata"),
        }
    }
}

// ============================================================================
// AWS Credentials
// ============================================================================

/// AWS credentials with source tracking and expiration support
#[derive(Clone, Serialize, Deserialize)]
pub struct AwsCredentials {
    /// AWS Access Key ID
    pub access_key_id: String,
    /// AWS Secret Access Key (sensitive)
    #[serde(skip_serializing)]
    pub secret_access_key: String,
    /// Optional session token for temporary credentials
    #[serde(skip_serializing)]
    pub session_token: Option<String>,
    /// Expiration time for temporary credentials
    pub expiration: Option<DateTime<Utc>>,
    /// Source where credentials were loaded from
    pub source: CredentialSource,
}

impl fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AwsCredentials")
            .field("access_key_id", &mask_access_key(&self.access_key_id))
            .field("secret_access_key", &"********")
            .field("session_token", &self.session_token.as_ref().map(|_| "***"))
            .field("expiration", &self.expiration)
            .field("source", &self.source)
            .finish()
    }
}

impl AwsCredentials {
    /// Create new credentials with explicit values
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        source: CredentialSource,
    ) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
            expiration: None,
            source,
        }
    }

    /// Add session token for temporary credentials
    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }

    /// Set expiration time
    pub fn with_expiration(mut self, expiration: DateTime<Utc>) -> Self {
        self.expiration = Some(expiration);
        self
    }

    /// Check if these are temporary credentials (have session token)
    pub fn is_temporary(&self) -> bool {
        self.session_token.is_some()
    }

    /// Get time until expiration (if any)
    pub fn time_until_expiration(&self) -> Option<chrono::Duration> {
        self.expiration.map(|exp| exp - Utc::now())
    }

    /// Check if credentials need refresh (less than 5 minutes until expiration)
    pub fn needs_refresh(&self) -> bool {
        if let Some(exp) = self.expiration {
            let now = Utc::now();
            let five_minutes = chrono::Duration::minutes(5);
            exp - now < five_minutes
        } else {
            false
        }
    }
}

impl ProviderCredentials for AwsCredentials {
    fn credential_type(&self) -> &str {
        "aws"
    }

    fn is_expired(&self) -> bool {
        if let Some(exp) = self.expiration {
            Utc::now() >= exp
        } else {
            false
        }
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "aws",
            "access_key_id": mask_access_key(&self.access_key_id),
            "source": self.source,
            "has_session_token": self.session_token.is_some(),
            "expiration": self.expiration,
            "is_expired": self.is_expired(),
        })
    }
}

// ============================================================================
// Credential Chain
// ============================================================================

/// AWS credential chain that tries multiple sources in order
pub struct AwsCredentialChain {
    /// Configuration values for explicit credentials
    config: Value,
    /// Profile name to use for shared credentials
    profile: Option<String>,
    /// Path to credentials file (defaults to ~/.aws/credentials)
    credentials_file: Option<PathBuf>,
    /// Whether to use IMDS for EC2 instances
    use_imds: bool,
}

impl AwsCredentialChain {
    /// Create a new credential chain from configuration
    pub fn new(config: &Value) -> Self {
        let profile = config
            .get("profile")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| std::env::var("AWS_PROFILE").ok());

        let credentials_file = config
            .get("credentials_file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let use_imds = config
            .get("use_imds")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Self {
            config: config.clone(),
            profile,
            credentials_file,
            use_imds,
        }
    }

    /// Resolve credentials from the chain
    pub async fn resolve(&self) -> ProvisioningResult<AwsCredentials> {
        // 1. Try explicit configuration
        if let Some(creds) = self.try_explicit()? {
            tracing::debug!("AWS credentials resolved from explicit configuration");
            return Ok(creds);
        }

        // 2. Try environment variables
        if let Some(creds) = self.try_environment()? {
            tracing::debug!("AWS credentials resolved from environment variables");
            return Ok(creds);
        }

        // 3. Try shared credentials file
        if let Some(creds) = self.try_shared_credentials().await? {
            tracing::debug!("AWS credentials resolved from shared credentials file");
            return Ok(creds);
        }

        // 4. Try instance metadata (IMDS)
        if self.use_imds {
            if let Some(creds) = self.try_instance_metadata().await? {
                tracing::debug!("AWS credentials resolved from instance metadata");
                return Ok(creds);
            }
        }

        Err(ProvisioningError::AuthenticationError {
            provider: "aws".to_string(),
            message: "No AWS credentials found. Tried: explicit config, environment variables, \
                     shared credentials file (~/.aws/credentials), and instance metadata (IMDS). \
                     Please configure AWS credentials using one of these methods."
                .to_string(),
        })
    }

    /// Try to load credentials from explicit configuration
    fn try_explicit(&self) -> ProvisioningResult<Option<AwsCredentials>> {
        let access_key = self
            .config
            .get("access_key")
            .or_else(|| self.config.get("access_key_id"))
            .and_then(|v| v.as_str());

        let secret_key = self
            .config
            .get("secret_key")
            .or_else(|| self.config.get("secret_access_key"))
            .and_then(|v| v.as_str());

        match (access_key, secret_key) {
            (Some(ak), Some(sk)) if !ak.is_empty() && !sk.is_empty() => {
                let mut creds = AwsCredentials::new(ak, sk, CredentialSource::Explicit);

                // Check for session token
                if let Some(token) = self
                    .config
                    .get("session_token")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    creds = creds.with_session_token(token);
                }

                Ok(Some(creds))
            }
            // Both provided but at least one is empty - treat as incomplete
            (Some(_), Some(_)) => Err(ProvisioningError::AuthenticationError {
                provider: "aws".to_string(),
                message:
                    "Incomplete explicit credentials: access_key and secret_key must not be empty"
                        .to_string(),
            }),
            (Some(_), None) | (None, Some(_)) => Err(ProvisioningError::AuthenticationError {
                provider: "aws".to_string(),
                message:
                    "Incomplete explicit credentials: both access_key and secret_key are required"
                        .to_string(),
            }),
            (None, None) => Ok(None),
        }
    }

    /// Try to load credentials from environment variables
    fn try_environment(&self) -> ProvisioningResult<Option<AwsCredentials>> {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();

        match (access_key, secret_key) {
            (Some(ak), Some(sk)) if !ak.is_empty() && !sk.is_empty() => {
                let mut creds = AwsCredentials::new(ak, sk, CredentialSource::Environment);

                // Check for session token
                if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
                    if !token.is_empty() {
                        creds = creds.with_session_token(token);
                    }
                }

                Ok(Some(creds))
            }
            _ => Ok(None),
        }
    }

    /// Try to load credentials from shared credentials file
    async fn try_shared_credentials(&self) -> ProvisioningResult<Option<AwsCredentials>> {
        let credentials_path = self.get_credentials_file_path()?;

        if !credentials_path.exists() {
            tracing::trace!(
                "Shared credentials file not found: {}",
                credentials_path.display()
            );
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&credentials_path)
            .await
            .map_err(|e| ProvisioningError::AuthenticationError {
                provider: "aws".to_string(),
                message: format!(
                    "Failed to read credentials file {}: {}",
                    credentials_path.display(),
                    e
                ),
            })?;

        let profile = self.profile.as_deref().unwrap_or("default");
        self.parse_credentials_file(&content, profile)
    }

    /// Get the path to the credentials file
    fn get_credentials_file_path(&self) -> ProvisioningResult<PathBuf> {
        if let Some(ref path) = self.credentials_file {
            return Ok(path.clone());
        }

        // Check AWS_SHARED_CREDENTIALS_FILE environment variable
        if let Ok(path) = std::env::var("AWS_SHARED_CREDENTIALS_FILE") {
            return Ok(PathBuf::from(path));
        }

        // Default to ~/.aws/credentials
        dirs::home_dir()
            .map(|h| h.join(".aws").join("credentials"))
            .ok_or_else(|| ProvisioningError::AuthenticationError {
                provider: "aws".to_string(),
                message: "Could not determine home directory for credentials file".to_string(),
            })
    }

    /// Parse INI-style credentials file
    fn parse_credentials_file(
        &self,
        content: &str,
        profile: &str,
    ) -> ProvisioningResult<Option<AwsCredentials>> {
        let mut current_profile: Option<String> = None;
        let mut profiles: HashMap<String, HashMap<String, String>> = HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Profile header
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].trim().to_string();
                current_profile = Some(name.clone());
                profiles.entry(name).or_default();
                continue;
            }

            // Key-value pair
            if let Some(ref profile_name) = current_profile {
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim().to_lowercase();
                    let value = value.trim().to_string();
                    profiles.get_mut(profile_name).unwrap().insert(key, value);
                }
            }
        }

        // Look up the requested profile
        let profile_data = profiles.get(profile);

        if let Some(data) = profile_data {
            let access_key = data
                .get("aws_access_key_id")
                .or_else(|| data.get("access_key_id"));
            let secret_key = data
                .get("aws_secret_access_key")
                .or_else(|| data.get("secret_access_key"));

            match (access_key, secret_key) {
                (Some(ak), Some(sk)) if !ak.is_empty() && !sk.is_empty() => {
                    let mut creds = AwsCredentials::new(
                        ak.clone(),
                        sk.clone(),
                        CredentialSource::SharedCredentials {
                            profile: profile.to_string(),
                        },
                    );

                    // Check for session token
                    if let Some(token) = data
                        .get("aws_session_token")
                        .or_else(|| data.get("session_token"))
                    {
                        if !token.is_empty() {
                            creds = creds.with_session_token(token.clone());
                        }
                    }

                    return Ok(Some(creds));
                }
                _ => {
                    tracing::trace!(
                        "Profile '{}' found but missing access_key or secret_key",
                        profile
                    );
                }
            }
        } else {
            tracing::trace!("Profile '{}' not found in credentials file", profile);
        }

        Ok(None)
    }

    /// Try to load credentials from EC2 instance metadata service (IMDS)
    async fn try_instance_metadata(&self) -> ProvisioningResult<Option<AwsCredentials>> {
        // IMDS v2 token endpoint
        const IMDS_TOKEN_URL: &str = "http://169.254.169.254/latest/api/token";
        const IMDS_ROLE_URL: &str =
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/";

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .map_err(|e| ProvisioningError::AuthenticationError {
                provider: "aws".to_string(),
                message: format!("Failed to create HTTP client for IMDS: {}", e),
            })?;

        // Get IMDSv2 token
        let token = match client
            .put(IMDS_TOKEN_URL)
            .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp.text().await.ok(),
            Ok(_) => {
                tracing::trace!("IMDS token request failed (not on EC2?)");
                return Ok(None);
            }
            Err(e) => {
                tracing::trace!("IMDS not available: {}", e);
                return Ok(None);
            }
        };

        let token = match token {
            Some(t) => t,
            None => return Ok(None),
        };

        // Get IAM role name
        let role_name = match client
            .get(IMDS_ROLE_URL)
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.text().await {
                Ok(text) => text.trim().to_string(),
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        };

        if role_name.is_empty() {
            tracing::trace!("No IAM role attached to instance");
            return Ok(None);
        }

        // Get credentials for the role
        let creds_url = format!("{}{}", IMDS_ROLE_URL, role_name);
        let creds_response = match client
            .get(&creds_url)
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ImdsCredentialsResponse>().await {
                    Ok(creds) => creds,
                    Err(e) => {
                        tracing::warn!("Failed to parse IMDS credentials response: {}", e);
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        };

        let expiration = chrono::DateTime::parse_from_rfc3339(&creds_response.expiration)
            .map(|dt| dt.with_timezone(&Utc))
            .ok();

        let mut creds = AwsCredentials::new(
            creds_response.access_key_id,
            creds_response.secret_access_key,
            CredentialSource::InstanceMetadata,
        );

        creds = creds.with_session_token(creds_response.token);

        if let Some(exp) = expiration {
            creds = creds.with_expiration(exp);
        }

        Ok(Some(creds))
    }
}

/// Response structure from IMDS security credentials endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImdsCredentialsResponse {
    access_key_id: String,
    secret_access_key: String,
    token: String,
    expiration: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Mask access key ID for logging (show first 4 and last 4 characters)
fn mask_access_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

/// Resolve AWS credentials from configuration using the credential chain
///
/// # Arguments
///
/// * `config` - Configuration value that may contain explicit credentials or chain options
///
/// # Returns
///
/// Resolved AWS credentials or an error if no credentials could be found
///
/// # Example
///
/// ```rust,ignore
/// use serde_json::json;
///
/// // Explicit credentials
/// let config = json!({
///     "access_key": "AKIAIOSFODNN7EXAMPLE",
///     "secret_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
/// });
/// let creds = resolve_credentials(&config).await?;
///
/// // Use specific profile
/// let config = json!({
///     "profile": "production"
/// });
/// let creds = resolve_credentials(&config).await?;
///
/// // Default chain (env vars, then credentials file, then IMDS)
/// let config = json!({});
/// let creds = resolve_credentials(&config).await?;
/// ```
pub async fn resolve_credentials(config: &Value) -> ProvisioningResult<AwsCredentials> {
    let chain = AwsCredentialChain::new(config);
    chain.resolve().await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_credential_source_display() {
        assert_eq!(
            CredentialSource::Explicit.to_string(),
            "explicit configuration"
        );
        assert_eq!(
            CredentialSource::Environment.to_string(),
            "environment variables"
        );
        assert_eq!(
            CredentialSource::SharedCredentials {
                profile: "default".to_string()
            }
            .to_string(),
            "shared credentials file (profile: default)"
        );
        assert_eq!(
            CredentialSource::InstanceMetadata.to_string(),
            "EC2 instance metadata"
        );
    }

    #[test]
    fn test_aws_credentials_new() {
        let creds = AwsCredentials::new("AKIATEST", "secret123", CredentialSource::Explicit);

        assert_eq!(creds.access_key_id, "AKIATEST");
        assert_eq!(creds.secret_access_key, "secret123");
        assert!(creds.session_token.is_none());
        assert!(creds.expiration.is_none());
        assert_eq!(creds.source, CredentialSource::Explicit);
    }

    #[test]
    fn test_aws_credentials_with_session_token() {
        let creds = AwsCredentials::new("AKIATEST", "secret123", CredentialSource::Environment)
            .with_session_token("token123");

        assert!(creds.is_temporary());
        assert_eq!(creds.session_token, Some("token123".to_string()));
    }

    #[test]
    fn test_aws_credentials_expiration() {
        // Non-expiring credentials
        let creds = AwsCredentials::new("AKIATEST", "secret123", CredentialSource::Explicit);
        assert!(!creds.is_expired());
        assert!(!creds.needs_refresh());

        // Expired credentials
        let expired =
            AwsCredentials::new("AKIATEST", "secret123", CredentialSource::InstanceMetadata)
                .with_expiration(Utc::now() - chrono::Duration::hours(1));
        assert!(expired.is_expired());
        assert!(expired.needs_refresh());

        // Soon-to-expire credentials (needs refresh)
        let soon_expiring =
            AwsCredentials::new("AKIATEST", "secret123", CredentialSource::InstanceMetadata)
                .with_expiration(Utc::now() + chrono::Duration::minutes(3));
        assert!(!soon_expiring.is_expired());
        assert!(soon_expiring.needs_refresh());

        // Valid credentials (not needing refresh)
        let valid =
            AwsCredentials::new("AKIATEST", "secret123", CredentialSource::InstanceMetadata)
                .with_expiration(Utc::now() + chrono::Duration::hours(1));
        assert!(!valid.is_expired());
        assert!(!valid.needs_refresh());
    }

    #[test]
    fn test_provider_credentials_trait() {
        let creds = AwsCredentials::new("AKIATEST123456", "secret123", CredentialSource::Explicit);

        assert_eq!(creds.credential_type(), "aws");
        assert!(!creds.is_expired());

        let value = creds.as_value();
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("aws"));
        assert_eq!(
            value.get("access_key_id").and_then(|v| v.as_str()),
            Some("AKIA...3456")
        );
    }

    #[test]
    fn test_mask_access_key() {
        assert_eq!(mask_access_key("AKIAIOSFODNN7EXAMPLE"), "AKIA...MPLE");
        assert_eq!(mask_access_key("SHORT"), "****");
        assert_eq!(mask_access_key("12345678"), "****");
        assert_eq!(mask_access_key("123456789"), "1234...6789");
    }

    #[test]
    fn test_credentials_debug_masks_secrets() {
        let creds = AwsCredentials::new(
            "AKIAIOSFODNN7EXAMPLE",
            "supersecretkey",
            CredentialSource::Explicit,
        )
        .with_session_token("session_token_value");

        let debug_output = format!("{:?}", creds);

        // Should contain masked access key
        assert!(debug_output.contains("AKIA...MPLE"));
        // Should NOT contain actual secret key
        assert!(!debug_output.contains("supersecretkey"));
        // Should NOT contain actual session token
        assert!(!debug_output.contains("session_token_value"));
        // Should contain masked indicators
        assert!(debug_output.contains("********"));
        assert!(debug_output.contains("***"));
    }

    #[test]
    fn test_try_explicit_valid() {
        let config = json!({
            "access_key": "AKIATEST",
            "secret_key": "secrettest"
        });

        let chain = AwsCredentialChain::new(&config);
        let result = chain.try_explicit().unwrap();

        assert!(result.is_some());
        let creds = result.unwrap();
        assert_eq!(creds.access_key_id, "AKIATEST");
        assert_eq!(creds.secret_access_key, "secrettest");
        assert_eq!(creds.source, CredentialSource::Explicit);
    }

    #[test]
    fn test_try_explicit_with_alternate_keys() {
        let config = json!({
            "access_key_id": "AKIATEST",
            "secret_access_key": "secrettest",
            "session_token": "tokentest"
        });

        let chain = AwsCredentialChain::new(&config);
        let result = chain.try_explicit().unwrap();

        assert!(result.is_some());
        let creds = result.unwrap();
        assert_eq!(creds.session_token, Some("tokentest".to_string()));
    }

    #[test]
    fn test_try_explicit_incomplete() {
        let config = json!({
            "access_key": "AKIATEST"
        });

        let chain = AwsCredentialChain::new(&config);
        let result = chain.try_explicit();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProvisioningError::AuthenticationError { .. }));
    }

    #[test]
    fn test_try_explicit_empty() {
        let config = json!({});

        let chain = AwsCredentialChain::new(&config);
        let result = chain.try_explicit().unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_try_explicit_empty_values() {
        let config = json!({
            "access_key": "",
            "secret_key": ""
        });

        let chain = AwsCredentialChain::new(&config);
        let result = chain.try_explicit();

        // Empty credentials are treated as an error (incomplete credentials)
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProvisioningError::AuthenticationError { .. }));
    }

    #[test]
    fn test_parse_credentials_file() {
        let content = r#"
[default]
aws_access_key_id = AKIADEFAULT
aws_secret_access_key = secretdefault

[production]
aws_access_key_id = AKIAPROD
aws_secret_access_key = secretprod
aws_session_token = tokenprod

# Comment line
[staging]
access_key_id = AKIASTAGING
secret_access_key = secretstaging
"#;

        let chain = AwsCredentialChain::new(&json!({}));

        // Test default profile
        let result = chain.parse_credentials_file(content, "default").unwrap();
        assert!(result.is_some());
        let creds = result.unwrap();
        assert_eq!(creds.access_key_id, "AKIADEFAULT");
        assert_eq!(creds.secret_access_key, "secretdefault");
        assert!(creds.session_token.is_none());

        // Test production profile with session token
        let result = chain.parse_credentials_file(content, "production").unwrap();
        assert!(result.is_some());
        let creds = result.unwrap();
        assert_eq!(creds.access_key_id, "AKIAPROD");
        assert_eq!(creds.session_token, Some("tokenprod".to_string()));
        assert!(matches!(
            creds.source,
            CredentialSource::SharedCredentials { profile } if profile == "production"
        ));

        // Test staging profile with alternate key names
        let result = chain.parse_credentials_file(content, "staging").unwrap();
        assert!(result.is_some());
        let creds = result.unwrap();
        assert_eq!(creds.access_key_id, "AKIASTAGING");

        // Test non-existent profile
        let result = chain
            .parse_credentials_file(content, "nonexistent")
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_chain_profile_from_config() {
        let config = json!({
            "profile": "myprofile"
        });

        let chain = AwsCredentialChain::new(&config);
        assert_eq!(chain.profile, Some("myprofile".to_string()));
    }

    #[test]
    fn test_chain_use_imds_config() {
        // Default is true
        let chain = AwsCredentialChain::new(&json!({}));
        assert!(chain.use_imds);

        // Can be disabled
        let chain = AwsCredentialChain::new(&json!({"use_imds": false}));
        assert!(!chain.use_imds);
    }

    #[test]
    fn test_aws_credentials_serialization() {
        let creds = AwsCredentials::new("AKIATEST", "secret123", CredentialSource::Explicit);
        let json = serde_json::to_value(&creds).unwrap();

        // Access key should be serialized
        assert_eq!(
            json.get("access_key_id").and_then(|v| v.as_str()),
            Some("AKIATEST")
        );
        // Secret key should NOT be serialized (skip_serializing)
        assert!(json.get("secret_access_key").is_none());
        // Session token should NOT be serialized
        assert!(json.get("session_token").is_none());
    }

    #[test]
    fn test_time_until_expiration() {
        let creds = AwsCredentials::new("AKIATEST", "secret", CredentialSource::InstanceMetadata)
            .with_expiration(Utc::now() + chrono::Duration::hours(1));

        let time_left = creds.time_until_expiration();
        assert!(time_left.is_some());
        let duration = time_left.unwrap();
        // Should be close to 1 hour (within a few seconds tolerance)
        assert!(duration.num_minutes() >= 59);
        assert!(duration.num_minutes() <= 60);
    }

    #[tokio::test]
    async fn test_resolve_credentials_explicit() {
        let config = json!({
            "access_key": "AKIATEST",
            "secret_key": "secrettest"
        });

        let result = resolve_credentials(&config).await;
        assert!(result.is_ok());

        let creds = result.unwrap();
        assert_eq!(creds.access_key_id, "AKIATEST");
        assert_eq!(creds.source, CredentialSource::Explicit);
    }

    #[tokio::test]
    async fn test_resolve_credentials_no_credentials() {
        // Clear environment variables that might interfere
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        std::env::remove_var("AWS_PROFILE");
        // Use a non-existent credentials file path
        let config = json!({
            "credentials_file": "/nonexistent/path/credentials",
            "use_imds": false
        });

        let result = resolve_credentials(&config).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ProvisioningError::AuthenticationError { .. }));
    }
}
