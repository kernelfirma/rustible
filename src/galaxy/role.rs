//! Role Installation Support
//!
//! This module handles the installation of Ansible roles from
//! Galaxy, local tarballs, or Git repositories.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::info;

use super::cache::GalaxyCache;
use super::client::GalaxyClient;
use super::error::{GalaxyError, GalaxyResult};
use super::integrity::{ChecksumAlgorithm, IntegrityVerifier};

/// Information about a role from Galaxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleInfo {
    /// Role ID
    #[serde(default)]
    pub id: u64,
    /// Role name
    #[serde(default)]
    pub name: String,
    /// Namespace/owner
    #[serde(default)]
    pub namespace: Option<String>,
    /// GitHub user (for v1 API)
    #[serde(default)]
    pub github_user: Option<String>,
    /// GitHub repo (for v1 API)
    #[serde(default)]
    pub github_repo: Option<String>,
    /// GitHub branch
    #[serde(default)]
    pub github_branch: Option<String>,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// Minimum Ansible version
    #[serde(default)]
    pub min_ansible_version: Option<String>,
    /// Download count
    #[serde(default)]
    pub download_count: u64,
    /// Star count
    #[serde(default)]
    pub stargazers_count: u64,
    /// Creation date
    #[serde(default)]
    pub created: Option<String>,
    /// Last modified date
    #[serde(default)]
    pub modified: Option<String>,
    /// Deprecated flag
    #[serde(default)]
    pub is_deprecated: bool,
    /// Summary fields
    #[serde(default)]
    pub summary_fields: Option<RoleSummaryFields>,
}

impl RoleInfo {
    /// Get the full name (owner.name)
    pub fn full_name(&self) -> String {
        let owner = self
            .namespace
            .as_ref()
            .or(self.github_user.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        format!("{}.{}", owner, self.name)
    }

    /// Get the download URL for the role
    pub fn download_url(&self) -> Option<String> {
        let user = self.github_user.as_ref()?;
        let repo = self.github_repo.as_ref()?;
        let branch = self
            .github_branch.as_deref()
            .unwrap_or("master");

        Some(format!(
            "https://github.com/{}/{}/archive/{}.tar.gz",
            user, repo, branch
        ))
    }
}

/// Summary fields for a role
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleSummaryFields {
    /// Dependencies
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Platforms
    #[serde(default)]
    pub platforms: Vec<RolePlatform>,
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Versions
    #[serde(default)]
    pub versions: Vec<RoleVersion>,
}

/// Platform information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePlatform {
    /// Platform name
    #[serde(default)]
    pub name: String,
    /// Platform release
    #[serde(default)]
    pub release: String,
}

/// Version information for a role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleVersion {
    /// Version name
    #[serde(default)]
    pub name: String,
    /// Version ID
    #[serde(default)]
    pub id: u64,
    /// Release date
    #[serde(default)]
    pub release_date: Option<String>,
}

/// Represents an installed role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalaxyRole {
    /// Role name
    pub name: String,
    /// Owner/namespace
    pub owner: String,
    /// Version
    pub version: Option<String>,
    /// Installation path
    pub path: PathBuf,
    /// Galaxy metadata
    #[serde(default)]
    pub galaxy_info: Option<GalaxyRoleInfo>,
}

impl GalaxyRole {
    /// Get the full name (owner.name)
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.owner, self.name)
    }

    /// Load role from an installed path
    pub async fn from_path(path: impl AsRef<Path>) -> GalaxyResult<Self> {
        let path = path.as_ref();
        let meta_path = path.join("meta/main.yml");

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let (owner, role_name) = if name.contains('.') {
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            (parts[0].to_string(), parts[1].to_string())
        } else {
            ("unknown".to_string(), name)
        };

        let galaxy_info = if meta_path.exists() {
            let content = tokio::fs::read_to_string(&meta_path).await?;
            let meta: RoleMetaMain = serde_yaml::from_str(&content)?;
            meta.galaxy_info
        } else {
            None
        };

        Ok(Self {
            name: role_name,
            owner,
            version: None,
            path: path.to_path_buf(),
            galaxy_info,
        })
    }
}

/// Role meta/main.yml structure
#[derive(Debug, Deserialize)]
struct RoleMetaMain {
    #[serde(default)]
    galaxy_info: Option<GalaxyRoleInfo>,
    #[serde(default)]
    dependencies: Vec<serde_yaml::Value>,
}

/// Galaxy role info from meta/main.yml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GalaxyRoleInfo {
    /// Role name
    #[serde(default)]
    pub role_name: Option<String>,
    /// Author
    #[serde(default)]
    pub author: Option<String>,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// Company
    #[serde(default)]
    pub company: Option<String>,
    /// License
    #[serde(default)]
    pub license: Option<String>,
    /// Minimum Ansible version
    #[serde(default)]
    pub min_ansible_version: Option<String>,
    /// Platforms
    #[serde(default)]
    pub platforms: Vec<RolePlatform>,
    /// Galaxy tags
    #[serde(default)]
    pub galaxy_tags: Vec<String>,
}

/// Handles role installation
pub struct RoleInstaller {
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

impl RoleInstaller {
    /// Create a new role installer
    pub fn new(client: Arc<GalaxyClient>, cache: Arc<GalaxyCache>) -> Self {
        let default_path = dirs::home_dir()
            .map(|h| h.join(".ansible/roles"))
            .unwrap_or_else(|| PathBuf::from("./roles"));

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

    /// Install a role
    pub async fn install(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        let dest = dest.unwrap_or_else(|| self.default_path.clone());

        // Parse role name
        let (owner, role_name) = if name.contains('.') {
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            (parts[0], parts[1])
        } else {
            return Err(GalaxyError::InvalidRoleName {
                name: name.to_string(),
                reason: "Role name should be in 'owner.name' or 'name' format".to_string(),
            });
        };

        let role_path = dest.join(format!("{}.{}", owner, role_name));

        // Check if already installed
        if role_path.exists() && !self.force {
            if let Ok(installed) = GalaxyRole::from_path(&role_path).await {
                if version.is_none() || version == installed.version.as_deref() {
                    info!("Role {} already installed at {}", name, role_path.display());
                    return Ok(role_path);
                }
            }
        }

        // Get role info from Galaxy
        let role_info = self.client.get_role_info(name).await?;

        // Determine version to install
        let target_version = version.map(|v| v.to_string()).or_else(|| {
            role_info
                .summary_fields
                .as_ref()
                .and_then(|sf| sf.versions.first())
                .map(|v| v.name.clone())
        });

        let version_str = target_version.as_deref().unwrap_or("latest");

        // Try cache first
        if let Some(cached) = self.cache.get_role(name, version_str).await? {
            info!("Using cached role {}-{}", name, version_str);
            return self
                .extract_and_install(&cached.path, &dest, owner, role_name)
                .await;
        }

        // Get download URL
        let download_url =
            role_info
                .download_url()
                .ok_or_else(|| GalaxyError::RoleInstallFailed {
                    name: name.to_string(),
                    message: "No download URL available for role".to_string(),
                })?;

        // Download role
        info!("Downloading role {}-{}", name, version_str);
        let data = self.download_role(&download_url).await?;

        // Compute checksum for cache
        let checksum = IntegrityVerifier::compute_checksum(&data, ChecksumAlgorithm::Sha256);

        // Save to cache
        let cache_path = self
            .cache
            .store_role(name, version_str, &data, Some(checksum))
            .await?;

        // Extract and install
        self.extract_and_install(&cache_path, &dest, owner, role_name)
            .await
    }

    /// Download a role from URL
    async fn download_role(&self, url: &str) -> GalaxyResult<bytes::Bytes> {
        let response = reqwest::get(url)
            .await
            .map_err(|e| GalaxyError::http_error_with_source("Failed to download role", e))?;

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to download role: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| GalaxyError::http_error_with_source("Failed to read role data", e))?;

        Ok(bytes)
    }

    /// Install a role from cache (offline mode)
    pub async fn install_from_cache(
        &self,
        name: &str,
        version: Option<&str>,
        dest: Option<PathBuf>,
    ) -> GalaxyResult<PathBuf> {
        let dest = dest.unwrap_or_else(|| self.default_path.clone());

        // Parse role name
        let (owner, role_name) = if name.contains('.') {
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            (parts[0], parts[1])
        } else {
            ("unknown", name)
        };

        // Get from cache
        let cached = if let Some(v) = version {
            self.cache.get_role(name, v).await?
        } else {
            self.cache.get_latest_role(name).await?
        };

        let cached = cached.ok_or_else(|| GalaxyError::NoCachedVersion {
            name: name.to_string(),
        })?;

        info!("Installing cached role {}-{}", name, cached.version);

        self.extract_and_install(&cached.path, &dest, owner, role_name)
            .await
    }

    /// Extract a role tarball and install it
    async fn extract_and_install(
        &self,
        tarball: &Path,
        dest: &Path,
        owner: &str,
        name: &str,
    ) -> GalaxyResult<PathBuf> {
        let role_path = dest.join(format!("{}.{}", owner, name));

        // Create destination directory
        if role_path.exists() {
            tokio::fs::remove_dir_all(&role_path).await.ok();
        }
        tokio::fs::create_dir_all(&role_path).await.map_err(|e| {
            GalaxyError::RoleInstallFailed {
                name: format!("{}.{}", owner, name),
                message: format!("Failed to create directory: {}", e),
            }
        })?;

        // Extract tarball
        let tarball_data = tokio::fs::read(tarball).await?;
        let gz = flate2::read::GzDecoder::new(&tarball_data[..]);
        let mut archive = tar::Archive::new(gz);

        // Extract to temp directory first
        let temp_dir = tempfile::tempdir()?;
        archive
            .unpack(temp_dir.path())
            .map_err(|e| GalaxyError::ExtractionFailed {
                path: tarball.to_path_buf(),
                message: e.to_string(),
            })?;

        // Find the extracted directory (usually repo-branch or similar)
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
        Self::copy_dir_recursive(&extracted_dir, &role_path).await?;

        info!(
            "Installed role {}.{} to {}",
            owner,
            name,
            role_path.display()
        );

        Ok(role_path)
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
    use crate::config::GalaxyConfig;

    #[test]
    fn test_role_info_full_name() {
        let info = RoleInfo {
            id: 1,
            name: "nginx".to_string(),
            namespace: None,
            github_user: Some("geerlingguy".to_string()),
            github_repo: Some("ansible-role-nginx".to_string()),
            github_branch: Some("master".to_string()),
            description: None,
            min_ansible_version: None,
            download_count: 0,
            stargazers_count: 0,
            created: None,
            modified: None,
            is_deprecated: false,
            summary_fields: None,
        };

        assert_eq!(info.full_name(), "geerlingguy.nginx");
    }

    #[test]
    fn test_role_info_download_url() {
        let info = RoleInfo {
            id: 1,
            name: "nginx".to_string(),
            namespace: None,
            github_user: Some("geerlingguy".to_string()),
            github_repo: Some("ansible-role-nginx".to_string()),
            github_branch: Some("main".to_string()),
            description: None,
            min_ansible_version: None,
            download_count: 0,
            stargazers_count: 0,
            created: None,
            modified: None,
            is_deprecated: false,
            summary_fields: None,
        };

        let url = info.download_url().unwrap();
        assert!(url.contains("github.com"));
        assert!(url.contains("geerlingguy"));
        assert!(url.contains("ansible-role-nginx"));
        assert!(url.contains("main.tar.gz"));
    }

    #[test]
    fn test_galaxy_role_full_name() {
        let role = GalaxyRole {
            name: "nginx".to_string(),
            owner: "geerlingguy".to_string(),
            version: Some("2.8.0".to_string()),
            path: PathBuf::from("/tmp/roles/geerlingguy.nginx"),
            galaxy_info: None,
        };

        assert_eq!(role.full_name(), "geerlingguy.nginx");
    }

    fn build_tar_gz(root_name: &str, files: &[(&str, &str)]) -> Vec<u8> {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root_path = temp_dir.path().join(root_name);
        std::fs::create_dir_all(&root_path).expect("create root");
        for (rel, contents) in files {
            let file_path = root_path.join(rel);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent).expect("create parents");
            }
            std::fs::write(&file_path, contents).expect("write file");
        }

        let mut buffer = Vec::new();
        let encoder =
            flate2::write::GzEncoder::new(&mut buffer, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        tar.append_dir_all(root_name, &root_path)
            .expect("append dir");
        let encoder = tar.into_inner().expect("finish tar");
        encoder.finish().expect("finish gzip");
        buffer
    }

    #[tokio::test]
    async fn test_galaxy_role_from_path_with_meta() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let role_dir = temp_dir.path().join("geerlingguy.nginx");
        let meta_dir = role_dir.join("meta");
        std::fs::create_dir_all(&meta_dir).expect("create meta");
        std::fs::write(
            meta_dir.join("main.yml"),
            r#"galaxy_info:
  role_name: nginx
  author: "Test Author"
  platforms:
    - name: ubuntu
      release: jammy
  galaxy_tags:
    - web
"#,
        )
        .expect("write meta");

        let role = GalaxyRole::from_path(&role_dir).await.expect("role");
        assert_eq!(role.owner, "geerlingguy");
        assert_eq!(role.name, "nginx");
        assert!(role.galaxy_info.is_some());
        assert_eq!(
            role.galaxy_info
                .as_ref()
                .and_then(|info| info.role_name.clone()),
            Some("nginx".to_string())
        );
    }

    #[tokio::test]
    async fn test_role_install_from_cache() {
        let cache_dir = tempfile::tempdir().expect("tempdir");
        let cache_config = crate::galaxy::GalaxyCacheConfig {
            cache_dir: cache_dir.path().to_path_buf(),
            max_size: 0,
            ttl_seconds: 0,
        };
        let cache = Arc::new(GalaxyCache::new(cache_config).expect("cache"));
        let client = Arc::new(GalaxyClient::new(&GalaxyConfig::default()).expect("client"));
        let installer = RoleInstaller::new(Arc::clone(&client), Arc::clone(&cache));

        let tarball = build_tar_gz(
            "geerlingguy-nginx-1.0.0",
            &[("tasks/main.yml", "hello role")],
        );
        let cache_path = cache
            .store_role("geerlingguy.nginx", "1.0.0", &tarball, None)
            .await
            .expect("store cache");
        assert!(cache_path.exists());

        let dest_dir = tempfile::tempdir().expect("tempdir");
        let installed_path = installer
            .install_from_cache(
                "geerlingguy.nginx",
                Some("1.0.0"),
                Some(dest_dir.path().to_path_buf()),
            )
            .await
            .expect("install from cache");

        let task_file = installed_path.join("tasks/main.yml");
        assert!(task_file.exists(), "expected extracted file");
    }
}
