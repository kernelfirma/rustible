//! Collection Installation Support
//!
//! This module handles the installation of Ansible collections from
//! Galaxy, local tarballs, or Git repositories.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::cache::GalaxyCache;
use super::client::GalaxyClient;
use super::error::{GalaxyError, GalaxyResult};
use super::integrity::{ChecksumAlgorithm, IntegrityVerifier};

/// Information about a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    /// Collection namespace
    pub namespace: String,
    /// Collection name
    pub name: String,
    /// Highest version available
    #[serde(default)]
    pub highest_version: Option<VersionInfo>,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// Download count
    #[serde(default)]
    pub download_count: u64,
    /// Creation date
    #[serde(default)]
    pub created_at: Option<String>,
    /// Last modified date
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Deprecated flag
    #[serde(default)]
    pub deprecated: bool,
}

impl CollectionInfo {
    /// Get the full name (namespace.name)
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }
}

/// Version information for a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Version string
    pub version: String,
    /// Href for version details
    #[serde(default)]
    pub href: Option<String>,
}

/// Detailed collection version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionVersion {
    /// Version string
    pub version: String,
    /// Namespace
    #[serde(default)]
    pub namespace: Option<NamespaceInfo>,
    /// Collection name
    #[serde(default)]
    pub name: Option<String>,
    /// Download URL
    #[serde(default)]
    pub download_url: Option<String>,
    /// Artifact information
    #[serde(default)]
    pub artifact: Option<ArtifactInfo>,
    /// Collection metadata
    #[serde(default)]
    pub metadata: Option<CollectionMetadata>,
    /// Dependencies
    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,
    /// Creation date
    #[serde(default)]
    pub created_at: Option<String>,
    /// Href
    #[serde(default)]
    pub href: Option<String>,
}

/// Namespace information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceInfo {
    /// Namespace name
    pub name: String,
}

/// Artifact information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    /// Filename
    #[serde(default)]
    pub filename: Option<String>,
    /// SHA256 checksum
    #[serde(default)]
    pub sha256: Option<String>,
    /// Size in bytes
    #[serde(default)]
    pub size: Option<u64>,
}

/// Collection metadata from galaxy.yml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionMetadata {
    /// Namespace
    #[serde(default)]
    pub namespace: Option<String>,
    /// Name
    #[serde(default)]
    pub name: Option<String>,
    /// Version
    #[serde(default)]
    pub version: Option<String>,
    /// Readme file
    #[serde(default)]
    pub readme: Option<String>,
    /// Authors
    #[serde(default)]
    pub authors: Vec<String>,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// License
    #[serde(default)]
    pub license: Option<Vec<String>>,
    /// License file
    #[serde(default)]
    pub license_file: Option<String>,
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Dependencies
    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,
    /// Repository URL
    #[serde(default)]
    pub repository: Option<String>,
    /// Documentation URL
    #[serde(default)]
    pub documentation: Option<String>,
    /// Homepage URL
    #[serde(default)]
    pub homepage: Option<String>,
    /// Issues URL
    #[serde(default)]
    pub issues: Option<String>,
    /// Build ignore patterns
    #[serde(default)]
    pub build_ignore: Vec<String>,
}

/// Represents an installed collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    /// Namespace
    pub namespace: String,
    /// Name
    pub name: String,
    /// Version
    pub version: String,
    /// Installation path
    pub path: PathBuf,
    /// Metadata
    #[serde(default)]
    pub metadata: Option<CollectionMetadata>,
}

impl Collection {
    /// Get the full name (namespace.name)
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// Load collection from an installed path
    pub async fn from_path(path: impl AsRef<Path>) -> GalaxyResult<Self> {
        let path = path.as_ref();
        let manifest_path = path.join("MANIFEST.json");

        if !manifest_path.exists() {
            return Err(GalaxyError::InvalidArchive {
                path: path.to_path_buf(),
                message: "MANIFEST.json not found".to_string(),
            });
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: CollectionManifest = serde_json::from_str(&manifest_content)?;

        Ok(Self {
            namespace: manifest
                .collection_info
                .namespace
                .clone()
                .unwrap_or_default(),
            name: manifest.collection_info.name.clone().unwrap_or_default(),
            version: manifest.collection_info.version.clone().unwrap_or_default(),
            path: path.to_path_buf(),
            metadata: Some(manifest.collection_info),
        })
    }
}

/// Collection manifest (MANIFEST.json)
#[derive(Debug, Deserialize)]
struct CollectionManifest {
    collection_info: CollectionMetadata,
    #[serde(default)]
    file_manifest_file: Option<FileManifestInfo>,
}

#[derive(Debug, Deserialize)]
struct FileManifestInfo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    ftype: String,
    #[serde(default)]
    chksum_type: String,
    #[serde(default)]
    chksum_sha256: String,
}

/// Handles collection installation
pub struct CollectionInstaller {
    /// Galaxy client
    client: Arc<GalaxyClient>,
    /// Cache
    cache: Arc<GalaxyCache>,
    /// Default installation path
    default_path: PathBuf,
    /// Force reinstall
    force: bool,
    /// Verify integrity
    verify: bool,
}

impl CollectionInstaller {
    /// Create a new collection installer
    pub fn new(client: Arc<GalaxyClient>, cache: Arc<GalaxyCache>) -> Self {
        let default_path = dirs::home_dir()
            .map(|h| h.join(".ansible/collections/ansible_collections"))
            .unwrap_or_else(|| PathBuf::from("./collections/ansible_collections"));

        Self {
            client,
            cache,
            default_path,
            force: false,
            verify: true,
        }
    }

    /// Set force reinstall flag
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Set verify integrity flag
    pub fn verify(mut self, verify: bool) -> Self {
        self.verify = verify;
        self
    }

    /// Set default installation path
    pub fn default_path(mut self, path: PathBuf) -> Self {
        self.default_path = path;
        self
    }

    /// Install a collection
    pub async fn install(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        let dest = dest.unwrap_or_else(|| self.default_path.clone());
        let parts: Vec<&str> = name.split('.').collect();

        if parts.len() != 2 {
            return Err(GalaxyError::InvalidCollectionName {
                name: name.to_string(),
                reason: "Collection name must be in 'namespace.name' format".to_string(),
            });
        }

        let (namespace, collection_name) = (parts[0], parts[1]);
        let collection_path = dest.join(namespace).join(collection_name);

        // Check if already installed
        if collection_path.exists() && !self.force {
            if let Ok(installed) = Collection::from_path(&collection_path).await {
                if version.is_none() || version == Some(&installed.version) {
                    info!(
                        "Collection {} version {} already installed at {}",
                        name,
                        installed.version,
                        collection_path.display()
                    );
                    return Ok(collection_path);
                }
            }
        }

        // Resolve version
        let version_info = if let Some(v) = version {
            self.client.get_collection_version(name, v).await?
        } else {
            // Get the latest version
            let versions = self.client.list_collection_versions(name).await?;
            versions
                .into_iter()
                .max_by(|a, b| {
                    semver::Version::parse(&a.version)
                        .unwrap_or_else(|_| semver::Version::new(0, 0, 0))
                        .cmp(
                            &semver::Version::parse(&b.version)
                                .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
                        )
                })
                .ok_or_else(|| GalaxyError::CollectionNotFound {
                    name: name.to_string(),
                })?
        };

        let download_url = version_info.download_url.as_ref().ok_or_else(|| {
            GalaxyError::CollectionInstallFailed {
                name: name.to_string(),
                message: "No download URL available".to_string(),
            }
        })?;

        let expected_sha256 = version_info
            .artifact
            .as_ref()
            .and_then(|a| a.sha256.clone());

        // Try cache first
        if let Some(cached) = self
            .cache
            .get_collection(name, &version_info.version)
            .await?
        {
            info!("Using cached collection {}-{}", name, version_info.version);

            // Verify integrity if enabled
            if self.verify {
                if let Some(ref expected) = expected_sha256 {
                    let actual = IntegrityVerifier::compute_file_checksum(
                        &cached.path,
                        ChecksumAlgorithm::Sha256,
                    )
                    .await?;

                    if actual != *expected {
                        warn!("Cache integrity check failed, re-downloading");
                        self.cache.remove_collection(name, &version_info.version)?;
                    } else {
                        return self
                            .extract_and_install(&cached.path, &dest, namespace, collection_name)
                            .await;
                    }
                }
            } else {
                return self
                    .extract_and_install(&cached.path, &dest, namespace, collection_name)
                    .await;
            }
        }

        // Download collection
        info!("Downloading collection {}-{}", name, version_info.version);
        let data = self.client.download_collection(download_url).await?;

        // Verify checksum if available
        if self.verify {
            if let Some(ref expected) = expected_sha256 {
                let actual = IntegrityVerifier::compute_checksum(&data, ChecksumAlgorithm::Sha256);
                if actual != *expected {
                    return Err(GalaxyError::checksum_mismatch(
                        format!("{}-{}.tar.gz", name, version_info.version),
                        expected,
                        &actual,
                    ));
                }
                debug!("Checksum verified for {}", name);
            }
        }

        // Save to cache
        let cache_path = self
            .cache
            .store_collection(name, &version_info.version, &data, expected_sha256)
            .await?;

        // Extract and install
        self.extract_and_install(&cache_path, &dest, namespace, collection_name)
            .await
    }

    /// Install a collection from cache (offline mode)
    pub async fn install_from_cache(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        let dest = dest.unwrap_or_else(|| self.default_path.clone());
        let parts: Vec<&str> = name.split('.').collect();

        if parts.len() != 2 {
            return Err(GalaxyError::InvalidCollectionName {
                name: name.to_string(),
                reason: "Collection name must be in 'namespace.name' format".to_string(),
            });
        }

        let (namespace, collection_name) = (parts[0], parts[1]);

        // Get from cache
        let cached = if let Some(v) = version {
            self.cache.get_collection(name, v).await?
        } else {
            self.cache.get_latest_collection(name).await?
        };

        let cached = cached.ok_or_else(|| GalaxyError::NoCachedVersion {
            name: name.to_string(),
        })?;

        info!("Installing cached collection {}-{}", name, cached.version);

        self.extract_and_install(&cached.path, &dest, namespace, collection_name)
            .await
    }

    /// Extract a collection tarball and install it
    async fn extract_and_install(
        &self,
        tarball: &Path,
        dest: &Path,
        namespace: &str,
        name: &str,
    ) -> GalaxyResult<PathBuf> {
        let collection_path = dest.join(namespace).join(name);

        // Create destination directory
        tokio::fs::create_dir_all(&collection_path)
            .await
            .map_err(|e| GalaxyError::CollectionInstallFailed {
                name: format!("{}.{}", namespace, name),
                message: format!("Failed to create directory: {}", e),
            })?;

        // Extract tarball
        let tarball_data = tokio::fs::read(tarball).await?;
        let gz = flate2::read::GzDecoder::new(&tarball_data[..]);
        let mut archive = tar::Archive::new(gz);

        // Find the root directory in the tarball (usually namespace-name-version)
        let temp_dir = tempfile::tempdir()?;
        archive
            .unpack(temp_dir.path())
            .map_err(|e| GalaxyError::ExtractionFailed {
                path: tarball.to_path_buf(),
                message: e.to_string(),
            })?;

        // Find the extracted directory
        let mut entries = std::fs::read_dir(temp_dir.path())?;
        let extracted_dir = entries
            .next()
            .and_then(|e| e.ok())
            .map(|e| e.path())
            .ok_or_else(|| GalaxyError::ExtractionFailed {
                path: tarball.to_path_buf(),
                message: "Empty archive".to_string(),
            })?;

        // Move contents to final destination
        Self::copy_dir_recursive(&extracted_dir, &collection_path).await?;

        info!(
            "Installed collection {}.{} to {}",
            namespace,
            name,
            collection_path.display()
        );

        Ok(collection_path)
    }

    /// Recursively copy directory contents
    async fn copy_dir_recursive(src: &Path, dest: &Path) -> GalaxyResult<()> {
        if !dest.exists() {
            tokio::fs::create_dir_all(dest).await?;
        }

        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let dest_path = dest.join(entry.file_name());

            if path.is_dir() {
                Box::pin(Self::copy_dir_recursive(&path, &dest_path)).await?;
            } else {
                tokio::fs::copy(&path, &dest_path).await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_full_name() {
        let collection = Collection {
            namespace: "community".to_string(),
            name: "general".to_string(),
            version: "5.0.0".to_string(),
            path: PathBuf::from("/tmp/collections/community/general"),
            metadata: None,
        };

        assert_eq!(collection.full_name(), "community.general");
    }

    #[test]
    fn test_collection_info_full_name() {
        let info = CollectionInfo {
            namespace: "ansible".to_string(),
            name: "netcommon".to_string(),
            highest_version: None,
            description: None,
            download_count: 0,
            created_at: None,
            updated_at: None,
            deprecated: false,
        };

        assert_eq!(info.full_name(), "ansible.netcommon");
    }
}
