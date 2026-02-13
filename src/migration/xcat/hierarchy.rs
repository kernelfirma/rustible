//! xCAT hierarchy/service-node topology import.
//!
//! Imports xCAT management hierarchy (management node, service nodes,
//! compute nodes) into Rustible inventory groups.

use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An xCAT service node in the hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XcatServiceNode {
    pub name: String,
    pub ip: Option<String>,
    pub compute_nodes: Vec<String>,
}

/// An xCAT hierarchy definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XcatHierarchy {
    pub management_node: String,
    pub service_nodes: Vec<XcatServiceNode>,
    pub unassigned_nodes: Vec<String>,
}

/// Imported Rustible group from xCAT hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedGroup {
    pub name: String,
    pub hosts: Vec<String>,
    pub vars: HashMap<String, String>,
    pub children: Vec<String>,
}

/// Result of an xCAT hierarchy import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyImportResult {
    pub groups: Vec<ImportedGroup>,
    pub total_nodes: usize,
    pub service_node_count: usize,
    pub compute_node_count: usize,
    pub unassigned_count: usize,
}

/// Imports xCAT hierarchy into Rustible inventory structure.
pub struct XcatHierarchyImporter {
    dry_run: bool,
}

impl XcatHierarchyImporter {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    /// Import from a YAML definition of xCAT hierarchy.
    pub fn import_from_yaml(
        &self,
        yaml: &str,
    ) -> crate::migration::MigrationResult<(HierarchyImportResult, MigrationReport)> {
        let hierarchy: XcatHierarchy = serde_yaml::from_str(yaml).map_err(|e| {
            crate::migration::MigrationError::ParseError {
                file: "xcat-hierarchy.yaml".into(),
                message: format!("xCAT hierarchy YAML: {}", e),
            }
        })?;

        self.import(&hierarchy)
    }

    /// Import from a parsed xCAT hierarchy.
    pub fn import(
        &self,
        hierarchy: &XcatHierarchy,
    ) -> crate::migration::MigrationResult<(HierarchyImportResult, MigrationReport)> {
        let mut groups = Vec::new();
        let mut report = MigrationReport::new("xCAT Hierarchy Import", "hierarchy import");

        // Top-level management group
        let mut mgmt_children = Vec::new();

        // Service node groups
        let mut compute_count = 0usize;
        for sn in &hierarchy.service_nodes {
            let group_name = format!("sn_{}", sn.name.replace(['.', '-'], "_"));
            mgmt_children.push(group_name.clone());

            let mut vars = HashMap::new();
            vars.insert("primary_service_node".to_string(), sn.name.clone());
            if let Some(ref ip) = sn.ip {
                vars.insert("service_node_ip".to_string(), ip.clone());
            }

            groups.push(ImportedGroup {
                name: group_name,
                hosts: sn.compute_nodes.clone(),
                vars,
                children: Vec::new(),
            });
            compute_count += sn.compute_nodes.len();
        }

        // Management group
        groups.push(ImportedGroup {
            name: "xcat_managed".to_string(),
            hosts: vec![hierarchy.management_node.clone()],
            vars: HashMap::from([("xcat_role".to_string(), "management".to_string())]),
            children: mgmt_children,
        });

        // Validation: check for duplicate nodes
        let mut all_nodes: Vec<&str> = Vec::new();
        all_nodes.push(&hierarchy.management_node);
        for sn in &hierarchy.service_nodes {
            all_nodes.push(&sn.name);
            for cn in &sn.compute_nodes {
                all_nodes.push(cn);
            }
        }
        for n in &hierarchy.unassigned_nodes {
            all_nodes.push(n);
        }

        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for node in &all_nodes {
            if !seen.insert(*node) {
                duplicates.push((*node).to_string());
            }
        }

        // Validation finding
        report.findings.push(MigrationFinding {
            source_item: "Node Uniqueness".into(),
            target_item: None,
            status: if duplicates.is_empty() {
                FindingStatus::Matched
            } else {
                FindingStatus::Divergent
            },
            diagnostics: duplicates
                .iter()
                .map(|d| MigrationDiagnostic {
                    category: DiagnosticCategory::OutputMismatch,
                    severity: MigrationSeverity::Error,
                    source_path: None,
                    source_field: None,
                    message: format!("Duplicate node: {}", d),
                    suggestion: None,
                })
                .collect(),
        });

        // Service node coverage
        report.findings.push(MigrationFinding {
            source_item: "Service Node Coverage".into(),
            target_item: None,
            status: if hierarchy.unassigned_nodes.is_empty() {
                FindingStatus::Matched
            } else {
                FindingStatus::PartiallyMapped
            },
            diagnostics: if hierarchy.unassigned_nodes.is_empty() {
                vec![]
            } else {
                vec![MigrationDiagnostic {
                    category: DiagnosticCategory::SemanticDivergence,
                    severity: MigrationSeverity::Warning,
                    source_path: None,
                    source_field: None,
                    message: format!(
                        "{} nodes not assigned to any service node",
                        hierarchy.unassigned_nodes.len()
                    ),
                    suggestion: Some(hierarchy.unassigned_nodes.join(", ")),
                }]
            },
        });

        // Hierarchy structure finding
        report.findings.push(MigrationFinding {
            source_item: "Hierarchy Structure".into(),
            target_item: None,
            status: FindingStatus::Matched,
            diagnostics: vec![MigrationDiagnostic {
                category: DiagnosticCategory::CompatibilityGap,
                severity: MigrationSeverity::Info,
                source_path: None,
                source_field: None,
                message: format!(
                    "Imported {} service nodes, {} compute nodes from management node '{}'",
                    hierarchy.service_nodes.len(),
                    compute_count,
                    hierarchy.management_node
                ),
                suggestion: None,
            }],
        });

        report.compute_summary();
        report.compute_outcome(0.8);

        let result = HierarchyImportResult {
            groups,
            total_nodes: all_nodes.len(),
            service_node_count: hierarchy.service_nodes.len(),
            compute_node_count: compute_count,
            unassigned_count: hierarchy.unassigned_nodes.len(),
        };

        Ok((result, report))
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::MigrationOutcome;

    #[test]
    fn test_basic_hierarchy_import() {
        let yaml = r#"
management_node: mgmt01
service_nodes:
  - name: sn01
    ip: "10.0.1.1"
    compute_nodes: ["node001", "node002", "node003"]
  - name: sn02
    ip: "10.0.1.2"
    compute_nodes: ["node004", "node005"]
unassigned_nodes: []
"#;
        let importer = XcatHierarchyImporter::new(true);
        let (result, report) = importer.import_from_yaml(yaml).unwrap();
        assert_eq!(result.service_node_count, 2);
        assert_eq!(result.compute_node_count, 5);
        assert_eq!(result.unassigned_count, 0);
        assert_eq!(report.outcome, MigrationOutcome::Pass);
    }

    #[test]
    fn test_hierarchy_with_unassigned() {
        let yaml = r#"
management_node: mgmt01
service_nodes:
  - name: sn01
    compute_nodes: ["node001"]
unassigned_nodes: ["orphan01", "orphan02"]
"#;
        let importer = XcatHierarchyImporter::new(false);
        let (result, _report) = importer.import_from_yaml(yaml).unwrap();
        assert_eq!(result.unassigned_count, 2);
    }

    #[test]
    fn test_empty_hierarchy() {
        let yaml = r#"
management_node: mgmt01
service_nodes: []
unassigned_nodes: []
"#;
        let importer = XcatHierarchyImporter::new(true);
        let (result, _) = importer.import_from_yaml(yaml).unwrap();
        assert_eq!(result.total_nodes, 1);
    }
}
