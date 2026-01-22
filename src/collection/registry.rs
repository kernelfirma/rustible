//! Collection registry for managing loaded collections
//!
//! Provides a central registry for looking up and resolving collection content.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::loader::{CollectionLoader, CollectionSearchPath};
use super::{Collection, CollectionError, CollectionResult, Fqcn};

/// Builder for CollectionRegistry
pub struct CollectionRegistryBuilder {
    search_paths: Vec<CollectionSearchPath>,
    default_collection: Option<String>,
    preload: bool,
}

impl CollectionRegistryBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            default_collection: None,
            preload: false,
        }
    }

    /// Add a search path
    pub fn with_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_paths
            .push(CollectionSearchPath::new(path.into()));
        self
    }

    /// Add a search path with priority
    pub fn with_search_path_priority(mut self, path: impl Into<PathBuf>, priority: u32) -> Self {
        self.search_paths
            .push(CollectionSearchPath::new(path.into()).with_priority(priority));
        self
    }

    /// Set the default collection for unqualified names
    pub fn with_default_collection(mut self, collection: impl Into<String>) -> Self {
        self.default_collection = Some(collection.into());
        self
    }

    /// Preload all collections on build
    pub fn preload(mut self, preload: bool) -> Self {
        self.preload = preload;
        self
    }

    /// Build the registry
    pub async fn build(self) -> CollectionResult<CollectionRegistry> {
        let mut loader = CollectionLoader::new();

        // Add custom paths first (higher priority)
        for path in self.search_paths {
            loader.add_search_path(path);
        }

        // Add default paths
        loader = loader.with_default_paths();

        let registry = CollectionRegistry {
            loader: Arc::new(RwLock::new(loader)),
            collections: Arc::new(RwLock::new(HashMap::new())),
            default_collection: self.default_collection,
        };

        if self.preload {
            registry.preload_all().await?;
        }

        Ok(registry)
    }
}

impl Default for CollectionRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of loaded collections
pub struct CollectionRegistry {
    /// Collection loader
    loader: Arc<RwLock<CollectionLoader>>,
    /// Loaded collections
    collections: Arc<RwLock<HashMap<String, Collection>>>,
    /// Default collection for unqualified names
    default_collection: Option<String>,
}

impl CollectionRegistry {
    /// Create a new registry builder
    pub fn builder() -> CollectionRegistryBuilder {
        CollectionRegistryBuilder::new()
    }

    /// Create a registry with default settings
    pub async fn new() -> CollectionResult<Self> {
        Self::builder().build().await
    }

    /// Get a collection by fully qualified name
    pub async fn get(&self, namespace: &str, name: &str) -> CollectionResult<Collection> {
        let fqn = format!("{}.{}", namespace, name);

        // Check cache
        {
            let collections = self.collections.read().await;
            if let Some(collection) = collections.get(&fqn) {
                return Ok(collection.clone());
            }
        }

        // Load collection
        let mut loader = self.loader.write().await;
        let collection = loader.load(namespace, name).await?.clone();

        // Cache it
        {
            let mut collections = self.collections.write().await;
            collections.insert(fqn, collection.clone());
        }

        Ok(collection)
    }

    /// Get a collection by FQCN
    pub async fn get_by_fqcn(&self, fqcn: &Fqcn) -> CollectionResult<Collection> {
        self.get(&fqcn.namespace, &fqcn.collection).await
    }

    /// Resolve a module by FQCN
    pub async fn resolve_module(&self, fqcn: &Fqcn) -> CollectionResult<PathBuf> {
        let collection = self.get_by_fqcn(fqcn).await?;

        collection
            .get_module_path(&fqcn.resource)
            .cloned()
            .ok_or_else(|| CollectionError::ModuleNotFound { fqcn: fqcn.full() })
    }

    /// Resolve a role by FQCN
    pub async fn resolve_role(&self, fqcn: &Fqcn) -> CollectionResult<PathBuf> {
        let collection = self.get_by_fqcn(fqcn).await?;

        collection
            .get_role_path(&fqcn.resource)
            .cloned()
            .ok_or_else(|| CollectionError::RoleNotFound { fqcn: fqcn.full() })
    }

    /// Resolve a plugin by FQCN and type
    pub async fn resolve_plugin(
        &self,
        fqcn: &Fqcn,
        plugin_type: &str,
    ) -> CollectionResult<PathBuf> {
        let collection = self.get_by_fqcn(fqcn).await?;

        collection
            .get_plugin_path(plugin_type, &fqcn.resource)
            .cloned()
            .ok_or_else(|| CollectionError::PluginNotFound {
                plugin_type: plugin_type.to_string(),
                fqcn: fqcn.full(),
            })
    }

    /// Resolve a simple module name using the default collection
    pub async fn resolve_simple_module(&self, name: &str) -> CollectionResult<PathBuf> {
        // First, try ansible.builtin
        let builtin_fqcn = Fqcn::from_short_name(name);
        if let Ok(path) = self.resolve_module(&builtin_fqcn).await {
            return Ok(path);
        }

        // Try default collection if set
        if let Some(ref default) = self.default_collection {
            let parts: Vec<&str> = default.split('.').collect();
            if parts.len() == 2 {
                let fqcn = Fqcn::new(parts[0], parts[1], name);
                if let Ok(path) = self.resolve_module(&fqcn).await {
                    return Ok(path);
                }
            }
        }

        Err(CollectionError::ModuleNotFound {
            fqcn: name.to_string(),
        })
    }

    /// Preload all available collections
    pub async fn preload_all(&self) -> CollectionResult<()> {
        let loader = self.loader.read().await;
        let all_collections = loader.list_all().await?;
        drop(loader);

        for (namespace, name, _path) in all_collections {
            // Load each collection, ignoring errors
            let _ = self.get(&namespace, &name).await;
        }

        Ok(())
    }

    /// List all loaded collections
    pub async fn list_loaded(&self) -> Vec<String> {
        let collections = self.collections.read().await;
        collections.keys().cloned().collect()
    }

    /// List all available collections
    pub async fn list_available(&self) -> CollectionResult<Vec<(String, String)>> {
        let loader = self.loader.read().await;
        let all = loader.list_all().await?;
        Ok(all.into_iter().map(|(ns, name, _)| (ns, name)).collect())
    }

    /// Clear the collection cache
    pub async fn clear_cache(&self) {
        let mut collections = self.collections.write().await;
        collections.clear();
    }

    /// Get the default collection
    pub fn default_collection(&self) -> Option<&str> {
        self.default_collection.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_builder() {
        let registry = CollectionRegistry::builder()
            .with_search_path("/tmp/collections")
            .with_default_collection("community.general")
            .build()
            .await
            .unwrap();

        assert_eq!(registry.default_collection(), Some("community.general"));
    }
}
