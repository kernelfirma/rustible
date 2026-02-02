//! Provider SDK for Rustible
//!
//! This module implements the Provider and Registry ecosystem, enabling cloud and platform
//! modules to be distributed, versioned, and upgraded independently of the core.
//!
//! # Overview
//!
//! Providers expose a manifest plus a dynamic module catalog. Each provider implements
//! the [`Provider`] trait which defines metadata, available modules, and an async
//! invocation interface.
//!
//! # Example
//!
//! ```rust,no_run
//! use rustible::plugins::provider::{
//!     Provider, ProviderMetadata, ModuleDescriptor, ProviderCapability,
//!     ProviderError, ModuleParams, ModuleOutput, ModuleContext,
//! };
//!
//! struct AwsProvider;
//!
//! #[async_trait::async_trait]
//! impl Provider for AwsProvider {
//!     fn metadata(&self) -> ProviderMetadata {
//!         ProviderMetadata {
//!             name: "aws".to_string(),
//!             version: semver::Version::new(1, 0, 0),
//!             api_version: semver::Version::new(1, 0, 0),
//!             supported_targets: vec!["aws".to_string()],
//!             capabilities: vec![ProviderCapability::Read, ProviderCapability::Create],
//!         }
//!     }
//!
//!     fn modules(&self) -> Vec<ModuleDescriptor> {
//!         vec![]
//!     }
//!
//!     async fn invoke(
//!         &self,
//!         module: &str,
//!         params: ModuleParams,
//!         ctx: ModuleContext,
//!     ) -> Result<ModuleOutput, ProviderError> {
//!         todo!()
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Parameters passed to a module invocation.
pub type ModuleParams = serde_json::Value;

/// Output returned from a module invocation.
pub type ModuleOutput = serde_json::Value;

/// Errors that can occur during provider operations.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The requested module was not found in this provider.
    #[error("module not found: {0}")]
    ModuleNotFound(String),

    /// Invalid parameters were passed to the module.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    /// The operation failed during execution.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// The provider does not have the required capability.
    #[error("capability not supported: {0:?}")]
    CapabilityNotSupported(ProviderCapability),

    /// Authentication or authorization failed.
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// A timeout occurred during the operation.
    #[error("operation timed out")]
    Timeout,

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization/deserialization error occurred.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Provider API version mismatch.
    #[error("API version mismatch: provider requires {required}, core provides {available}")]
    ApiVersionMismatch {
        required: semver::Version,
        available: semver::Version,
    },

    /// Generic error with custom message.
    #[error("{0}")]
    Other(String),
}

/// Capabilities that a provider can support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderCapability {
    /// Can read/query resources.
    Read,
    /// Can create new resources.
    Create,
    /// Can update existing resources.
    Update,
    /// Can delete resources.
    Delete,
}

/// Metadata describing a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    /// Unique name of the provider (e.g., "aws", "azure", "gcp").
    pub name: String,

    /// Version of this provider.
    pub version: semver::Version,

    /// API version this provider is compatible with.
    pub api_version: semver::Version,

    /// Target platforms this provider supports (e.g., ["aws"], ["onprem"]).
    pub supported_targets: Vec<String>,

    /// Capabilities this provider supports.
    pub capabilities: Vec<ProviderCapability>,
}

/// Parameter descriptor for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDescriptor {
    /// Name of the parameter.
    pub name: String,

    /// Description of what this parameter does.
    pub description: String,

    /// Whether this parameter is required.
    pub required: bool,

    /// JSON schema type (e.g., "string", "number", "boolean", "object", "array").
    pub param_type: String,

    /// Default value if not provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// Output descriptor for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDescriptor {
    /// Name of the output field.
    pub name: String,

    /// Description of this output.
    pub description: String,

    /// JSON schema type of the output.
    pub output_type: String,
}

/// Descriptor for a module exposed by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDescriptor {
    /// Name of the module (e.g., "ec2_instance", "s3_bucket").
    pub name: String,

    /// Human-readable description of what this module does.
    pub description: String,

    /// Parameters this module accepts.
    pub parameters: Vec<ParameterDescriptor>,

    /// Outputs this module produces.
    pub outputs: Vec<OutputDescriptor>,
}

/// Context provided to module invocations.
#[derive(Debug, Clone, Default)]
pub struct ModuleContext {
    /// Variables available from the playbook context.
    pub variables: HashMap<String, serde_json::Value>,

    /// Whether to run in check mode (dry-run).
    pub check_mode: bool,

    /// Whether to show diff output.
    pub diff_mode: bool,

    /// Verbosity level (0-4).
    pub verbosity: u8,

    /// Connection timeout in seconds.
    pub timeout: Option<u64>,

    /// Additional context-specific data.
    pub extra: HashMap<String, serde_json::Value>,
}

/// The core trait that all providers must implement.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Returns metadata about this provider.
    fn metadata(&self) -> ProviderMetadata;

    /// Returns descriptors for all modules this provider exposes.
    fn modules(&self) -> Vec<ModuleDescriptor>;

    /// Invokes a module with the given parameters and context.
    async fn invoke(
        &self,
        module: &str,
        params: ModuleParams,
        ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError>;
}

/// Registry for managing and discovering providers.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    /// Creates a new empty provider registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Registers a provider with the registry.
    pub fn register(&mut self, provider: Arc<dyn Provider>) -> Result<(), ProviderError> {
        let metadata = provider.metadata();
        let name = metadata.name.clone();

        if self.providers.contains_key(&name) {
            return Err(ProviderError::Other(format!(
                "provider '{}' is already registered",
                name
            )));
        }

        self.providers.insert(name, provider);
        Ok(())
    }

    /// Unregisters a provider from the registry.
    pub fn unregister(&mut self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.remove(name)
    }

    /// Gets a provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(name).cloned()
    }

    /// Lists all registered provider names.
    pub fn list(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Lists all registered providers with their metadata.
    pub fn list_with_metadata(&self) -> Vec<ProviderMetadata> {
        self.providers.values().map(|p| p.metadata()).collect()
    }

    /// Finds providers that support a specific target.
    pub fn find_by_target(&self, target: &str) -> Vec<Arc<dyn Provider>> {
        self.providers
            .values()
            .filter(|p| p.metadata().supported_targets.contains(&target.to_string()))
            .cloned()
            .collect()
    }

    /// Finds providers that have a specific capability.
    pub fn find_by_capability(&self, capability: ProviderCapability) -> Vec<Arc<dyn Provider>> {
        self.providers
            .values()
            .filter(|p| p.metadata().capabilities.contains(&capability))
            .cloned()
            .collect()
    }

    /// Invokes a module on a specific provider.
    pub async fn invoke(
        &self,
        provider_name: &str,
        module: &str,
        params: ModuleParams,
        ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError> {
        let provider = self
            .get(provider_name)
            .ok_or_else(|| ProviderError::Other(format!("provider '{}' not found", provider_name)))?;

        provider.invoke(module, params, ctx).await
    }
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A dependency requirement for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDependency {
    /// Name of the required provider.
    pub name: String,
    /// Semver version requirement string (e.g., ">=1.0.0", "^2.0").
    pub req: String,
    /// Whether this dependency is optional.
    #[serde(default)]
    pub optional: bool,
}

/// An index entry describing a published provider version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderIndexEntry {
    /// Provider name.
    pub name: String,
    /// Version string (semver).
    pub vers: String,
    /// Dependencies of this provider version.
    #[serde(default)]
    pub deps: Vec<ProviderDependency>,
    /// Checksum of the provider artifact.
    #[serde(default)]
    pub cksum: String,
    /// Feature flags exposed by this provider.
    #[serde(default)]
    pub features: HashMap<String, Vec<String>>,
    /// Whether this version has been yanked.
    #[serde(default)]
    pub yanked: bool,
    /// Minimum rustible version required.
    #[serde(default)]
    pub rustible_version: Option<String>,
    /// API version this provider targets.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Supported target platforms.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Capability tags for this provider version.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl ProviderIndexEntry {
    /// Parse the version string into a `semver::Version`.
    pub fn version(&self) -> Result<semver::Version, semver::Error> {
        self.vers.parse()
    }

    /// Create an index entry from provider metadata.
    pub fn from_metadata(metadata: &ProviderMetadata, checksum: &str) -> Self {
        Self {
            name: metadata.name.clone(),
            vers: metadata.version.to_string(),
            deps: vec![],
            cksum: checksum.to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: None,
            api_version: Some(metadata.api_version.to_string()),
            targets: metadata.supported_targets.clone(),
            capabilities: metadata
                .capabilities
                .iter()
                .map(|c| format!("{:?}", c).to_lowercase())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        metadata: ProviderMetadata,
    }

    impl MockProvider {
        fn new(name: &str) -> Self {
            Self {
                metadata: ProviderMetadata {
                    name: name.to_string(),
                    version: semver::Version::new(1, 0, 0),
                    api_version: semver::Version::new(1, 0, 0),
                    supported_targets: vec!["test".to_string()],
                    capabilities: vec![ProviderCapability::Read, ProviderCapability::Create],
                },
            }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn metadata(&self) -> ProviderMetadata {
            self.metadata.clone()
        }

        fn modules(&self) -> Vec<ModuleDescriptor> {
            vec![ModuleDescriptor {
                name: "test_module".to_string(),
                description: "A test module".to_string(),
                parameters: vec![],
                outputs: vec![],
            }]
        }

        async fn invoke(
            &self,
            module: &str,
            _params: ModuleParams,
            _ctx: ModuleContext,
        ) -> Result<ModuleOutput, ProviderError> {
            if module == "test_module" {
                Ok(serde_json::json!({"status": "ok"}))
            } else {
                Err(ProviderError::ModuleNotFound(module.to_string()))
            }
        }
    }

    #[test]
    fn test_provider_metadata() {
        let provider = MockProvider::new("test");
        let metadata = provider.metadata();
        assert_eq!(metadata.name, "test");
        assert_eq!(metadata.version, semver::Version::new(1, 0, 0));
    }

    #[test]
    fn test_provider_modules() {
        let provider = MockProvider::new("test");
        let modules = provider.modules();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "test_module");
    }

    #[test]
    fn test_registry_register() {
        let mut registry = ProviderRegistry::new();
        let provider = Arc::new(MockProvider::new("test"));
        assert!(registry.register(provider).is_ok());
        assert!(registry.get("test").is_some());
    }

    #[test]
    fn test_registry_duplicate_register() {
        let mut registry = ProviderRegistry::new();
        let provider1 = Arc::new(MockProvider::new("test"));
        let provider2 = Arc::new(MockProvider::new("test"));
        assert!(registry.register(provider1).is_ok());
        assert!(registry.register(provider2).is_err());
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = ProviderRegistry::new();
        let provider = Arc::new(MockProvider::new("test"));
        registry.register(provider).unwrap();
        assert!(registry.unregister("test").is_some());
        assert!(registry.get("test").is_none());
    }

    #[test]
    fn test_registry_list() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::new("aws"))).unwrap();
        registry.register(Arc::new(MockProvider::new("azure"))).unwrap();
        let names = registry.list();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"aws".to_string()));
        assert!(names.contains(&"azure".to_string()));
    }

    #[test]
    fn test_registry_find_by_capability() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::new("test"))).unwrap();
        let providers = registry.find_by_capability(ProviderCapability::Read);
        assert_eq!(providers.len(), 1);
    }

    #[tokio::test]
    async fn test_registry_invoke() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::new("test"))).unwrap();

        let result = registry
            .invoke("test", "test_module", serde_json::json!({}), ModuleContext::default())
            .await;
        assert!(result.is_ok());

        let result = registry
            .invoke("test", "unknown", serde_json::json!({}), ModuleContext::default())
            .await;
        assert!(matches!(result, Err(ProviderError::ModuleNotFound(_))));
    }

    #[tokio::test]
    async fn test_registry_invoke_unknown_provider() {
        let registry = ProviderRegistry::new();
        let result = registry
            .invoke("unknown", "module", serde_json::json!({}), ModuleContext::default())
            .await;
        assert!(matches!(result, Err(ProviderError::Other(_))));
    }
}
