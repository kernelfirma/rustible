//! Ansible Collection Compatibility for Rustible
//!
//! This module provides support for Ansible collections, enabling:
//! - Parsing of collection structure (galaxy.yml, meta/runtime.yml)
//! - Fully Qualified Collection Names (FQCN) resolution
//! - Collection dependency resolution with version constraints
//! - Local collection development workflow
//! - Migration path from Ansible collections
//!
//! # Collection Structure
//!
//! Collections follow the Ansible directory layout:
//!
//! ```text
//! namespace/
//! └── collection_name/
//!     ├── galaxy.yml           # Collection metadata
//!     ├── meta/
//!     │   └── runtime.yml      # Runtime configuration
//!     ├── plugins/
//!     │   ├── modules/         # Module plugins
//!     │   ├── action/          # Action plugins
//!     │   ├── lookup/          # Lookup plugins
//!     │   ├── filter/          # Filter plugins
//!     │   └── ...
//!     ├── roles/               # Collection roles
//!     ├── playbooks/           # Collection playbooks
//!     └── docs/                # Documentation
//! ```
//!
//! # FQCN (Fully Qualified Collection Name)
//!
//! FQCNs uniquely identify collection content:
//! - `namespace.collection.module_name` - For modules
//! - `namespace.collection.role_name` - For roles
//! - `namespace.collection.plugin_name` - For other plugins
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::collection::{Collection, CollectionRegistry, Fqcn};
//!
//! // Parse a FQCN
//! let fqcn = Fqcn::parse("community.general.json_query")?;
//! assert_eq!(fqcn.namespace, "community");
//! assert_eq!(fqcn.collection, "general");
//! assert_eq!(fqcn.resource, "json_query");
//!
//! // Load collections
//! let registry = CollectionRegistry::builder()
//!     .with_search_path("~/.ansible/collections")
//!     .with_search_path("./collections")
//!     .build()
//!     .await?;
//!
//! // Resolve a module from a collection
//! let module = registry.resolve_module(&fqcn).await?;
//! # Ok(())
//! # }
//! ```

pub mod dependency;
pub mod fqcn;
pub mod loader;
pub mod metadata;
pub mod registry;
pub mod runtime;

pub use dependency::{
    CollectionDependency, DependencyGraph, DependencyResolutionError, DependencyResolver,
    VersionConstraint,
};
pub use fqcn::{Fqcn, FqcnParseError, ResourceType};
pub use loader::{CollectionLoader, CollectionSearchPath};
pub use metadata::{CollectionMetadata, GalaxyMetadata};
pub use registry::{CollectionRegistry, CollectionRegistryBuilder};
pub use runtime::{PluginRouting, RuntimeConfig};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during collection operations
#[derive(Debug, Error)]
pub enum CollectionError {
    #[error("Collection not found: {namespace}.{name}")]
    NotFound { namespace: String, name: String },

    #[error("Invalid FQCN: {0}")]
    InvalidFqcn(#[from] FqcnParseError),

    #[error("Failed to load collection from '{path}': {message}")]
    LoadError { path: PathBuf, message: String },

    #[error("Invalid galaxy.yml: {0}")]
    InvalidGalaxyYml(String),

    #[error("Invalid runtime.yml: {0}")]
    InvalidRuntimeYml(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyError(#[from] DependencyResolutionError),

    #[error("Module not found in collection: {fqcn}")]
    ModuleNotFound { fqcn: String },

    #[error("Role not found in collection: {fqcn}")]
    RoleNotFound { fqcn: String },

    #[error("Plugin not found: {plugin_type}/{fqcn}")]
    PluginNotFound { plugin_type: String, fqcn: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Result type for collection operations
pub type CollectionResult<T> = Result<T, CollectionError>;

/// Represents a loaded Ansible collection
#[derive(Debug, Clone)]
pub struct Collection {
    /// Collection namespace (e.g., "community", "ansible")
    pub namespace: String,

    /// Collection name (e.g., "general", "builtin")
    pub name: String,

    /// Version of the collection
    pub version: String,

    /// Path to the collection root
    pub path: PathBuf,

    /// Collection metadata from galaxy.yml
    pub metadata: CollectionMetadata,

    /// Runtime configuration from meta/runtime.yml
    pub runtime: Option<RuntimeConfig>,

    /// Available modules in this collection
    pub modules: HashMap<String, PathBuf>,

    /// Available roles in this collection
    pub roles: HashMap<String, PathBuf>,

    /// Available plugins by type
    pub plugins: HashMap<String, HashMap<String, PathBuf>>,
}

impl Collection {
    /// Returns the fully qualified name (namespace.name)
    pub fn fqn(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// Checks if this collection provides a specific module
    pub fn has_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    /// Checks if this collection provides a specific role
    pub fn has_role(&self, name: &str) -> bool {
        self.roles.contains_key(name)
    }

    /// Checks if this collection provides a specific plugin
    pub fn has_plugin(&self, plugin_type: &str, name: &str) -> bool {
        self.plugins
            .get(plugin_type)
            .map(|plugins| plugins.contains_key(name))
            .unwrap_or(false)
    }

    /// Gets the path to a module
    pub fn get_module_path(&self, name: &str) -> Option<&PathBuf> {
        self.modules.get(name)
    }

    /// Gets the path to a role
    pub fn get_role_path(&self, name: &str) -> Option<&PathBuf> {
        self.roles.get(name)
    }

    /// Gets the path to a plugin
    pub fn get_plugin_path(&self, plugin_type: &str, name: &str) -> Option<&PathBuf> {
        self.plugins.get(plugin_type).and_then(|p| p.get(name))
    }

    /// Lists all available modules
    pub fn list_modules(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    /// Lists all available roles
    pub fn list_roles(&self) -> Vec<&str> {
        self.roles.keys().map(|s| s.as_str()).collect()
    }

    /// Lists all available plugin types
    pub fn list_plugin_types(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Lists plugins of a specific type
    pub fn list_plugins(&self, plugin_type: &str) -> Vec<&str> {
        self.plugins
            .get(plugin_type)
            .map(|p| p.keys().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }
}

/// Plugin types supported in collections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    /// Module plugins (plugins/modules/)
    Module,
    /// Action plugins (plugins/action/)
    Action,
    /// Lookup plugins (plugins/lookup/)
    Lookup,
    /// Filter plugins (plugins/filter/)
    Filter,
    /// Test plugins (plugins/test/)
    Test,
    /// Connection plugins (plugins/connection/)
    Connection,
    /// Callback plugins (plugins/callback/)
    Callback,
    /// Inventory plugins (plugins/inventory/)
    Inventory,
    /// Vars plugins (plugins/vars/)
    Vars,
    /// Strategy plugins (plugins/strategy/)
    Strategy,
    /// Cache plugins (plugins/cache/)
    Cache,
    /// Doc fragments (plugins/doc_fragments/)
    DocFragments,
    /// Module utils (plugins/module_utils/)
    ModuleUtils,
}

impl PluginType {
    /// Returns the directory name for this plugin type
    pub fn directory_name(&self) -> &'static str {
        match self {
            PluginType::Module => "modules",
            PluginType::Action => "action",
            PluginType::Lookup => "lookup",
            PluginType::Filter => "filter",
            PluginType::Test => "test",
            PluginType::Connection => "connection",
            PluginType::Callback => "callback",
            PluginType::Inventory => "inventory",
            PluginType::Vars => "vars",
            PluginType::Strategy => "strategy",
            PluginType::Cache => "cache",
            PluginType::DocFragments => "doc_fragments",
            PluginType::ModuleUtils => "module_utils",
        }
    }

    /// Returns all plugin types
    pub fn all() -> &'static [PluginType] {
        &[
            PluginType::Module,
            PluginType::Action,
            PluginType::Lookup,
            PluginType::Filter,
            PluginType::Test,
            PluginType::Connection,
            PluginType::Callback,
            PluginType::Inventory,
            PluginType::Vars,
            PluginType::Strategy,
            PluginType::Cache,
            PluginType::DocFragments,
            PluginType::ModuleUtils,
        ]
    }
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.directory_name())
    }
}

impl std::str::FromStr for PluginType {
    type Err = CollectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "module" | "modules" => Ok(PluginType::Module),
            "action" => Ok(PluginType::Action),
            "lookup" => Ok(PluginType::Lookup),
            "filter" => Ok(PluginType::Filter),
            "test" => Ok(PluginType::Test),
            "connection" => Ok(PluginType::Connection),
            "callback" => Ok(PluginType::Callback),
            "inventory" => Ok(PluginType::Inventory),
            "vars" => Ok(PluginType::Vars),
            "strategy" => Ok(PluginType::Strategy),
            "cache" => Ok(PluginType::Cache),
            "doc_fragments" => Ok(PluginType::DocFragments),
            "module_utils" => Ok(PluginType::ModuleUtils),
            _ => Err(CollectionError::PluginNotFound {
                plugin_type: s.to_string(),
                fqcn: String::new(),
            }),
        }
    }
}

/// Configuration for collection behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionConfig {
    /// Search paths for collections
    #[serde(default)]
    pub collections_paths: Vec<PathBuf>,

    /// Whether to scan system collection paths
    #[serde(default = "default_true")]
    pub scan_system_paths: bool,

    /// Whether to auto-install missing dependencies
    #[serde(default)]
    pub auto_install_dependencies: bool,

    /// Cache TTL for collection metadata (in seconds)
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_seconds: u64,

    /// Default collection for unqualified names
    #[serde(default)]
    pub default_collection: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    3600 // 1 hour
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            collections_paths: vec![],
            scan_system_paths: true,
            auto_install_dependencies: false,
            cache_ttl_seconds: 3600,
            default_collection: None,
        }
    }
}

impl CollectionConfig {
    /// Creates a new configuration with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a collection search path
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.collections_paths.push(path.into());
        self
    }

    /// Sets whether to scan system paths
    pub fn with_system_paths(mut self, enabled: bool) -> Self {
        self.scan_system_paths = enabled;
        self
    }

    /// Sets auto-install behavior
    pub fn with_auto_install(mut self, enabled: bool) -> Self {
        self.auto_install_dependencies = enabled;
        self
    }

    /// Sets the default collection
    pub fn with_default_collection(mut self, collection: impl Into<String>) -> Self {
        self.default_collection = Some(collection.into());
        self
    }

    /// Returns the effective search paths (including system paths if enabled)
    pub fn effective_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.collections_paths.clone();

        if self.scan_system_paths {
            // Add standard Ansible collection paths
            if let Some(home) = dirs::home_dir() {
                paths.push(home.join(".ansible/collections"));
            }

            // System-wide paths
            paths.push(PathBuf::from("/usr/share/ansible/collections"));
            paths.push(PathBuf::from("/etc/ansible/collections"));

            // Environment-based path
            if let Ok(ansible_collections) = std::env::var("ANSIBLE_COLLECTIONS_PATH") {
                for p in ansible_collections.split(':') {
                    if !p.is_empty() {
                        paths.push(PathBuf::from(p));
                    }
                }
            }
        }

        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_fqn() {
        let collection = Collection {
            namespace: "community".to_string(),
            name: "general".to_string(),
            version: "1.0.0".to_string(),
            path: PathBuf::from("/tmp/test"),
            metadata: CollectionMetadata::default(),
            runtime: None,
            modules: HashMap::new(),
            roles: HashMap::new(),
            plugins: HashMap::new(),
        };

        assert_eq!(collection.fqn(), "community.general");
    }

    #[test]
    fn test_plugin_type_directory() {
        assert_eq!(PluginType::Module.directory_name(), "modules");
        assert_eq!(PluginType::Action.directory_name(), "action");
        assert_eq!(PluginType::Filter.directory_name(), "filter");
    }

    #[test]
    fn test_plugin_type_from_str() {
        assert_eq!(
            "module".parse::<PluginType>().unwrap(),
            PluginType::Module
        );
        assert_eq!(
            "modules".parse::<PluginType>().unwrap(),
            PluginType::Module
        );
        assert_eq!(
            "filter".parse::<PluginType>().unwrap(),
            PluginType::Filter
        );
    }

    #[test]
    fn test_collection_config_paths() {
        let config = CollectionConfig::new()
            .with_path("/custom/collections")
            .with_system_paths(false);

        let paths = config.effective_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/custom/collections"));
    }
}
