//! Resource and Provider Registry
//!
//! This module provides a central registry for managing providers and resources.
//! It handles provider initialization, resource type lookup, and dependency injection.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info};

use super::error::{ProvisioningError, ProvisioningResult};
use super::traits::{Provider, ProviderConfig, Resource};

// ============================================================================
// Provider Registry
// ============================================================================

/// Factory function for creating providers
pub type ProviderFactory = Box<dyn Fn() -> Box<dyn Provider> + Send + Sync>;

/// Registry for infrastructure providers
#[derive(Default)]
pub struct ProviderRegistry {
    /// Registered provider factories
    factories: HashMap<String, ProviderFactory>,

    /// Initialized provider instances
    instances: RwLock<HashMap<String, Arc<RwLock<Box<dyn Provider>>>>>,
}

impl ProviderRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with built-in providers
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // AWS provider (feature-gated)
        #[cfg(feature = "aws")]
        {
            // registry.register_factory("aws", || Box::new(super::providers::aws::AwsProvider::new()));
        }

        // Azure provider (experimental)
        #[cfg(all(feature = "azure", feature = "experimental"))]
        {
            registry.register_factory("azure", || {
                Box::new(super::providers::azure::AzureProvider::new())
            });
        }

        // GCP provider (experimental)
        #[cfg(all(feature = "gcp", feature = "experimental"))]
        {
            registry.register_factory(
                "gcp",
                || Box::new(super::providers::gcp::GcpProvider::new()),
            );
        }

        registry
    }

    /// Register a provider factory
    pub fn register_factory<F>(&mut self, name: impl Into<String>, factory: F)
    where
        F: Fn() -> Box<dyn Provider> + Send + Sync + 'static,
    {
        let name = name.into();
        debug!("Registering provider factory: {}", name);
        self.factories.insert(name, Box::new(factory));
    }

    /// Check if a provider is registered
    pub fn has_provider(&self, name: &str) -> bool {
        self.factories.contains_key(name) || self.instances.read().contains_key(name)
    }

    /// Get list of available providers
    pub fn available_providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = self.factories.keys().cloned().collect();
        providers.sort();
        providers
    }

    /// Initialize a provider with configuration
    pub async fn initialize_provider(&self, config: ProviderConfig) -> ProvisioningResult<()> {
        let name = config.name.clone();

        // Check if already initialized
        if self.instances.read().contains_key(&name) {
            debug!("Provider {} already initialized", name);
            return Ok(());
        }

        // Get factory
        let factory = self
            .factories
            .get(&name)
            .ok_or_else(|| ProvisioningError::ProviderNotFound(name.clone()))?;

        // Create and configure provider
        let mut provider = factory();
        provider.configure(config).await?;

        info!("Initialized provider: {}", name);

        // Store instance
        self.instances
            .write()
            .insert(name, Arc::new(RwLock::new(provider)));

        Ok(())
    }

    /// Get an initialized provider
    pub fn get_provider(&self, name: &str) -> ProvisioningResult<Arc<RwLock<Box<dyn Provider>>>> {
        self.instances
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| ProvisioningError::ProviderNotFound(name.to_string()))
    }

    /// Get a resource from a provider
    pub fn get_resource(
        &self,
        provider: &str,
        resource_type: &str,
    ) -> ProvisioningResult<Arc<dyn Resource>> {
        let provider_lock = self.get_provider(provider)?;
        let provider_guard = provider_lock.read();
        provider_guard.resource(resource_type)
    }

    /// Clear all initialized providers
    pub fn clear_instances(&self) {
        self.instances.write().clear();
    }
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("factories", &self.factories.keys().collect::<Vec<_>>())
            .field(
                "instances",
                &self.instances.read().keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ============================================================================
// Resource Registry
// ============================================================================

/// Registry for resource types
#[derive(Default)]
pub struct ResourceRegistry {
    /// Provider registry
    providers: Arc<ProviderRegistry>,

    /// Cached resource schemas
    schemas: RwLock<HashMap<String, super::traits::ResourceSchema>>,
}

impl ResourceRegistry {
    /// Create a new resource registry
    pub fn new(providers: Arc<ProviderRegistry>) -> Self {
        Self {
            providers,
            schemas: RwLock::default(),
        }
    }

    /// Get a resource by full type (e.g., "aws_vpc")
    pub fn get(&self, resource_type: &str) -> ProvisioningResult<Arc<dyn Resource>> {
        // Parse provider from resource type
        let (provider, _) = parse_resource_type(resource_type)?;

        self.providers.get_resource(&provider, resource_type)
    }

    /// Get the provider for a resource type
    pub fn provider_for(&self, resource_type: &str) -> ProvisioningResult<String> {
        let (provider, _) = parse_resource_type(resource_type)?;
        Ok(provider)
    }

    /// Check if a resource type is supported
    pub fn is_supported(&self, resource_type: &str) -> bool {
        self.get(resource_type).is_ok()
    }

    /// Get all supported resource types
    pub fn supported_types(&self) -> Vec<String> {
        let mut types = Vec::new();

        for provider_name in self.providers.available_providers() {
            if let Ok(provider_lock) = self.providers.get_provider(&provider_name) {
                let provider = provider_lock.read();
                types.extend(provider.resource_types());
            }
        }

        types.sort();
        types
    }

    /// Get schema for a resource type (cached)
    pub fn schema(&self, resource_type: &str) -> ProvisioningResult<super::traits::ResourceSchema> {
        // Check cache
        if let Some(schema) = self.schemas.read().get(resource_type) {
            return Ok(schema.clone());
        }

        // Get from resource
        let resource = self.get(resource_type)?;
        let schema = resource.schema();

        // Cache it
        self.schemas
            .write()
            .insert(resource_type.to_string(), schema.clone());

        Ok(schema)
    }

    /// Clear schema cache
    pub fn clear_cache(&self) {
        self.schemas.write().clear();
    }
}

impl std::fmt::Debug for ResourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceRegistry")
            .field("providers", &self.providers)
            .field(
                "cached_schemas",
                &self.schemas.read().keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse a resource type into provider and type
/// e.g., "aws_vpc" -> ("aws", "vpc")
/// e.g., "azurerm_virtual_network" -> ("azurerm", "virtual_network")
pub fn parse_resource_type(resource_type: &str) -> ProvisioningResult<(String, String)> {
    // Common provider prefixes
    let prefixes = [
        ("aws_", "aws"),
        ("azurerm_", "azure"),
        ("google_", "gcp"),
        ("digitalocean_", "digitalocean"),
        ("kubernetes_", "kubernetes"),
        ("local_", "local"),
        ("null_", "null"),
        ("random_", "random"),
        ("tls_", "tls"),
    ];

    for (prefix, provider) in prefixes {
        if resource_type.starts_with(prefix) {
            let type_part = resource_type.strip_prefix(prefix).unwrap_or(resource_type);
            return Ok((provider.to_string(), type_part.to_string()));
        }
    }

    // Fallback: split on first underscore
    if let Some(idx) = resource_type.find('_') {
        let (provider, rest) = resource_type.split_at(idx);
        let type_part = rest.strip_prefix('_').unwrap_or(rest);
        return Ok((provider.to_string(), type_part.to_string()));
    }

    Err(ProvisioningError::ValidationError(format!(
        "Cannot determine provider for resource type: {}",
        resource_type
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resource_type_aws() {
        let (provider, type_part) = parse_resource_type("aws_vpc").unwrap();
        assert_eq!(provider, "aws");
        assert_eq!(type_part, "vpc");

        let (provider, type_part) = parse_resource_type("aws_security_group").unwrap();
        assert_eq!(provider, "aws");
        assert_eq!(type_part, "security_group");
    }

    #[test]
    fn test_parse_resource_type_azure() {
        let (provider, type_part) = parse_resource_type("azurerm_virtual_network").unwrap();
        assert_eq!(provider, "azure");
        assert_eq!(type_part, "virtual_network");
    }

    #[test]
    fn test_parse_resource_type_gcp() {
        let (provider, type_part) = parse_resource_type("google_compute_instance").unwrap();
        assert_eq!(provider, "gcp");
        assert_eq!(type_part, "compute_instance");
    }

    #[test]
    fn test_parse_resource_type_fallback() {
        let (provider, type_part) = parse_resource_type("custom_resource").unwrap();
        assert_eq!(provider, "custom");
        assert_eq!(type_part, "resource");
    }

    #[test]
    fn test_provider_registry() {
        let registry = ProviderRegistry::new();
        assert!(registry.available_providers().is_empty());
        assert!(!registry.has_provider("aws"));
    }
}
