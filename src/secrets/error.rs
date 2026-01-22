//! Error types for secret management.

use thiserror::Error;

/// Result type for secret operations.
pub type SecretResult<T> = std::result::Result<T, SecretError>;

/// Errors that can occur during secret operations.
#[derive(Error, Debug)]
pub enum SecretError {
    /// Secret not found at the specified path.
    #[error("Secret not found: {0}")]
    NotFound(String),

    /// Key not found within a secret.
    #[error("Key not found in secret: {0}")]
    KeyNotFound(String),

    /// Type mismatch when accessing a secret value.
    #[error("Type mismatch for key '{key}': expected {expected}")]
    TypeMismatch {
        /// The key that had the wrong type
        key: String,
        /// The expected type
        expected: String,
    },

    /// Authentication failed with the secret backend.
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization failed - insufficient permissions.
    #[error("Authorization failed: {0}")]
    Authorization(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Connection error to the secret backend.
    #[error("Connection error: {0}")]
    Connection(String),

    /// The secret backend returned an error.
    #[error("Backend error: {message}")]
    Backend {
        /// Error message from the backend
        message: String,
        /// HTTP status code if applicable
        status_code: Option<u16>,
    },

    /// Secret rotation failed.
    #[error("Rotation failed for secret '{path}': {message}")]
    Rotation {
        /// The secret path
        path: String,
        /// Error message
        message: String,
    },

    /// Cache error.
    #[error("Cache error: {0}")]
    Cache(String),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid secret format.
    #[error("Invalid secret format: {0}")]
    InvalidFormat(String),

    /// Secret version conflict.
    #[error("Version conflict: expected {expected}, found {found}")]
    VersionConflict {
        /// Expected version
        expected: String,
        /// Found version
        found: String,
    },

    /// Rate limit exceeded.
    #[error("Rate limit exceeded: {0}")]
    RateLimited(String),

    /// Timeout error.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Secret is sealed or unavailable.
    #[error("Secret backend is sealed or unavailable: {0}")]
    Sealed(String),

    /// Network error.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic error with context.
    #[error("{message}")]
    Other {
        /// Error message
        message: String,
        /// Source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl SecretError {
    /// Create a new backend error with status code.
    pub fn backend(message: impl Into<String>, status_code: Option<u16>) -> Self {
        Self::Backend {
            message: message.into(),
            status_code,
        }
    }

    /// Create a new rotation error.
    pub fn rotation(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Rotation {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create an error with additional context.
    pub fn with_context(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Other {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SecretError::Connection(_)
                | SecretError::Timeout(_)
                | SecretError::RateLimited(_)
                | SecretError::Network(_)
        )
    }

    /// Check if this is an authentication error.
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            SecretError::Authentication(_) | SecretError::Authorization(_)
        )
    }

    /// Check if this is a not found error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, SecretError::NotFound(_) | SecretError::KeyNotFound(_))
    }

    /// Get the HTTP status code if available.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            SecretError::Backend { status_code, .. } => *status_code,
            SecretError::NotFound(_) | SecretError::KeyNotFound(_) => Some(404),
            SecretError::Authentication(_) => Some(401),
            SecretError::Authorization(_) => Some(403),
            SecretError::RateLimited(_) => Some(429),
            SecretError::Network(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
}

/// Extension trait for adding secret context to results.
pub trait SecretResultExt<T> {
    /// Add context to a secret error.
    fn secret_context(self, context: impl Into<String>) -> SecretResult<T>;
}

impl<T, E> SecretResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn secret_context(self, context: impl Into<String>) -> SecretResult<T> {
        self.map_err(|e| SecretError::with_context(context, e))
    }
}

/// Error hints for common secret management issues.
pub struct SecretErrorHints;

impl SecretErrorHints {
    /// Get hints for a specific error.
    pub fn get_hints(error: &SecretError) -> Vec<String> {
        match error {
            SecretError::Authentication(_) => vec![
                "Check your authentication credentials (token, AppRole, etc.)".to_string(),
                "Verify the token has not expired".to_string(),
                "Ensure environment variables are set correctly".to_string(),
                "For Vault: Check VAULT_TOKEN or VAULT_ROLE_ID/VAULT_SECRET_ID".to_string(),
                "For AWS: Check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY".to_string(),
            ],
            SecretError::Authorization(_) => vec![
                "Check that your credentials have the necessary permissions".to_string(),
                "Verify the policy attached to your token/role".to_string(),
                "For Vault: Use 'vault token capabilities <path>' to check permissions".to_string(),
                "For AWS: Check IAM policies for secretsmanager:GetSecretValue".to_string(),
            ],
            SecretError::NotFound(_) => vec![
                "Verify the secret path is correct".to_string(),
                "Check if the secret exists in the backend".to_string(),
                "For Vault KV v2: Ensure path includes 'data/' segment".to_string(),
                "For AWS: Try using the full ARN instead of the name".to_string(),
            ],
            SecretError::Connection(_) | SecretError::Network(_) => vec![
                "Check network connectivity to the secret backend".to_string(),
                "Verify the backend URL is correct".to_string(),
                "Check for firewall rules blocking the connection".to_string(),
                "For Vault: Verify VAULT_ADDR is set correctly".to_string(),
            ],
            SecretError::Sealed(_) => vec![
                "The Vault server needs to be unsealed".to_string(),
                "Contact your Vault administrator".to_string(),
                "Check Vault status: vault status".to_string(),
            ],
            SecretError::RateLimited(_) => vec![
                "Implement exponential backoff in your retry logic".to_string(),
                "Consider caching secrets locally".to_string(),
                "Reduce the frequency of secret access".to_string(),
            ],
            SecretError::Timeout(_) => vec![
                "Increase the timeout configuration".to_string(),
                "Check network latency to the backend".to_string(),
                "Consider using a closer backend instance".to_string(),
            ],
            _ => vec![
                "Check the secret backend logs for more details".to_string(),
                "Verify your configuration settings".to_string(),
            ],
        }
    }

    /// Format error with hints for display.
    pub fn format_with_hints(error: &SecretError) -> String {
        let hints = Self::get_hints(error);
        let mut output = format!("Error: {}", error);

        if !hints.is_empty() {
            output.push_str("\n\nHints:");
            for hint in hints {
                output.push_str(&format!("\n  - {}", hint));
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = SecretError::NotFound("secret/myapp".to_string());
        assert!(err.to_string().contains("secret/myapp"));
    }

    #[test]
    fn test_error_retryable() {
        assert!(SecretError::Connection("timeout".to_string()).is_retryable());
        assert!(SecretError::Timeout("10s".to_string()).is_retryable());
        assert!(SecretError::RateLimited("too many requests".to_string()).is_retryable());
        assert!(!SecretError::NotFound("path".to_string()).is_retryable());
        assert!(!SecretError::Authentication("bad token".to_string()).is_retryable());
    }

    #[test]
    fn test_error_status_codes() {
        assert_eq!(
            SecretError::NotFound("x".to_string()).status_code(),
            Some(404)
        );
        assert_eq!(
            SecretError::Authentication("x".to_string()).status_code(),
            Some(401)
        );
        assert_eq!(
            SecretError::Authorization("x".to_string()).status_code(),
            Some(403)
        );
        assert_eq!(
            SecretError::RateLimited("x".to_string()).status_code(),
            Some(429)
        );
    }

    #[test]
    fn test_error_hints() {
        let err = SecretError::NotFound("secret/myapp".to_string());
        let hints = SecretErrorHints::get_hints(&err);
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|h| h.contains("path")));
    }

    #[test]
    fn test_backend_error() {
        let err = SecretError::backend("internal server error", Some(500));
        assert!(err.to_string().contains("internal server error"));
        assert_eq!(err.status_code(), Some(500));
    }
}
