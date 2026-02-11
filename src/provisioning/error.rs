//! Provisioning error types
//!
//! This module defines all error types specific to infrastructure provisioning.

use std::fmt;

use thiserror::Error;

/// Result type for provisioning operations
pub type ProvisioningResult<T> = Result<T, ProvisioningError>;

/// Errors that can occur during infrastructure provisioning
#[derive(Error, Debug)]
pub enum ProvisioningError {
    /// Provider not found in registry
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    /// Resource type not found
    #[error("Resource type not found: {provider}/{resource_type}")]
    ResourceNotFound {
        provider: String,
        resource_type: String,
    },

    /// Resource already exists
    #[error("Resource already exists: {0}")]
    ResourceExists(String),

    /// Resource not in state
    #[error("Resource not in state: {0}")]
    ResourceNotInState(String),

    /// Provider configuration error
    #[error("Provider configuration error for {provider}: {message}")]
    ProviderConfigError { provider: String, message: String },

    /// Cloud API error
    #[error("Cloud API error: {0}")]
    CloudApiError(String),

    /// Authentication error
    #[error("Authentication error for provider {provider}: {message}")]
    AuthenticationError { provider: String, message: String },

    /// Resource validation error
    #[error("Resource validation error: {0}")]
    ValidationError(String),

    /// Dependency cycle detected
    #[error("Dependency cycle detected: {0:?}")]
    DependencyCycle(Vec<String>),

    /// Missing dependency
    #[error("Missing dependency: {resource} depends on {dependency} which does not exist")]
    MissingDependency {
        resource: String,
        dependency: String,
    },

    /// State persistence error
    #[error("State persistence error: {0}")]
    StatePersistenceError(String),

    /// State corruption detected
    #[error("State corruption detected: {0}")]
    StateCorruption(String),

    /// Import error
    #[error("Failed to import resource {resource_type}/{resource_id}: {message}")]
    ImportError {
        resource_type: String,
        resource_id: String,
        message: String,
    },

    /// Refresh error
    #[error("Failed to refresh resource {resource_id}: {message}")]
    RefreshError {
        resource_id: String,
        message: String,
    },

    /// Plan execution error
    #[error("Plan execution error: {0}")]
    PlanError(String),

    /// Apply error
    #[error("Apply error for {resource}: {message}")]
    ApplyError { resource: String, message: String },

    /// Destroy error
    #[error("Destroy error for {resource}: {message}")]
    DestroyError { resource: String, message: String },

    /// Configuration parsing error
    #[error("Configuration parsing error: {0}")]
    ConfigError(String),

    /// Template rendering error
    #[error("Template rendering error: {0}")]
    TemplateError(String),

    /// Cross-reference resolution error
    #[error("Cannot resolve reference: {reference}")]
    UnresolvedReference { reference: String },

    /// Timeout error
    #[error("Operation timed out after {seconds} seconds: {operation}")]
    Timeout { operation: String, seconds: u64 },

    /// Concurrency error
    #[error("Concurrency error: {0}")]
    ConcurrencyError(String),

    /// Generic IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Lifecycle prevent_destroy violation
    #[error("Cannot destroy resource {resource}: prevent_destroy is set")]
    PreventDestroyViolation { resource: String },

    /// Blast radius exceeded
    #[error("Blast radius exceeded: {message}")]
    BlastRadiusExceeded { message: String },

    /// State move failed
    #[error("State move failed: {message}")]
    StateMoveFailed { message: String },
}

impl ProvisioningError {
    /// Create a provider configuration error
    pub fn provider_config(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ProviderConfigError {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// Create an authentication error
    pub fn auth(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::AuthenticationError {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// Create a resource not found error
    pub fn resource_not_found(
        provider: impl Into<String>,
        resource_type: impl Into<String>,
    ) -> Self {
        Self::ResourceNotFound {
            provider: provider.into(),
            resource_type: resource_type.into(),
        }
    }

    /// Create an apply error
    pub fn apply(resource: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ApplyError {
            resource: resource.into(),
            message: message.into(),
        }
    }

    /// Create a destroy error
    pub fn destroy(resource: impl Into<String>, message: impl Into<String>) -> Self {
        Self::DestroyError {
            resource: resource.into(),
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::CloudApiError(_)
                | Self::Timeout { .. }
                | Self::ConcurrencyError(_)
                | Self::RefreshError { .. }
        )
    }

    /// Check if this error requires user intervention
    pub fn requires_intervention(&self) -> bool {
        matches!(
            self,
            Self::AuthenticationError { .. }
                | Self::StateCorruption(_)
                | Self::DependencyCycle(_)
                | Self::ValidationError(_)
        )
    }
}

impl From<serde_json::Error> for ProvisioningError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err.to_string())
    }
}

impl From<serde_yaml::Error> for ProvisioningError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::ConfigError(err.to_string())
    }
}

/// Extended error information for detailed diagnostics
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// The primary error
    pub error: String,
    /// Resource affected (if any)
    pub resource: Option<String>,
    /// Provider involved (if any)
    pub provider: Option<String>,
    /// Suggested remediation steps
    pub remediation: Vec<String>,
    /// Related documentation links
    pub docs: Vec<String>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            resource: None,
            provider: None,
            remediation: Vec::new(),
            docs: Vec::new(),
        }
    }

    /// Add resource context
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Add provider context
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Add a remediation step
    pub fn with_remediation(mut self, step: impl Into<String>) -> Self {
        self.remediation.push(step.into());
        self
    }

    /// Add a documentation link
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.docs.push(doc.into());
        self
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;

        if let Some(ref resource) = self.resource {
            write!(f, "\n  Resource: {}", resource)?;
        }

        if let Some(ref provider) = self.provider {
            write!(f, "\n  Provider: {}", provider)?;
        }

        if !self.remediation.is_empty() {
            write!(f, "\n\n  Suggested fixes:")?;
            for (i, step) in self.remediation.iter().enumerate() {
                write!(f, "\n    {}. {}", i + 1, step)?;
            }
        }

        if !self.docs.is_empty() {
            write!(f, "\n\n  Documentation:")?;
            for doc in &self.docs {
                write!(f, "\n    - {}", doc)?;
            }
        }

        Ok(())
    }
}
