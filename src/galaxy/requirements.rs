//! Requirements File Parsing
//!
//! This module handles parsing of requirements.yml files for
//! installing collections and roles from Ansible Galaxy.
//!
//! # Supported Formats
//!
//! ## Collections
//! ```yaml
//! collections:
//!   - name: community.general
//!     version: ">=5.0.0"
//!   - name: ansible.netcommon
//!     source: https://galaxy.ansible.com
//! ```
//!
//! ## Roles
//! ```yaml
//! roles:
//!   - name: geerlingguy.nginx
//!     version: "2.8.0"
//!   - src: https://github.com/user/role
//!     name: my_role
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::debug;

use super::error::{GalaxyError, GalaxyResult};

/// Parsed requirements file
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequirementsFile {
    /// List of collections to install
    #[serde(default)]
    pub collections: Vec<Requirement>,
    /// List of roles to install
    #[serde(default)]
    pub roles: Vec<Requirement>,
}

impl RequirementsFile {
    /// Create an empty requirements file
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a requirements file from a path
    pub async fn from_path(path: impl AsRef<Path>) -> GalaxyResult<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Err(GalaxyError::RequirementsFileNotFound {
                path: path.to_path_buf(),
            });
        }

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            GalaxyError::RequirementsParseError {
                path: path.to_path_buf(),
                message: format!("Failed to read file: {}", e),
            }
        })?;

        Self::from_str(&content, path)
    }

    /// Parse requirements from a string
    pub fn from_str(content: &str, path: impl AsRef<Path>) -> GalaxyResult<Self> {
        let path = path.as_ref();

        // Try to parse as structured format first
        let parsed: Result<RequirementsRaw, _> = serde_yaml::from_str(content);

        match parsed {
            Ok(raw) => {
                debug!(
                    "Parsed requirements file: {} collections, {} roles",
                    raw.collections.len(),
                    raw.roles.len()
                );
                Self::from_raw(raw)
            }
            Err(e) => {
                // Try legacy format (list of roles only)
                let legacy: Result<Vec<RequirementRaw>, _> = serde_yaml::from_str(content);
                if let Ok(roles) = legacy {
                    debug!("Parsed legacy requirements format: {} roles", roles.len());
                    let requirements: Vec<Requirement> =
                        roles.into_iter().map(|r| r.into()).collect();
                    Ok(Self {
                        collections: Vec::new(),
                        roles: requirements,
                    })
                } else {
                    Err(GalaxyError::RequirementsParseError {
                        path: path.to_path_buf(),
                        message: e.to_string(),
                    })
                }
            }
        }
    }

    /// Convert from raw parsed format
    fn from_raw(raw: RequirementsRaw) -> GalaxyResult<Self> {
        let collections: Vec<Requirement> = raw.collections.into_iter().map(|r| r.into()).collect();

        let roles: Vec<Requirement> = raw.roles.into_iter().map(|r| r.into()).collect();

        Ok(Self { collections, roles })
    }

    /// Add a collection requirement
    pub fn add_collection(&mut self, requirement: Requirement) {
        self.collections.push(requirement);
    }

    /// Add a role requirement
    pub fn add_role(&mut self, requirement: Requirement) {
        self.roles.push(requirement);
    }

    /// Check if the requirements file is empty
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty() && self.roles.is_empty()
    }

    /// Get the total number of requirements
    pub fn len(&self) -> usize {
        self.collections.len() + self.roles.len()
    }

    /// Serialize to YAML string
    pub fn to_yaml(&self) -> GalaxyResult<String> {
        serde_yaml::to_string(self)
            .map_err(|e| GalaxyError::Other(format!("Failed to serialize requirements: {}", e)))
    }

    /// Write to a file
    pub async fn write_to_file(&self, path: impl AsRef<Path>) -> GalaxyResult<()> {
        let content = self.to_yaml()?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }
}

/// A single requirement (collection or role)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    /// Name of the requirement (e.g., "community.general" or "geerlingguy.nginx")
    pub name: String,
    /// Version constraint (e.g., ">=1.0.0", "1.2.3", "*")
    #[serde(default)]
    pub version: Option<String>,
    /// Source URL (alternative Galaxy server or Git URL)
    #[serde(default)]
    pub source: Option<RequirementSource>,
    /// Requirement type (collection or role)
    #[serde(default)]
    pub requirement_type: RequirementType,
    /// Whether to include pre-release versions
    #[serde(default)]
    pub include_prerelease: bool,
}

impl Requirement {
    /// Create a new collection requirement
    pub fn collection(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            source: None,
            requirement_type: RequirementType::Collection,
            include_prerelease: false,
        }
    }

    /// Create a new role requirement
    pub fn role(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            source: None,
            requirement_type: RequirementType::Role,
            include_prerelease: false,
        }
    }

    /// Set the version constraint
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the source
    pub fn with_source(mut self, source: RequirementSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Set include prerelease flag
    pub fn with_prerelease(mut self, include: bool) -> Self {
        self.include_prerelease = include;
        self
    }
}

/// Source for a requirement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequirementSource {
    /// Galaxy server URL
    Galaxy(String),
    /// Git repository URL
    Git {
        url: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        commit: Option<String>,
    },
    /// Local file path
    Local(String),
    /// URL to tarball
    Url(String),
}

impl RequirementSource {
    /// Create a Galaxy source
    pub fn galaxy(url: impl Into<String>) -> Self {
        Self::Galaxy(url.into())
    }

    /// Create a Git source
    pub fn git(url: impl Into<String>) -> Self {
        Self::Git {
            url: url.into(),
            branch: None,
            tag: None,
            commit: None,
        }
    }

    /// Create a Git source with a branch
    pub fn git_branch(url: impl Into<String>, branch: impl Into<String>) -> Self {
        Self::Git {
            url: url.into(),
            branch: Some(branch.into()),
            tag: None,
            commit: None,
        }
    }

    /// Create a Git source with a tag
    pub fn git_tag(url: impl Into<String>, tag: impl Into<String>) -> Self {
        Self::Git {
            url: url.into(),
            branch: None,
            tag: Some(tag.into()),
            commit: None,
        }
    }

    /// Create a local source
    pub fn local(path: impl Into<String>) -> Self {
        Self::Local(path.into())
    }

    /// Create a URL source
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }
}

/// Type of requirement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RequirementType {
    /// Collection requirement
    #[default]
    Collection,
    /// Role requirement
    Role,
}

// Raw parsing structures

#[derive(Debug, Deserialize)]
struct RequirementsRaw {
    #[serde(default)]
    collections: Vec<RequirementRaw>,
    #[serde(default)]
    roles: Vec<RequirementRaw>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequirementRaw {
    /// Simple string format: "namespace.name"
    Simple(String),
    /// Full format with options
    Full(RequirementFull),
}

#[derive(Debug, Deserialize)]
struct RequirementFull {
    /// Name of the collection/role (optional - can use src as name if not provided)
    #[serde(default)]
    name: Option<String>,
    /// Version constraint
    #[serde(default)]
    version: Option<String>,
    /// Source URL
    #[serde(default)]
    source: Option<String>,
    /// Git repository URL (for roles)
    #[serde(default)]
    src: Option<String>,
    /// SCM type (git)
    #[serde(default)]
    scm: Option<String>,
    /// Include pre-release versions
    #[serde(default)]
    include_prerelease: bool,
}

impl From<RequirementRaw> for Requirement {
    fn from(raw: RequirementRaw) -> Self {
        match raw {
            RequirementRaw::Simple(name) => Requirement {
                name,
                version: None,
                source: None,
                requirement_type: RequirementType::Collection,
                include_prerelease: false,
            },
            RequirementRaw::Full(full) => {
                // Build source from src or source field
                let source = if let Some(ref git_src) = full.src {
                    Some(RequirementSource::Git {
                        url: git_src.clone(),
                        branch: None,
                        tag: None,
                        commit: None,
                    })
                } else if let Some(url) = full.source {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        if url.ends_with(".git") || full.scm.as_deref() == Some("git") {
                            Some(RequirementSource::Git {
                                url,
                                branch: None,
                                tag: None,
                                commit: None,
                            })
                        } else if url.contains("galaxy") {
                            Some(RequirementSource::Galaxy(url))
                        } else {
                            Some(RequirementSource::Url(url))
                        }
                    } else {
                        Some(RequirementSource::Local(url))
                    }
                } else {
                    None
                };

                // Use explicit name if provided, otherwise fall back to src URL
                let name = full
                    .name
                    .unwrap_or_else(|| full.src.unwrap_or_else(|| "unknown".to_string()));

                Requirement {
                    name,
                    version: full.version,
                    source,
                    requirement_type: RequirementType::Collection,
                    include_prerelease: full.include_prerelease,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_collections() {
        let yaml = r#"
collections:
  - community.general
  - ansible.netcommon
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.collections.len(), 2);
        assert_eq!(requirements.collections[0].name, "community.general");
        assert_eq!(requirements.collections[1].name, "ansible.netcommon");
    }

    #[test]
    fn test_parse_collections_with_version() {
        let yaml = r#"
collections:
  - name: community.general
    version: ">=5.0.0"
  - name: ansible.netcommon
    version: "2.0.0"
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.collections.len(), 2);
        assert_eq!(
            requirements.collections[0].version,
            Some(">=5.0.0".to_string())
        );
        assert_eq!(
            requirements.collections[1].version,
            Some("2.0.0".to_string())
        );
    }

    #[test]
    fn test_parse_roles() {
        let yaml = r#"
roles:
  - name: geerlingguy.nginx
    version: "2.8.0"
  - name: geerlingguy.docker
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.roles.len(), 2);
        assert_eq!(requirements.roles[0].name, "geerlingguy.nginx");
        assert_eq!(requirements.roles[0].version, Some("2.8.0".to_string()));
    }

    #[test]
    fn test_parse_mixed() {
        let yaml = r#"
collections:
  - community.general

roles:
  - geerlingguy.nginx
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.collections.len(), 1);
        assert_eq!(requirements.roles.len(), 1);
    }

    #[test]
    fn test_parse_legacy_format() {
        let yaml = r#"
- name: geerlingguy.nginx
  version: "2.8.0"
- geerlingguy.docker
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.roles.len(), 2);
        assert_eq!(requirements.roles[0].name, "geerlingguy.nginx");
    }

    #[test]
    fn test_parse_git_source() {
        let yaml = r#"
roles:
  - src: https://github.com/user/role.git
    name: my_role
    scm: git
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.roles.len(), 1);
        assert!(matches!(
            requirements.roles[0].source,
            Some(RequirementSource::Git { .. })
        ));
    }

    #[test]
    fn test_requirement_builders() {
        let collection = Requirement::collection("community.general").with_version(">=5.0.0");
        assert_eq!(collection.name, "community.general");
        assert_eq!(collection.version, Some(">=5.0.0".to_string()));
        assert_eq!(collection.requirement_type, RequirementType::Collection);

        let role = Requirement::role("geerlingguy.nginx").with_source(RequirementSource::git_tag(
            "https://github.com/geerlingguy/ansible-role-nginx",
            "2.8.0",
        ));
        assert_eq!(role.name, "geerlingguy.nginx");
        assert!(matches!(role.source, Some(RequirementSource::Git { .. })));
    }

    #[test]
    fn test_requirements_file_empty() {
        let requirements = RequirementsFile::new();
        assert!(requirements.is_empty());
        assert_eq!(requirements.len(), 0);
    }

    #[test]
    fn test_requirements_file_add() {
        let mut requirements = RequirementsFile::new();
        requirements.add_collection(Requirement::collection("community.general"));
        requirements.add_role(Requirement::role("geerlingguy.nginx"));

        assert!(!requirements.is_empty());
        assert_eq!(requirements.len(), 2);
        assert_eq!(requirements.collections.len(), 1);
        assert_eq!(requirements.roles.len(), 1);
    }

    #[test]
    fn test_to_yaml() {
        let mut requirements = RequirementsFile::new();
        requirements
            .add_collection(Requirement::collection("community.general").with_version("5.0.0"));

        let yaml = requirements.to_yaml().unwrap();
        assert!(yaml.contains("community.general"));
        assert!(yaml.contains("5.0.0"));
    }

    #[test]
    fn test_parse_source_variants() {
        let yaml = r#"
collections:
  - name: community.general
    source: ./local/path
  - name: community.general
    source: https://galaxy.ansible.com
  - name: community.general
    source: https://example.com/archive.tar.gz
  - name: community.general
    source: https://github.com/org/repo.git
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.collections.len(), 4);
        assert!(matches!(
            requirements.collections[0].source,
            Some(RequirementSource::Local(_))
        ));
        assert!(matches!(
            requirements.collections[1].source,
            Some(RequirementSource::Galaxy(_))
        ));
        assert!(matches!(
            requirements.collections[2].source,
            Some(RequirementSource::Url(_))
        ));
        assert!(matches!(
            requirements.collections[3].source,
            Some(RequirementSource::Git { .. })
        ));
    }

    #[test]
    fn test_parse_source_unknown_name() {
        let yaml = r#"
collections:
  - source: https://example.com/custom.tar.gz
"#;
        let requirements = RequirementsFile::from_str(yaml, "test.yml").unwrap();
        assert_eq!(requirements.collections.len(), 1);
        assert_eq!(requirements.collections[0].name, "unknown");
        assert!(matches!(
            requirements.collections[0].source,
            Some(RequirementSource::Url(_))
        ));
    }

    #[tokio::test]
    async fn test_from_path_not_found() {
        let result = RequirementsFile::from_path("missing-requirements.yml").await;
        assert!(matches!(
            result,
            Err(GalaxyError::RequirementsFileNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn test_write_to_file_round_trip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("requirements.yml");

        let mut requirements = RequirementsFile::new();
        requirements
            .add_collection(Requirement::collection("community.general").with_version("5.0.0"));
        requirements
            .write_to_file(&path)
            .await
            .expect("write requirements");

        let loaded = RequirementsFile::from_path(&path)
            .await
            .expect("read requirements");
        assert_eq!(loaded.collections.len(), 1);
        assert_eq!(loaded.collections[0].name, "community.general");
    }
}
