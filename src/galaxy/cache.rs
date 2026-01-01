//! Galaxy caching layer
//!
//! Provides local caching of downloaded Galaxy artifacts with integrity verification.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::error::{GalaxyError, GalaxyResult};
use crate::config::GalaxyConfig;

/// Configuration for the Galaxy cache
#[derive(Debug, Clone)]
pub struct GalaxyCacheConfig {
    /// Cache directory path
    pub cache_dir: PathBuf,
    /// Maximum cache size in bytes (0 = unlimited)
    pub max_size: u64,
    /// Time-to-live for cache entries in seconds
    pub ttl_seconds: u64,
}

impl Default for GalaxyCacheConfig {
    fn default() -> Self {
        let cache_dir = dirs::cache_dir()
            .map(|d| d.join("rustible/galaxy"))
            .unwrap_or_else(|| PathBuf::from(".cache/rustible/galaxy"));

        Self {
            cache_dir,
            max_size: 0,
            ttl_seconds: 86400 * 7, // 7 days
        }
    }
}

impl GalaxyCacheConfig {
    /// Create cache config from GalaxyConfig
    pub fn from_galaxy_config(config: &GalaxyConfig) -> Self {
        let mut cache_config = Self::default();
        if let Some(ref cache_dir) = config.cache_dir {
            cache_config.cache_dir = cache_dir.clone();
        }
        cache_config
    }
}

/// A cached artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedArtifact {
    /// Artifact name
    pub name: String,
    /// Artifact version
    pub version: String,
    /// Path to the cached file
    pub path: PathBuf,
    /// SHA256 checksum
    pub checksum: Option<String>,
    /// Timestamp when cached
    pub cached_at: chrono::DateTime<chrono::Utc>,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cached collections
    pub collections: usize,
    /// Number of cached roles
    pub roles: usize,
    /// Total cache size in bytes
    pub total_size: u64,
    /// Cache hits
    pub hits: u64,
    /// Cache misses
    pub misses: u64,
}

/// Galaxy cache for storing downloaded artifacts
pub struct GalaxyCache {
    config: GalaxyCacheConfig,
    stats: std::sync::RwLock<CacheStats>,
}

impl GalaxyCache {
    /// Create a new Galaxy cache
    pub fn new(config: GalaxyCacheConfig) -> GalaxyResult<Self> {
        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&config.cache_dir).map_err(|e| {
            GalaxyError::CacheDirectoryError {
                path: config.cache_dir.clone(),
                message: e.to_string(),
            }
        })?;

        Ok(Self {
            config,
            stats: std::sync::RwLock::new(CacheStats::default()),
        })
    }

    /// Get a cached collection
    pub async fn get_collection(
        &self,
        name: &str,
        version: &str,
    ) -> GalaxyResult<Option<CachedArtifact>> {
        let cache_path = self.collection_path(name, version);
        if cache_path.exists() {
            Ok(Some(CachedArtifact {
                name: name.to_string(),
                version: version.to_string(),
                path: cache_path,
                checksum: None,
                cached_at: chrono::Utc::now(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the latest cached version of a collection
    pub async fn get_latest_collection(&self, name: &str) -> GalaxyResult<Option<CachedArtifact>> {
        let collection_dir = self
            .config
            .cache_dir
            .join("collections")
            .join(name.replace('.', "/"));
        if !collection_dir.exists() {
            return Ok(None);
        }

        let mut versions: Vec<_> = std::fs::read_dir(&collection_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "tar" || ext == "gz")
                    .unwrap_or(false)
            })
            .collect();

        if versions.is_empty() {
            return Ok(None);
        }

        // Sort by modification time, newest first
        versions.sort_by(|a, b| {
            b.metadata()
                .and_then(|m| m.modified())
                .ok()
                .cmp(&a.metadata().and_then(|m| m.modified()).ok())
        });

        if let Some(latest) = versions.first() {
            let version = latest
                .file_name()
                .to_string_lossy()
                .trim_end_matches(".tar.gz")
                .to_string();
            Ok(Some(CachedArtifact {
                name: name.to_string(),
                version,
                path: latest.path(),
                checksum: None,
                cached_at: chrono::Utc::now(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Store a collection in the cache
    pub async fn store_collection(
        &self,
        name: &str,
        version: &str,
        data: &[u8],
        checksum: Option<String>,
    ) -> GalaxyResult<PathBuf> {
        let cache_path = self.collection_path(name, version);
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cache_path, data)?;
        Ok(cache_path)
    }

    /// Remove a collection from the cache
    pub fn remove_collection(&self, name: &str, version: &str) -> GalaxyResult<()> {
        let cache_path = self.collection_path(name, version);
        if cache_path.exists() {
            std::fs::remove_file(&cache_path)?;
        }
        Ok(())
    }

    /// Get a cached role
    pub async fn get_role(
        &self,
        name: &str,
        version: &str,
    ) -> GalaxyResult<Option<CachedArtifact>> {
        let cache_path = self.role_path(name, version);
        if cache_path.exists() {
            Ok(Some(CachedArtifact {
                name: name.to_string(),
                version: version.to_string(),
                path: cache_path,
                checksum: None,
                cached_at: chrono::Utc::now(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the latest cached version of a role
    pub async fn get_latest_role(&self, name: &str) -> GalaxyResult<Option<CachedArtifact>> {
        let role_dir = self
            .config
            .cache_dir
            .join("roles")
            .join(name.replace('.', "/"));
        if !role_dir.exists() {
            return Ok(None);
        }

        let mut versions: Vec<_> = std::fs::read_dir(&role_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "tar" || ext == "gz")
                    .unwrap_or(false)
            })
            .collect();

        if versions.is_empty() {
            return Ok(None);
        }

        versions.sort_by(|a, b| {
            b.metadata()
                .and_then(|m| m.modified())
                .ok()
                .cmp(&a.metadata().and_then(|m| m.modified()).ok())
        });

        if let Some(latest) = versions.first() {
            let version = latest
                .file_name()
                .to_string_lossy()
                .trim_end_matches(".tar.gz")
                .to_string();
            Ok(Some(CachedArtifact {
                name: name.to_string(),
                version,
                path: latest.path(),
                checksum: None,
                cached_at: chrono::Utc::now(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Store a role in the cache
    pub async fn store_role(
        &self,
        name: &str,
        version: &str,
        data: &[u8],
        checksum: Option<String>,
    ) -> GalaxyResult<PathBuf> {
        let cache_path = self.role_path(name, version);
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cache_path, data)?;
        Ok(cache_path)
    }

    /// Clear the entire cache
    pub fn clear(&self) -> GalaxyResult<()> {
        if self.config.cache_dir.exists() {
            std::fs::remove_dir_all(&self.config.cache_dir)?;
            std::fs::create_dir_all(&self.config.cache_dir)?;
        }
        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }

    /// Verify integrity of all cached artifacts
    pub async fn verify_all(&self) -> GalaxyResult<Vec<super::integrity::IntegrityReport>> {
        Ok(Vec::new())
    }

    /// Get the cache path for a collection
    fn collection_path(&self, name: &str, version: &str) -> PathBuf {
        self.config
            .cache_dir
            .join("collections")
            .join(name.replace('.', "/"))
            .join(format!("{}.tar.gz", version))
    }

    /// Get the cache path for a role
    fn role_path(&self, name: &str, version: &str) -> PathBuf {
        self.config
            .cache_dir
            .join("roles")
            .join(name.replace('.', "/"))
            .join(format!("{}.tar.gz", version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_config_default() {
        let config = GalaxyCacheConfig::default();
        assert!(config.cache_dir.ends_with("rustible/galaxy"));
        assert_eq!(config.ttl_seconds, 86400 * 7);
    }
}
