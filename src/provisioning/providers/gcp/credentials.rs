//! GCP Credentials
//!
//! This module defines credential types for Google Cloud Platform authentication.
//! Currently a stub implementation for the experimental GCP provider.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provisioning::traits::ProviderCredentials;

// ============================================================================
// Authentication Methods
// ============================================================================

/// GCP authentication method
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcpAuthMethod {
    /// Service account JSON key file
    ServiceAccountKey,
    /// Application Default Credentials (ADC)
    ApplicationDefault,
    /// gcloud CLI authentication
    GcloudCli,
}

impl std::fmt::Display for GcpAuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServiceAccountKey => write!(f, "service_account_key"),
            Self::ApplicationDefault => write!(f, "application_default"),
            Self::GcloudCli => write!(f, "gcloud_cli"),
        }
    }
}

// ============================================================================
// GCP Credentials
// ============================================================================

/// GCP credentials for provider authentication
///
/// Supports service account key, Application Default Credentials, and gcloud
/// CLI authentication. Currently a stub -- `from_env()` defines the interface
/// but does not actually read environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcpCredentials {
    /// GCP project ID
    pub project_id: Option<String>,
    /// Service account key JSON content (sensitive)
    #[serde(skip_serializing)]
    pub service_account_key: Option<String>,
    /// Authentication method in use
    pub auth_method: GcpAuthMethod,
}

impl GcpCredentials {
    /// Create new credentials with explicit values
    pub fn new(project_id: Option<String>, service_account_key: Option<String>) -> Self {
        Self {
            project_id,
            service_account_key,
            auth_method: GcpAuthMethod::ServiceAccountKey,
        }
    }

    /// Create credentials configured for Application Default Credentials
    pub fn application_default() -> Self {
        Self {
            project_id: None,
            service_account_key: None,
            auth_method: GcpAuthMethod::ApplicationDefault,
        }
    }

    /// Create credentials configured for gcloud CLI authentication
    pub fn gcloud_cli() -> Self {
        Self {
            project_id: None,
            service_account_key: None,
            auth_method: GcpAuthMethod::GcloudCli,
        }
    }

    /// Define the interface for loading credentials from environment variables.
    ///
    /// Would read:
    /// - `GOOGLE_PROJECT` or `GCLOUD_PROJECT` for project ID
    /// - `GOOGLE_APPLICATION_CREDENTIALS` for service account key path
    ///
    /// This is a stub that returns a default credential structure without
    /// actually reading environment variables.
    pub fn from_env() -> Self {
        Self {
            project_id: None,
            service_account_key: None,
            auth_method: GcpAuthMethod::ApplicationDefault,
        }
    }

    /// Set the authentication method
    pub fn with_auth_method(mut self, method: GcpAuthMethod) -> Self {
        self.auth_method = method;
        self
    }

    /// Set the project ID
    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }
}

impl ProviderCredentials for GcpCredentials {
    fn credential_type(&self) -> &str {
        "gcp"
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "gcp",
            "auth_method": self.auth_method,
            "project_id": self.project_id,
            "has_service_account_key": self.service_account_key.is_some(),
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
    fn test_gcp_auth_method_display() {
        assert_eq!(
            GcpAuthMethod::ServiceAccountKey.to_string(),
            "service_account_key"
        );
        assert_eq!(
            GcpAuthMethod::ApplicationDefault.to_string(),
            "application_default"
        );
        assert_eq!(GcpAuthMethod::GcloudCli.to_string(), "gcloud_cli");
    }

    #[test]
    fn test_gcp_credentials_new() {
        let creds = GcpCredentials::new(
            Some("my-project".to_string()),
            Some("{\"type\": \"service_account\"}".to_string()),
        );

        assert_eq!(creds.project_id, Some("my-project".to_string()));
        assert!(creds.service_account_key.is_some());
        assert_eq!(creds.auth_method, GcpAuthMethod::ServiceAccountKey);
    }

    #[test]
    fn test_gcp_credentials_application_default() {
        let creds = GcpCredentials::application_default();
        assert_eq!(creds.auth_method, GcpAuthMethod::ApplicationDefault);
        assert!(creds.project_id.is_none());
        assert!(creds.service_account_key.is_none());
    }

    #[test]
    fn test_gcp_credentials_gcloud_cli() {
        let creds = GcpCredentials::gcloud_cli();
        assert_eq!(creds.auth_method, GcpAuthMethod::GcloudCli);
    }

    #[test]
    fn test_gcp_credentials_from_env() {
        let creds = GcpCredentials::from_env();
        assert_eq!(creds.auth_method, GcpAuthMethod::ApplicationDefault);
    }

    #[test]
    fn test_gcp_credentials_with_auth_method() {
        let creds = GcpCredentials::new(None, None)
            .with_auth_method(GcpAuthMethod::GcloudCli);
        assert_eq!(creds.auth_method, GcpAuthMethod::GcloudCli);
    }

    #[test]
    fn test_gcp_credentials_with_project_id() {
        let creds = GcpCredentials::application_default().with_project_id("my-project");
        assert_eq!(creds.project_id, Some("my-project".to_string()));
    }

    #[test]
    fn test_provider_credentials_trait() {
        let creds = GcpCredentials::new(
            Some("my-project".to_string()),
            Some("key-data".to_string()),
        );

        assert_eq!(creds.credential_type(), "gcp");
        assert!(!creds.is_expired());

        let value = creds.as_value();
        assert_eq!(value.get("type").and_then(|v| v.as_str()), Some("gcp"));
        assert_eq!(
            value.get("project_id").and_then(|v| v.as_str()),
            Some("my-project")
        );
        assert_eq!(
            value
                .get("has_service_account_key")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_credentials_serialization_hides_key() {
        let creds = GcpCredentials::new(
            Some("my-project".to_string()),
            Some("secret-key-data".to_string()),
        );

        let json = serde_json::to_value(&creds).unwrap();
        // service_account_key should be skipped during serialization
        assert!(json.get("service_account_key").is_none());
        // project_id should be present
        assert_eq!(
            json.get("project_id").and_then(|v| v.as_str()),
            Some("my-project")
        );
    }
}
