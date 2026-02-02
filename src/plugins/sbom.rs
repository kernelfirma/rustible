//! Software Bill of Materials (SBOM) generation for provider supply-chain security.
//!
//! Generates CycloneDX-compatible SBOM for installed providers and their dependencies,
//! supporting supply-chain verification and revocation.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// SBOM format version
const SBOM_SPEC_VERSION: &str = "1.5";

/// A Software Bill of Materials document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    /// SBOM format (CycloneDX)
    pub bom_format: String,
    /// Spec version
    pub spec_version: String,
    /// Unique serial number
    pub serial_number: String,
    /// SBOM version
    pub version: u32,
    /// Metadata about the SBOM generation
    pub metadata: SbomMetadata,
    /// Components (providers, modules, dependencies)
    pub components: Vec<SbomComponent>,
    /// Dependencies between components
    pub dependencies: Vec<SbomDependency>,
}

/// SBOM generation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomMetadata {
    /// Generation timestamp (ISO 8601)
    pub timestamp: String,
    /// Tool that generated the SBOM
    pub tool: SbomTool,
    /// The component this SBOM describes
    pub component: SbomComponent,
}

/// Tool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomTool {
    pub vendor: String,
    pub name: String,
    pub version: String,
}

/// A component in the SBOM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomComponent {
    /// Component type (library, application, framework)
    #[serde(rename = "type")]
    pub component_type: ComponentType,
    /// Component name
    pub name: String,
    /// Component version
    pub version: String,
    /// Package URL (purl)
    pub purl: Option<String>,
    /// SHA256 hash of the component
    pub sha256: Option<String>,
    /// License
    pub license: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Whether this component has been revoked
    #[serde(default)]
    pub revoked: bool,
    /// Revocation reason if revoked
    pub revocation_reason: Option<String>,
}

/// Component type classification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Application,
    Library,
    Framework,
    Module,
    Provider,
}

/// Dependency relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomDependency {
    /// The component that has dependencies
    #[serde(rename = "ref")]
    pub reference: String,
    /// Components it depends on
    pub depends_on: Vec<String>,
}

/// Revocation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationEntry {
    /// Component name
    pub name: String,
    /// Affected version range (semver)
    pub version_range: String,
    /// Reason for revocation
    pub reason: String,
    /// When the revocation was issued
    pub issued_at: String,
    /// Severity (critical, high, medium, low)
    pub severity: String,
    /// CVE identifier if applicable
    pub cve: Option<String>,
}

/// Revocation list for checking provider integrity
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RevocationList {
    /// Last updated timestamp
    pub last_updated: String,
    /// Revoked components
    pub entries: Vec<RevocationEntry>,
}

/// SBOM generator
pub struct SbomGenerator {
    /// Application name
    app_name: String,
    /// Application version
    app_version: String,
}

impl SbomGenerator {
    pub fn new(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            app_version: app_version.into(),
        }
    }

    /// Generate an SBOM from a list of provider components
    pub fn generate(&self, components: Vec<SbomComponent>, dependencies: Vec<SbomDependency>) -> Sbom {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let serial = format!("urn:uuid:{}", uuid::Uuid::new_v4());

        Sbom {
            bom_format: "CycloneDX".to_string(),
            spec_version: SBOM_SPEC_VERSION.to_string(),
            serial_number: serial,
            version: 1,
            metadata: SbomMetadata {
                timestamp,
                tool: SbomTool {
                    vendor: "Rustible".to_string(),
                    name: "rustible-sbom".to_string(),
                    version: self.app_version.clone(),
                },
                component: SbomComponent {
                    component_type: ComponentType::Application,
                    name: self.app_name.clone(),
                    version: self.app_version.clone(),
                    purl: None,
                    sha256: None,
                    license: Some("MIT".to_string()),
                    description: Some("Rustible configuration management tool".to_string()),
                    revoked: false,
                    revocation_reason: None,
                },
            },
            components,
            dependencies,
        }
    }

    /// Write SBOM to a JSON file
    pub fn write_json(&self, sbom: &Sbom, path: &Path) -> Result<(), SbomError> {
        let json = serde_json::to_string_pretty(sbom)
            .map_err(|e| SbomError::SerializationError(e.to_string()))?;
        std::fs::write(path, json)
            .map_err(|e| SbomError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Check components against a revocation list
    pub fn check_revocations(
        &self,
        sbom: &Sbom,
        revocation_list: &RevocationList,
    ) -> Vec<RevocationMatch> {
        let mut matches = Vec::new();
        for component in &sbom.components {
            for entry in &revocation_list.entries {
                if component.name == entry.name && version_in_range(&component.version, &entry.version_range) {
                    matches.push(RevocationMatch {
                        component: component.clone(),
                        revocation: entry.clone(),
                    });
                }
            }
        }
        matches
    }
}

/// A match between a component and a revocation entry
#[derive(Debug, Clone)]
pub struct RevocationMatch {
    pub component: SbomComponent,
    pub revocation: RevocationEntry,
}

/// SBOM-related errors
#[derive(Debug, thiserror::Error)]
pub enum SbomError {
    #[error("SBOM serialization error: {0}")]
    SerializationError(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Revocation check failed: {0}")]
    RevocationCheckFailed(String),
}

/// Simple version range check (supports exact match and wildcard *)
fn version_in_range(version: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    if let Some(prefix) = range.strip_suffix(".*") {
        return version.starts_with(prefix);
    }
    if let Some(min) = range.strip_prefix(">=") {
        return version >= min;
    }
    if let Some(max) = range.strip_prefix("<=") {
        return version <= max;
    }
    version == range
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_in_range_exact() {
        assert!(version_in_range("1.2.3", "1.2.3"));
        assert!(!version_in_range("1.2.4", "1.2.3"));
    }

    #[test]
    fn test_version_in_range_wildcard() {
        assert!(version_in_range("1.2.3", "*"));
        assert!(version_in_range("1.2.3", "1.2.*"));
        assert!(!version_in_range("1.3.0", "1.2.*"));
    }

    #[test]
    fn test_generate_sbom() {
        let gen = SbomGenerator::new("test-app", "0.1.0");
        let component = SbomComponent {
            component_type: ComponentType::Provider,
            name: "aws-provider".to_string(),
            version: "1.0.0".to_string(),
            purl: Some("pkg:rustible/aws-provider@1.0.0".to_string()),
            sha256: Some("abc123".to_string()),
            license: Some("MIT".to_string()),
            description: Some("AWS cloud provider".to_string()),
            revoked: false,
            revocation_reason: None,
        };
        let sbom = gen.generate(vec![component], vec![]);
        assert_eq!(sbom.bom_format, "CycloneDX");
        assert_eq!(sbom.components.len(), 1);
        assert_eq!(sbom.components[0].name, "aws-provider");
    }

    #[test]
    fn test_check_revocations() {
        let gen = SbomGenerator::new("test-app", "0.1.0");
        let component = SbomComponent {
            component_type: ComponentType::Provider,
            name: "bad-provider".to_string(),
            version: "1.2.3".to_string(),
            purl: None,
            sha256: None,
            license: None,
            description: None,
            revoked: false,
            revocation_reason: None,
        };
        let sbom = gen.generate(vec![component], vec![]);
        let revocation_list = RevocationList {
            last_updated: "2025-01-01T00:00:00Z".to_string(),
            entries: vec![RevocationEntry {
                name: "bad-provider".to_string(),
                version_range: "1.2.*".to_string(),
                reason: "Security vulnerability".to_string(),
                issued_at: "2025-01-01T00:00:00Z".to_string(),
                severity: "critical".to_string(),
                cve: Some("CVE-2025-0001".to_string()),
            }],
        };
        let matches = gen.check_revocations(&sbom, &revocation_list);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].revocation.severity, "critical");
    }

    #[test]
    fn test_no_revocation_match() {
        let gen = SbomGenerator::new("test-app", "0.1.0");
        let component = SbomComponent {
            component_type: ComponentType::Provider,
            name: "good-provider".to_string(),
            version: "2.0.0".to_string(),
            purl: None,
            sha256: None,
            license: None,
            description: None,
            revoked: false,
            revocation_reason: None,
        };
        let sbom = gen.generate(vec![component], vec![]);
        let revocation_list = RevocationList {
            last_updated: "2025-01-01T00:00:00Z".to_string(),
            entries: vec![RevocationEntry {
                name: "bad-provider".to_string(),
                version_range: "*".to_string(),
                reason: "Malicious code".to_string(),
                issued_at: "2025-01-01T00:00:00Z".to_string(),
                severity: "critical".to_string(),
                cve: None,
            }],
        };
        let matches = gen.check_revocations(&sbom, &revocation_list);
        assert_eq!(matches.len(), 0);
    }
}
