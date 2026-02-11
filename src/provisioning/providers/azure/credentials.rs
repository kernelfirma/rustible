//! Azure Credentials
//!
//! This module defines credential types for Azure authentication.
//! Currently a stub implementation for the experimental Azure provider.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provisioning::traits::ProviderCredentials;

// ============================================================================
// Authentication Methods
// ============================================================================

/// Azure authentication method
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AzureAuthMethod {
    /// Service principal with client ID and secret
    ServicePrincipal,
    /// Managed identity (system-assigned or user-assigned)
    ManagedIdentity,
    /// Azure CLI authentication
    Cli,
}

impl std::fmt::Display for AzureAuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServicePrincipal => write!(f, "service_principal"),
            Self::ManagedIdentity => write!(f, "managed_identity"),
            Self::Cli => write!(f, "cli"),
        }
    }
}

// ============================================================================
// Azure Credentials
// ============================================================================

/// Azure credentials for provider authentication
///
/// Supports service principal, managed identity, and CLI authentication.
/// Currently a stub -- `from_env()` defines the interface but does not
/// actually read environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureCredentials {
    /// Azure Active Directory tenant ID
    pub tenant_id: Option<String>,
    /// Application (client) ID for service principal auth
    pub client_id: Option<String>,
    /// Client secret for service principal auth (sensitive)
    #[serde(skip_serializing)]
    pub client_secret: Option<String>,
    /// Authentication method in use
    pub auth_method: AzureAuthMethod,
}

impl AzureCredentials {
    /// Create new credentials with explicit values
    pub fn new(
        tenant_id: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
    ) -> Self {
        Self {
            tenant_id,
            client_id,
            client_secret,
            auth_method: AzureAuthMethod::ServicePrincipal,
        }
    }

    /// Create credentials configured for CLI authentication
    pub fn cli() -> Self {
        Self {
            tenant_id: None,
            client_id: None,
            client_secret: None,
            auth_method: AzureAuthMethod::Cli,
        }
    }

    /// Create credentials configured for managed identity
    pub fn managed_identity() -> Self {
        Self {
            tenant_id: None,
            client_id: None,
            client_secret: None,
            auth_method: AzureAuthMethod::ManagedIdentity,
        }
    }

    /// Define the interface for loading credentials from environment variables.
    ///
    /// Would read:
    /// - `AZURE_TENANT_ID`
    /// - `AZURE_CLIENT_ID`
    /// - `AZURE_CLIENT_SECRET`
    ///
    /// This is a stub that returns a default credential structure without
    /// actually reading environment variables.
    pub fn from_env() -> Self {
        Self {
            tenant_id: None,
            client_id: None,
            client_secret: None,
            auth_method: AzureAuthMethod::Cli,
        }
    }

    /// Set the authentication method
    pub fn with_auth_method(mut self, method: AzureAuthMethod) -> Self {
        self.auth_method = method;
        self
    }
}

impl ProviderCredentials for AzureCredentials {
    fn credential_type(&self) -> &str {
        "azure"
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "azure",
            "auth_method": self.auth_method,
            "tenant_id": self.tenant_id,
            "has_client_id": self.client_id.is_some(),
            "has_client_secret": self.client_secret.is_some(),
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_auth_method_display() {
        assert_eq!(AzureAuthMethod::ServicePrincipal.to_string(), "service_principal");
        assert_eq!(AzureAuthMethod::ManagedIdentity.to_string(), "managed_identity");
        assert_eq!(AzureAuthMethod::Cli.to_string(), "cli");
    }

    #[test]
    fn test_azure_credentials_new() {
        let creds = AzureCredentials::new(
            Some("tenant-123".to_string()),
            Some("client-456".to_string()),
            Some("secret-789".to_string()),
        );

        assert_eq!(creds.tenant_id, Some("tenant-123".to_string()));
        assert_eq!(creds.client_id, Some("client-456".to_string()));
        assert_eq!(creds.client_secret, Some("secret-789".to_string()));
        assert_eq!(creds.auth_method, AzureAuthMethod::ServicePrincipal);
    }

    #[test]
    fn test_azure_credentials_cli() {
        let creds = AzureCredentials::cli();
        assert_eq!(creds.auth_method, AzureAuthMethod::Cli);
        assert!(creds.tenant_id.is_none());
        assert!(creds.client_id.is_none());
        assert!(creds.client_secret.is_none());
    }

    #[test]
    fn test_azure_credentials_managed_identity() {
        let creds = AzureCredentials::managed_identity();
        assert_eq!(creds.auth_method, AzureAuthMethod::ManagedIdentity);
    }

    #[test]
    fn test_azure_credentials_from_env() {
        let creds = AzureCredentials::from_env();
        assert_eq!(creds.auth_method, AzureAuthMethod::Cli);
    }

    #[test]
    fn test_azure_credentials_with_auth_method() {
        let creds = AzureCredentials::new(None, None, None)
            .with_auth_method(AzureAuthMethod::ManagedIdentity);
        assert_eq!(creds.auth_method, AzureAuthMethod::ManagedIdentity);
    }

    #[test]
    fn test_provider_credentials_trait() {
        let creds = AzureCredentials::new(
            Some("tenant-123".to_string()),
            Some("client-456".to_string()),
            Some("secret-789".to_string()),
        );

        assert_eq!(creds.credential_type(), "azure");
        assert!(!creds.is_expired());

        let value = creds.as_value();
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("azure"));
        assert_eq!(
            value.get("has_client_id").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            value.get("has_client_secret").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_credentials_serialization_hides_secret() {
        let creds = AzureCredentials::new(
            Some("tenant-123".to_string()),
            Some("client-456".to_string()),
            Some("secret-789".to_string()),
        );

        let json = serde_json::to_value(&creds).unwrap();
        // client_secret should be skipped during serialization
        assert!(json.get("client_secret").is_none());
        // tenant_id should be present
        assert_eq!(
            json.get("tenant_id").and_then(|v| v.as_str()),
            Some("tenant-123")
        );
    }
}
