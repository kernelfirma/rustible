//! Warewulf image and overlay import.
//!
//! Parses Warewulf container image definitions and overlay configurations
//! from YAML, mapping them to Rustible-native metadata structures.
//! Warewulf `.ww` template files are mapped to Jinja2 template references.

use serde::{Deserialize, Serialize};

use crate::migration::error::{MigrationError, MigrationResult};
use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Metadata for a Warewulf container image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarewulfImage {
    /// Logical name used by Warewulf (e.g. `rocky9`).
    pub name: String,
    /// OCI / container image reference (e.g. `ghcr.io/warewulf/rocky:9`).
    pub container_name: String,
    /// Absolute path to the extracted root filesystem.
    pub rootfs_path: String,
    /// Image size in bytes (0 if unknown).
    pub size_bytes: u64,
    /// Optional integrity checksum (sha256 hex).
    pub checksum: Option<String>,
}

/// Type of a Warewulf overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayType {
    /// System overlay applied at image build time.
    System,
    /// Runtime overlay applied on each boot.
    Runtime,
    /// User-defined overlay.
    Custom,
}

/// A single template file within an overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateFile {
    /// Path of the template source file relative to the overlay root.
    pub source_path: String,
    /// Destination path on the target node.
    pub dest_path: String,
    /// Whether this file uses Warewulf `.ww` template syntax.
    pub is_ww_template: bool,
}

/// Metadata for a Warewulf overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarewulfOverlay {
    /// Overlay name (e.g. `wwinit`, `generic`, `chrony`).
    pub name: String,
    /// Overlay type classification.
    pub overlay_type: OverlayType,
    /// Template files contained in this overlay.
    pub template_files: Vec<TemplateFile>,
}

// ---------------------------------------------------------------------------
// Import result
// ---------------------------------------------------------------------------

/// Result of a Warewulf image/overlay import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageImportResult {
    /// Imported container images.
    pub images: Vec<WarewulfImage>,
    /// Imported overlays.
    pub overlays: Vec<WarewulfOverlay>,
    /// Total number of template files across all overlays.
    pub template_count: usize,
    /// Migration report with diagnostics.
    pub report: MigrationReport,
}

// ---------------------------------------------------------------------------
// YAML source model (what we parse from disk)
// ---------------------------------------------------------------------------

/// Top-level YAML structure for a Warewulf images configuration.
#[derive(Debug, Deserialize)]
struct YamlWarewulfConfig {
    #[serde(default)]
    images: Vec<YamlImage>,
    #[serde(default)]
    overlays: Vec<YamlOverlay>,
}

#[derive(Debug, Deserialize)]
struct YamlImage {
    name: Option<String>,
    container: Option<String>,
    rootfs: Option<String>,
    #[serde(default)]
    size: u64,
    checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YamlOverlay {
    name: Option<String>,
    #[serde(default = "default_overlay_type")]
    overlay_type: String,
    #[serde(default)]
    files: Vec<YamlTemplateFile>,
}

fn default_overlay_type() -> String {
    "custom".to_string()
}

#[derive(Debug, Deserialize)]
struct YamlTemplateFile {
    source: Option<String>,
    dest: Option<String>,
    #[serde(default)]
    ww_template: bool,
}

// ---------------------------------------------------------------------------
// Importer
// ---------------------------------------------------------------------------

/// Importer for Warewulf container images and overlays.
pub struct WarewulfImageImporter;

impl WarewulfImageImporter {
    /// Create a new importer instance.
    pub fn new() -> Self {
        Self
    }

    /// Import images and overlays from a YAML string.
    ///
    /// The expected YAML schema:
    ///
    /// ```yaml
    /// images:
    ///   - name: rocky9
    ///     container: ghcr.io/warewulf/rocky:9
    ///     rootfs: /var/lib/warewulf/chroots/rocky9
    ///     size: 1073741824
    ///     checksum: abc123...
    ///
    /// overlays:
    ///   - name: wwinit
    ///     overlay_type: system
    ///     files:
    ///       - source: hostname.ww
    ///         dest: /etc/hostname
    ///         ww_template: true
    ///       - source: resolv.conf
    ///         dest: /etc/resolv.conf
    ///         ww_template: false
    /// ```
    pub fn import_from_yaml(&self, yaml_content: &str) -> MigrationResult<ImageImportResult> {
        let config: YamlWarewulfConfig =
            serde_yaml::from_str(yaml_content).map_err(|e| MigrationError::ParseError(e.to_string()))?;

        let mut report = MigrationReport::new("warewulf-images");
        let mut images = Vec::new();
        let mut overlays = Vec::new();
        let mut template_count: usize = 0;
        let mut finding_idx: usize = 0;

        // Process images
        for (idx, yaml_img) in config.images.iter().enumerate() {
            let name = match &yaml_img.name {
                Some(n) if !n.is_empty() => n.clone(),
                _ => {
                    finding_idx += 1;
                    report.add_finding(MigrationFinding {
                        id: format!("IMG-E{:03}", finding_idx),
                        severity: MigrationSeverity::Error,
                        status: FindingStatus::Open,
                        title: format!("Image at index {} has no name", idx),
                        description: "Every image entry must have a non-empty 'name' field.".into(),
                        category: DiagnosticCategory::Validation,
                        diagnostics: vec![MigrationDiagnostic {
                            severity: MigrationSeverity::Error,
                            category: DiagnosticCategory::Validation,
                            message: format!("Missing name for image at index {}", idx),
                            source_object: None,
                        }],
                    });
                    continue;
                }
            };

            let container_name = yaml_img.container.clone().unwrap_or_default();
            if container_name.is_empty() {
                finding_idx += 1;
                report.add_finding(MigrationFinding {
                    id: format!("IMG-W{:03}", finding_idx),
                    severity: MigrationSeverity::Warning,
                    status: FindingStatus::Open,
                    title: format!("Image '{}' has no container reference", name),
                    description: "The 'container' field is empty; image may not be pullable.".into(),
                    category: DiagnosticCategory::FieldMapping,
                    diagnostics: vec![],
                });
            }

            let rootfs_path = yaml_img
                .rootfs
                .clone()
                .unwrap_or_else(|| format!("/var/lib/warewulf/chroots/{}", name));

            images.push(WarewulfImage {
                name,
                container_name,
                rootfs_path,
                size_bytes: yaml_img.size,
                checksum: yaml_img.checksum.clone(),
            });
        }

        // Process overlays
        for (idx, yaml_ovl) in config.overlays.iter().enumerate() {
            let name = match &yaml_ovl.name {
                Some(n) if !n.is_empty() => n.clone(),
                _ => {
                    finding_idx += 1;
                    report.add_finding(MigrationFinding {
                        id: format!("OVL-E{:03}", finding_idx),
                        severity: MigrationSeverity::Error,
                        status: FindingStatus::Open,
                        title: format!("Overlay at index {} has no name", idx),
                        description: "Every overlay entry must have a non-empty 'name' field.".into(),
                        category: DiagnosticCategory::Validation,
                        diagnostics: vec![],
                    });
                    continue;
                }
            };

            let overlay_type = match yaml_ovl.overlay_type.to_lowercase().as_str() {
                "system" => OverlayType::System,
                "runtime" => OverlayType::Runtime,
                "custom" => OverlayType::Custom,
                other => {
                    finding_idx += 1;
                    report.add_finding(MigrationFinding {
                        id: format!("OVL-W{:03}", finding_idx),
                        severity: MigrationSeverity::Warning,
                        status: FindingStatus::Open,
                        title: format!("Unknown overlay type '{}' for '{}'", other, name),
                        description: format!(
                            "Overlay type '{}' is not recognised; defaulting to 'custom'.",
                            other
                        ),
                        category: DiagnosticCategory::Unsupported,
                        diagnostics: vec![],
                    });
                    OverlayType::Custom
                }
            };

            let mut template_files = Vec::new();
            for yaml_file in &yaml_ovl.files {
                let source_path = yaml_file.source.clone().unwrap_or_default();
                let dest_path = yaml_file.dest.clone().unwrap_or_default();

                if source_path.is_empty() || dest_path.is_empty() {
                    finding_idx += 1;
                    report.add_finding(MigrationFinding {
                        id: format!("TPL-W{:03}", finding_idx),
                        severity: MigrationSeverity::Warning,
                        status: FindingStatus::Open,
                        title: format!(
                            "Incomplete template file in overlay '{}'",
                            name
                        ),
                        description: format!(
                            "source='{}', dest='{}' — both must be non-empty.",
                            source_path, dest_path
                        ),
                        category: DiagnosticCategory::Validation,
                        diagnostics: vec![],
                    });
                    continue;
                }

                // Detect .ww templates by extension or explicit flag
                let is_ww = yaml_file.ww_template || source_path.ends_with(".ww");

                if is_ww {
                    finding_idx += 1;
                    report.add_finding(MigrationFinding {
                        id: format!("TPL-I{:03}", finding_idx),
                        severity: MigrationSeverity::Info,
                        status: FindingStatus::Resolved,
                        title: format!("Mapped .ww template to Jinja2: {}", source_path),
                        description: format!(
                            "Warewulf template '{}' will be referenced as a Jinja2 template at '{}'.",
                            source_path, dest_path
                        ),
                        category: DiagnosticCategory::FieldMapping,
                        diagnostics: vec![MigrationDiagnostic {
                            severity: MigrationSeverity::Info,
                            category: DiagnosticCategory::FieldMapping,
                            message: format!(
                                ".ww template '{}' -> Jinja2 ref '{}'",
                                source_path, dest_path
                            ),
                            source_object: Some(name.clone()),
                        }],
                    });
                }

                template_count += 1;
                template_files.push(TemplateFile {
                    source_path,
                    dest_path,
                    is_ww_template: is_ww,
                });
            }

            overlays.push(WarewulfOverlay {
                name,
                overlay_type,
                template_files,
            });
        }

        Ok(ImageImportResult {
            images,
            overlays,
            template_count,
            report,
        })
    }
}

impl Default for WarewulfImageImporter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::report::MigrationOutcome;

    #[test]
    fn test_import_basic_images_and_overlays() {
        let yaml = r#"
images:
  - name: rocky9
    container: ghcr.io/warewulf/rocky:9
    rootfs: /var/lib/warewulf/chroots/rocky9
    size: 1073741824
    checksum: abcdef1234567890

overlays:
  - name: wwinit
    overlay_type: system
    files:
      - source: hostname.ww
        dest: /etc/hostname
        ww_template: true
      - source: resolv.conf
        dest: /etc/resolv.conf
        ww_template: false
  - name: chrony
    overlay_type: runtime
    files:
      - source: chrony.conf.ww
        dest: /etc/chrony.conf
"#;

        let importer = WarewulfImageImporter::new();
        let result = importer.import_from_yaml(yaml).unwrap();

        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].name, "rocky9");
        assert_eq!(result.images[0].container_name, "ghcr.io/warewulf/rocky:9");
        assert_eq!(result.images[0].size_bytes, 1_073_741_824);
        assert_eq!(
            result.images[0].checksum.as_deref(),
            Some("abcdef1234567890")
        );

        assert_eq!(result.overlays.len(), 2);
        assert_eq!(result.overlays[0].name, "wwinit");
        assert_eq!(result.overlays[0].overlay_type, OverlayType::System);
        assert_eq!(result.overlays[0].template_files.len(), 2);
        assert!(result.overlays[0].template_files[0].is_ww_template);
        assert!(!result.overlays[0].template_files[1].is_ww_template);

        assert_eq!(result.overlays[1].name, "chrony");
        assert_eq!(result.overlays[1].overlay_type, OverlayType::Runtime);
        // chrony.conf.ww should be detected as a .ww template by extension
        assert!(result.overlays[1].template_files[0].is_ww_template);

        assert_eq!(result.template_count, 3);
        assert_eq!(result.report.compute_outcome(5), MigrationOutcome::Success);
    }

    #[test]
    fn test_import_missing_name_produces_error() {
        let yaml = r#"
images:
  - container: ghcr.io/warewulf/rocky:9
    rootfs: /var/lib/warewulf/chroots/rocky9

overlays:
  - overlay_type: system
    files:
      - source: hostname.ww
        dest: /etc/hostname
"#;

        let importer = WarewulfImageImporter::new();
        let result = importer.import_from_yaml(yaml).unwrap();

        // Both the unnamed image and unnamed overlay should be skipped
        assert_eq!(result.images.len(), 0);
        assert_eq!(result.overlays.len(), 0);

        let summary = result.report.compute_summary();
        assert_eq!(summary.errors, 2, "expected 2 errors for missing names");
    }

    #[test]
    fn test_import_unknown_overlay_type_defaults_to_custom() {
        let yaml = r#"
images: []
overlays:
  - name: custom_overlay
    overlay_type: experimental
    files:
      - source: test.conf
        dest: /etc/test.conf
"#;

        let importer = WarewulfImageImporter::new();
        let result = importer.import_from_yaml(yaml).unwrap();

        assert_eq!(result.overlays.len(), 1);
        assert_eq!(result.overlays[0].overlay_type, OverlayType::Custom);

        let summary = result.report.compute_summary();
        assert!(summary.warnings >= 1, "expected at least one warning for unknown overlay type");
    }

    #[test]
    fn test_import_empty_config() {
        let yaml = "images: []\noverlays: []\n";
        let importer = WarewulfImageImporter::new();
        let result = importer.import_from_yaml(yaml).unwrap();

        assert!(result.images.is_empty());
        assert!(result.overlays.is_empty());
        assert_eq!(result.template_count, 0);
        assert_eq!(result.report.compute_outcome(0), MigrationOutcome::Success);
    }
}
