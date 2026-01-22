//! Collection metadata parsing (galaxy.yml)
//!
//! This module handles parsing of the galaxy.yml file that provides
//! collection metadata including version, dependencies, and build information.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::CollectionResult;

/// Collection metadata from galaxy.yml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalaxyMetadata {
    /// Collection namespace
    pub namespace: String,

    /// Collection name
    pub name: String,

    /// Collection version (semver)
    pub version: String,

    /// Short description
    #[serde(default)]
    pub description: Option<String>,

    /// Long description (README content)
    #[serde(default)]
    pub readme: Option<String>,

    /// Collection authors
    #[serde(default)]
    pub authors: Vec<String>,

    /// License identifier (SPDX)
    #[serde(default)]
    pub license: Option<String>,

    /// License file path
    #[serde(default)]
    pub license_file: Option<String>,

    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,

    /// Collection dependencies
    #[serde(default)]
    pub dependencies: HashMap<String, String>,

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

    /// Required Ansible version
    #[serde(default)]
    pub requires_ansible: Option<String>,

    /// Extra files to include in build
    #[serde(default)]
    pub manifest: Option<ManifestConfig>,
}

/// Manifest configuration for collection builds
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestConfig {
    /// Files to include
    #[serde(default)]
    pub files: Vec<String>,

    /// Directives for manifest generation
    #[serde(default)]
    pub directives: Vec<String>,
}

impl GalaxyMetadata {
    /// Loads galaxy.yml from a path
    pub fn from_file(path: impl AsRef<Path>) -> CollectionResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let metadata: GalaxyMetadata = serde_yaml::from_str(&content)?;
        Ok(metadata)
    }

    /// Parses galaxy.yml from a string
    pub fn from_str(yaml: &str) -> CollectionResult<Self> {
        let metadata: GalaxyMetadata = serde_yaml::from_str(yaml)?;
        Ok(metadata)
    }

    /// Returns the fully qualified collection name
    pub fn fqn(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// Validates the metadata
    pub fn validate(&self) -> CollectionResult<()> {
        use super::CollectionError;

        if self.namespace.is_empty() {
            return Err(CollectionError::InvalidGalaxyYml(
                "namespace is required".to_string(),
            ));
        }

        if self.name.is_empty() {
            return Err(CollectionError::InvalidGalaxyYml(
                "name is required".to_string(),
            ));
        }

        if self.version.is_empty() {
            return Err(CollectionError::InvalidGalaxyYml(
                "version is required".to_string(),
            ));
        }

        // Validate version is valid semver
        if semver::Version::parse(&self.version).is_err() {
            // Try parsing as a partial version
            if !is_valid_version(&self.version) {
                return Err(CollectionError::InvalidGalaxyYml(format!(
                    "invalid version: {}",
                    self.version
                )));
            }
        }

        Ok(())
    }
}

impl std::str::FromStr for GalaxyMetadata {
    type Err = super::CollectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GalaxyMetadata::from_str(s)
    }
}

/// Checks if a version string is valid (allows partial semver)
fn is_valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return false;
    }

    for part in parts {
        if part.parse::<u64>().is_err() {
            // Allow pre-release suffixes like "1.0.0-beta"
            if !part.contains('-') || part.split('-').next().unwrap().parse::<u64>().is_err() {
                return false;
            }
        }
    }

    true
}

/// Extended collection metadata including computed fields
#[derive(Debug, Clone, Default)]
pub struct CollectionMetadata {
    /// Galaxy metadata from galaxy.yml
    pub galaxy: Option<GalaxyMetadata>,

    /// Collection type (content, network, etc.)
    pub collection_type: CollectionType,

    /// Whether this is a local development collection
    pub is_local: bool,

    /// Installation source
    pub source: CollectionSource,

    /// Installation timestamp
    pub installed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Type of collection content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionType {
    /// General purpose collection
    #[default]
    Content,
    /// Network-focused collection
    Network,
    /// Security-focused collection
    Security,
    /// Cloud provider collection
    Cloud,
    /// Container-focused collection
    Container,
    /// Monitoring/observability collection
    Monitoring,
    /// Database collection
    Database,
}

/// Source of collection installation
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSource {
    /// Installed from Galaxy
    #[default]
    Galaxy,
    /// Installed from Automation Hub
    AutomationHub,
    /// Installed from Git repository
    Git {
        url: String,
        version: Option<String>,
    },
    /// Installed from local path
    Local { path: String },
    /// Built-in collection (ansible.builtin)
    Builtin,
}

impl CollectionMetadata {
    /// Creates metadata from a galaxy.yml file
    pub fn from_galaxy(galaxy: GalaxyMetadata) -> Self {
        Self {
            galaxy: Some(galaxy),
            collection_type: CollectionType::Content,
            is_local: false,
            source: CollectionSource::Galaxy,
            installed_at: None,
        }
    }

    /// Marks as locally developed
    pub fn with_local(mut self, is_local: bool) -> Self {
        self.is_local = is_local;
        if is_local {
            self.source = CollectionSource::Local {
                path: String::new(),
            };
        }
        self
    }

    /// Sets the collection source
    pub fn with_source(mut self, source: CollectionSource) -> Self {
        self.source = source;
        self
    }

    /// Returns the namespace if available
    pub fn namespace(&self) -> Option<&str> {
        self.galaxy.as_ref().map(|g| g.namespace.as_str())
    }

    /// Returns the name if available
    pub fn name(&self) -> Option<&str> {
        self.galaxy.as_ref().map(|g| g.name.as_str())
    }

    /// Returns the version if available
    pub fn version(&self) -> Option<&str> {
        self.galaxy.as_ref().map(|g| g.version.as_str())
    }

    /// Returns dependencies if available
    pub fn dependencies(&self) -> HashMap<String, String> {
        self.galaxy
            .as_ref()
            .map(|g| g.dependencies.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_galaxy_yml() {
        let yaml = r#"
namespace: community
name: general
version: 4.0.0
description: Modules for general system administration
authors:
  - Ansible Community
license: GPL-3.0-or-later
tags:
  - linux
  - tools
dependencies:
  ansible.netcommon: ">=2.0.0"
  ansible.utils: ">=2.0.0"
repository: https://github.com/ansible-collections/community.general
"#;

        let metadata = GalaxyMetadata::from_str(yaml).unwrap();
        assert_eq!(metadata.namespace, "community");
        assert_eq!(metadata.name, "general");
        assert_eq!(metadata.version, "4.0.0");
        assert_eq!(metadata.authors, vec!["Ansible Community"]);
        assert_eq!(metadata.dependencies.len(), 2);
        assert!(metadata.dependencies.contains_key("ansible.netcommon"));
    }

    #[test]
    fn test_galaxy_fqn() {
        let metadata = GalaxyMetadata {
            namespace: "ansible".to_string(),
            name: "builtin".to_string(),
            version: "2.14.0".to_string(),
            description: None,
            readme: None,
            authors: vec![],
            license: None,
            license_file: None,
            tags: vec![],
            dependencies: HashMap::new(),
            repository: None,
            documentation: None,
            homepage: None,
            issues: None,
            build_ignore: vec![],
            requires_ansible: None,
            manifest: None,
        };

        assert_eq!(metadata.fqn(), "ansible.builtin");
    }

    #[test]
    fn test_validate_metadata() {
        let valid = GalaxyMetadata {
            namespace: "test".to_string(),
            name: "collection".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            readme: None,
            authors: vec![],
            license: None,
            license_file: None,
            tags: vec![],
            dependencies: HashMap::new(),
            repository: None,
            documentation: None,
            homepage: None,
            issues: None,
            build_ignore: vec![],
            requires_ansible: None,
            manifest: None,
        };
        assert!(valid.validate().is_ok());

        let mut invalid = valid.clone();
        invalid.namespace = String::new();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_is_valid_version() {
        assert!(is_valid_version("1.0.0"));
        assert!(is_valid_version("1.0"));
        assert!(is_valid_version("1"));
        assert!(is_valid_version("1.0.0-beta"));

        assert!(!is_valid_version(""));
        assert!(!is_valid_version("abc"));
        assert!(!is_valid_version("1.2.3.4"));
    }
}
