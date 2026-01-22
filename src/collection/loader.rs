//! Collection loading from filesystem
//!
//! Handles discovering and loading collections from search paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::metadata::GalaxyMetadata;
use super::runtime::RuntimeConfig;
use super::{Collection, CollectionError, CollectionMetadata, CollectionResult, PluginType};

/// Search path for collections
#[derive(Debug, Clone)]
pub struct CollectionSearchPath {
    /// Path to search
    pub path: PathBuf,
    /// Priority (lower = higher priority)
    pub priority: u32,
    /// Whether this is a system path
    pub is_system: bool,
}

impl CollectionSearchPath {
    /// Create a new search path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            priority: 100,
            is_system: false,
        }
    }

    /// Create a system search path
    pub fn system(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            priority: 1000,
            is_system: true,
        }
    }

    /// Set priority
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

/// Loader for collections from filesystem
pub struct CollectionLoader {
    /// Search paths
    search_paths: Vec<CollectionSearchPath>,
    /// Loaded collections cache
    cache: HashMap<String, Collection>,
}

impl CollectionLoader {
    /// Create a new collection loader
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            cache: HashMap::new(),
        }
    }

    /// Add a search path
    pub fn add_search_path(&mut self, path: CollectionSearchPath) {
        self.search_paths.push(path);
        self.search_paths.sort_by_key(|p| p.priority);
    }

    /// Add default search paths
    pub fn with_default_paths(mut self) -> Self {
        // Current directory collections
        self.add_search_path(CollectionSearchPath::new("./collections").with_priority(10));

        // User collections
        if let Some(home) = dirs::home_dir() {
            self.add_search_path(
                CollectionSearchPath::new(home.join(".ansible/collections")).with_priority(50),
            );
        }

        // System collections
        self.add_search_path(CollectionSearchPath::system(
            "/usr/share/ansible/collections",
        ));

        self
    }

    /// Find a collection by name
    pub fn find(&self, namespace: &str, name: &str) -> Option<PathBuf> {
        for search_path in &self.search_paths {
            let collection_path = search_path
                .path
                .join("ansible_collections")
                .join(namespace)
                .join(name);

            if collection_path.exists() && collection_path.join("galaxy.yml").exists() {
                return Some(collection_path);
            }
        }
        None
    }

    /// Load a collection by name
    pub async fn load(&mut self, namespace: &str, name: &str) -> CollectionResult<&Collection> {
        let fqn = format!("{}.{}", namespace, name);

        // Check cache first
        if self.cache.contains_key(&fqn) {
            return Ok(self.cache.get(&fqn).unwrap());
        }

        // Find collection path
        let path = self
            .find(namespace, name)
            .ok_or_else(|| CollectionError::NotFound {
                namespace: namespace.to_string(),
                name: name.to_string(),
            })?;

        // Load collection
        let collection = Self::load_from_path(&path).await?;
        self.cache.insert(fqn.clone(), collection);

        Ok(self.cache.get(&fqn).unwrap())
    }

    /// Load a collection from a specific path
    pub async fn load_from_path(path: &Path) -> CollectionResult<Collection> {
        // Load galaxy.yml
        let galaxy_path = path.join("galaxy.yml");
        let galaxy = if galaxy_path.exists() {
            Some(GalaxyMetadata::from_file(&galaxy_path)?)
        } else {
            return Err(CollectionError::LoadError {
                path: path.to_path_buf(),
                message: "galaxy.yml not found".to_string(),
            });
        };

        let galaxy = galaxy.unwrap();

        // Load runtime.yml
        let runtime_path = path.join("meta/runtime.yml");
        let runtime = if runtime_path.exists() {
            RuntimeConfig::from_file(&runtime_path).ok()
        } else {
            None
        };

        // Discover modules
        let modules = Self::discover_plugins(path, PluginType::Module).await?;

        // Discover roles
        let roles = Self::discover_roles(path).await?;

        // Discover all plugin types
        let mut plugins = HashMap::new();
        for plugin_type in PluginType::all() {
            if *plugin_type != PluginType::Module {
                let type_plugins = Self::discover_plugins(path, *plugin_type).await?;
                if !type_plugins.is_empty() {
                    plugins.insert(plugin_type.directory_name().to_string(), type_plugins);
                }
            }
        }

        Ok(Collection {
            namespace: galaxy.namespace.clone(),
            name: galaxy.name.clone(),
            version: galaxy.version.clone(),
            path: path.to_path_buf(),
            metadata: CollectionMetadata::from_galaxy(galaxy),
            runtime,
            modules,
            roles,
            plugins,
        })
    }

    /// Discover plugins of a specific type
    async fn discover_plugins(
        collection_path: &Path,
        plugin_type: PluginType,
    ) -> CollectionResult<HashMap<String, PathBuf>> {
        let mut plugins = HashMap::new();
        let plugins_dir = collection_path
            .join("plugins")
            .join(plugin_type.directory_name());

        if !plugins_dir.exists() {
            return Ok(plugins);
        }

        Self::scan_plugin_directory(&plugins_dir, &mut plugins).await?;

        Ok(plugins)
    }

    /// Scan a directory for plugin files
    async fn scan_plugin_directory(
        dir: &Path,
        plugins: &mut HashMap<String, PathBuf>,
    ) -> CollectionResult<()> {
        let mut entries = tokio::fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectories
                Box::pin(Self::scan_plugin_directory(&path, plugins)).await?;
            } else if let Some(ext) = path.extension() {
                // Python files are plugins
                if ext == "py" {
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy();
                        // Skip __init__.py and private modules
                        if !name.starts_with('_') {
                            plugins.insert(name.to_string(), path);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Discover roles in a collection
    async fn discover_roles(collection_path: &Path) -> CollectionResult<HashMap<String, PathBuf>> {
        let mut roles = HashMap::new();
        let roles_dir = collection_path.join("roles");

        if !roles_dir.exists() {
            return Ok(roles);
        }

        let mut entries = tokio::fs::read_dir(&roles_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                // Check for role markers (tasks/main.yml or meta/main.yml)
                let is_role = path.join("tasks/main.yml").exists()
                    || path.join("tasks/main.yaml").exists()
                    || path.join("meta/main.yml").exists()
                    || path.join("meta/main.yaml").exists();

                if is_role {
                    if let Some(name) = path.file_name() {
                        roles.insert(name.to_string_lossy().to_string(), path);
                    }
                }
            }
        }

        Ok(roles)
    }

    /// List all available collections
    pub async fn list_all(&self) -> CollectionResult<Vec<(String, String, PathBuf)>> {
        let mut collections = Vec::new();

        for search_path in &self.search_paths {
            let base = search_path.path.join("ansible_collections");
            if !base.exists() {
                continue;
            }

            // Iterate namespaces
            if let Ok(mut namespaces) = tokio::fs::read_dir(&base).await {
                while let Ok(Some(ns_entry)) = namespaces.next_entry().await {
                    let ns_path = ns_entry.path();
                    if !ns_path.is_dir() {
                        continue;
                    }

                    let namespace = ns_entry.file_name().to_string_lossy().to_string();

                    // Iterate collections in namespace
                    if let Ok(mut colls) = tokio::fs::read_dir(&ns_path).await {
                        while let Ok(Some(coll_entry)) = colls.next_entry().await {
                            let coll_path = coll_entry.path();
                            if coll_path.is_dir() && coll_path.join("galaxy.yml").exists() {
                                let name = coll_entry.file_name().to_string_lossy().to_string();
                                collections.push((namespace.clone(), name, coll_path));
                            }
                        }
                    }
                }
            }
        }

        Ok(collections)
    }

    /// Get search paths
    pub fn search_paths(&self) -> &[CollectionSearchPath] {
        &self.search_paths
    }
}

impl Default for CollectionLoader {
    fn default() -> Self {
        Self::new().with_default_paths()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_path_priority() {
        let mut loader = CollectionLoader::new();
        loader.add_search_path(CollectionSearchPath::new("/low").with_priority(100));
        loader.add_search_path(CollectionSearchPath::new("/high").with_priority(10));

        assert_eq!(loader.search_paths[0].path, PathBuf::from("/high"));
        assert_eq!(loader.search_paths[1].path, PathBuf::from("/low"));
    }

    #[test]
    fn test_default_paths() {
        let loader = CollectionLoader::default();
        assert!(!loader.search_paths.is_empty());
    }
}
