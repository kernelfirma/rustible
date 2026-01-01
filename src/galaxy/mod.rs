//! Ansible Galaxy Support Module
//!
//! This module provides comprehensive support for installing and managing
//! Ansible Galaxy collections and roles. It addresses Galaxy instability
//! pain points by providing:
//!
//! - **Robust API client**: HTTP client with retry logic and timeout handling
//! - **Collection installation**: Install collections from Galaxy or tarballs
//! - **Role installation**: Install roles from Galaxy or Git repositories
//! - **Requirements parsing**: Parse and process requirements.yml files
//! - **Local caching**: Cache downloaded artifacts with integrity verification
//! - **Offline mode**: Fall back to cached artifacts when Galaxy is unavailable
//!
//! # Architecture
//!
//! ```text
//! +-------------------+
//! |   GalaxyClient    |  HTTP API interactions
//! +-------------------+
//!          |
//!          v
//! +-------------------+
//! |   GalaxyCache     |  Local caching layer
//! +-------------------+
//!          |
//!          v
//! +-------------------+     +-------------------+
//! | CollectionInstall | <-> | RoleInstaller     |
//! +-------------------+     +-------------------+
//!          |                         |
//!          v                         v
//! +-------------------+     +-------------------+
//! | RequirementsFile  |     | IntegrityVerifier |
//! +-------------------+     +-------------------+
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::galaxy::{Galaxy, GalaxyConfig, RequirementsFile};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create Galaxy client with default configuration
//!     let galaxy = Galaxy::new(GalaxyConfig::default())?;
//!
//!     // Install a collection
//!     galaxy.install_collection("community.general", Some("5.0.0")).await?;
//!
//!     // Install from requirements.yml
//!     let requirements = RequirementsFile::from_path("requirements.yml").await?;
//!     galaxy.install_requirements(&requirements).await?;
//!
//!     Ok(())
//! }
//! ```

mod cache;
mod client;
mod collection;
mod error;
mod integrity;
mod requirements;
mod role;

pub use cache::{CachedArtifact, GalaxyCache, GalaxyCacheConfig};
pub use client::{GalaxyClient, GalaxyClientBuilder};
pub use collection::{Collection, CollectionInfo, CollectionInstaller, CollectionVersion};
pub use error::{GalaxyError, GalaxyResult};
pub use integrity::{ChecksumAlgorithm, FileIntegrity, IntegrityVerifier};
pub use requirements::{Requirement, RequirementSource, RequirementType, RequirementsFile};
pub use role::{GalaxyRole, RoleInfo, RoleInstaller};

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::GalaxyConfig;

/// Main Galaxy interface that coordinates all Galaxy operations.
///
/// This struct provides a unified interface for installing collections
/// and roles from Ansible Galaxy, handling caching, integrity verification,
/// and offline mode fallback.
pub struct Galaxy {
    /// The HTTP client for Galaxy API interactions
    client: Arc<GalaxyClient>,
    /// Local cache for downloaded artifacts
    cache: Arc<GalaxyCache>,
    /// Configuration
    config: GalaxyConfig,
    /// Collection installer
    collection_installer: CollectionInstaller,
    /// Role installer
    role_installer: RoleInstaller,
    /// Offline mode flag
    offline_mode: bool,
}

impl Galaxy {
    /// Create a new Galaxy instance with the provided configuration.
    pub fn new(config: GalaxyConfig) -> GalaxyResult<Self> {
        let cache_config = GalaxyCacheConfig::from_galaxy_config(&config);
        let cache = Arc::new(GalaxyCache::new(cache_config)?);
        let client = Arc::new(GalaxyClient::new(&config)?);

        let collection_installer =
            CollectionInstaller::new(Arc::clone(&client), Arc::clone(&cache));

        let role_installer = RoleInstaller::new(Arc::clone(&client), Arc::clone(&cache));

        Ok(Self {
            client,
            cache,
            config,
            collection_installer,
            role_installer,
            offline_mode: false,
        })
    }

    /// Create a Galaxy instance with offline mode enabled.
    ///
    /// In offline mode, only cached artifacts will be used and no
    /// network requests will be made.
    pub fn offline(config: GalaxyConfig) -> GalaxyResult<Self> {
        let mut galaxy = Self::new(config)?;
        galaxy.offline_mode = true;
        Ok(galaxy)
    }

    /// Enable or disable offline mode.
    pub fn set_offline_mode(&mut self, offline: bool) {
        self.offline_mode = offline;
    }

    /// Check if offline mode is enabled.
    pub fn is_offline(&self) -> bool {
        self.offline_mode
    }

    /// Install a collection from Galaxy.
    ///
    /// # Arguments
    ///
    /// * `name` - The collection name in format "namespace.name"
    /// * `version` - Optional version constraint (e.g., ">=1.0.0,<2.0.0")
    /// * `dest` - Optional destination path (defaults to collections_path from config)
    ///
    /// # Returns
    ///
    /// Returns the path where the collection was installed.
    pub async fn install_collection(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<&PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        if self.offline_mode {
            return self.install_collection_offline(name, version, dest).await;
        }

        self.collection_installer
            .install(name, version, dest.cloned())
            .await
    }

    /// Install a collection from cache only (offline mode).
    async fn install_collection_offline(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<&PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        self.collection_installer
            .install_from_cache(name, version, dest.cloned())
            .await
    }

    /// Install a role from Galaxy.
    ///
    /// # Arguments
    ///
    /// * `name` - The role name (can be namespace.name or just name)
    /// * `version` - Optional version constraint
    /// * `dest` - Optional destination path (defaults to roles_path from config)
    ///
    /// # Returns
    ///
    /// Returns the path where the role was installed.
    pub async fn install_role(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<&PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        if self.offline_mode {
            return self.install_role_offline(name, version, dest).await;
        }

        self.role_installer
            .install(name, version, dest.cloned())
            .await
    }

    /// Install a role from cache only (offline mode).
    async fn install_role_offline(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<&PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        self.role_installer
            .install_from_cache(name, version, dest.cloned())
            .await
    }

    /// Install all requirements from a requirements file.
    ///
    /// This method parses the requirements file and installs all
    /// collections and roles specified in it.
    pub async fn install_requirements(
        &self,
        requirements: &RequirementsFile,
    ) -> GalaxyResult<Vec<PathBuf>> {
        let mut installed_paths = Vec::new();

        // Install collections
        for collection in &requirements.collections {
            let path = self
                .install_collection(&collection.name, collection.version.as_deref(), None)
                .await?;
            installed_paths.push(path);
        }

        // Install roles
        for role in &requirements.roles {
            let path = self
                .install_role(&role.name, role.version.as_deref(), None)
                .await?;
            installed_paths.push(path);
        }

        Ok(installed_paths)
    }

    /// Install requirements from a file path.
    pub async fn install_requirements_file(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> GalaxyResult<Vec<PathBuf>> {
        let requirements = RequirementsFile::from_path(path).await?;
        self.install_requirements(&requirements).await
    }

    /// Get information about a collection from Galaxy.
    pub async fn get_collection_info(&self, name: &str) -> GalaxyResult<CollectionInfo> {
        self.client.get_collection_info(name).await
    }

    /// Get information about a role from Galaxy.
    pub async fn get_role_info(&self, name: &str) -> GalaxyResult<RoleInfo> {
        self.client.get_role_info(name).await
    }

    /// List available versions for a collection.
    pub async fn list_collection_versions(
        &self,
        name: &str,
    ) -> GalaxyResult<Vec<CollectionVersion>> {
        self.client.list_collection_versions(name).await
    }

    /// Search for collections matching a query.
    pub async fn search_collections(&self, query: &str) -> GalaxyResult<Vec<CollectionInfo>> {
        self.client.search_collections(query).await
    }

    /// Search for roles matching a query.
    pub async fn search_roles(&self, query: &str) -> GalaxyResult<Vec<RoleInfo>> {
        self.client.search_roles(query).await
    }

    /// Clear the local cache.
    pub fn clear_cache(&self) -> GalaxyResult<()> {
        self.cache.clear()
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> cache::CacheStats {
        self.cache.stats()
    }

    /// Verify integrity of all cached artifacts.
    pub async fn verify_cache_integrity(&self) -> GalaxyResult<Vec<integrity::IntegrityReport>> {
        self.cache.verify_all().await
    }

    /// Get the underlying client for advanced operations.
    pub fn client(&self) -> &GalaxyClient {
        &self.client
    }

    /// Get the cache instance.
    pub fn cache(&self) -> &GalaxyCache {
        &self.cache
    }

    /// Get the configuration.
    pub fn config(&self) -> &GalaxyConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_galaxy_creation() {
        let config = GalaxyConfig::default();
        let result = Galaxy::new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_offline_mode() {
        let config = GalaxyConfig::default();
        let galaxy = Galaxy::offline(config).unwrap();
        assert!(galaxy.is_offline());
    }

    #[test]
    fn test_set_offline_mode() {
        let config = GalaxyConfig::default();
        let mut galaxy = Galaxy::new(config).unwrap();
        assert!(!galaxy.is_offline());

        galaxy.set_offline_mode(true);
        assert!(galaxy.is_offline());

        galaxy.set_offline_mode(false);
        assert!(!galaxy.is_offline());
    }
}
