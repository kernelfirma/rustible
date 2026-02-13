//! Warewulf 4 profile importer.
//!
//! Parses Warewulf `nodes.conf` / `wwctl node list -y` YAML exports and maps
//! them into Rustible inventory hosts and groups.
//!
//! # Mapping Rules
//!
//! | Warewulf field            | Rustible equivalent              |
//! |---------------------------|----------------------------------|
//! | Node id / name            | Host `name`                      |
//! | `net_devs[0].ipaddr`      | `ansible_host` variable          |
//! | `profiles`                | Group membership                 |
//! | `tags`                    | Host variables                   |
//! | `container`               | `warewulf_container` host var    |
//! | `kernel.version`          | `warewulf_kernel_version` var    |
//! | `kernel.args`             | `warewulf_kernel_args` var       |
//! | `ipmi.*`                  | `ipmi_*` host variables          |

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::migration::error::{MigrationError, MigrationResult};
use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};

// ---------------------------------------------------------------------------
// Warewulf data model
// ---------------------------------------------------------------------------

/// A single Warewulf node as represented in `nodes.conf` YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarewulfNode {
    /// Node identifier (the YAML mapping key).
    #[serde(default)]
    pub id: String,

    /// Optional human-readable hostname override.
    /// When absent the `id` is used as the hostname.
    #[serde(default)]
    pub name: Option<String>,

    /// Profiles assigned to this node (used as group membership).
    #[serde(default)]
    pub profiles: Vec<String>,

    /// Network devices configured on the node.
    #[serde(default, alias = "network devices")]
    pub net_devs: Vec<WarewulfNetDev>,

    /// Kernel configuration.
    #[serde(default)]
    pub kernel: Option<WarewulfKernel>,

    /// Container / VNFS image name.
    #[serde(default)]
    pub container: Option<String>,

    /// IPMI / BMC configuration.
    #[serde(default)]
    pub ipmi: Option<WarewulfIpmi>,

    /// Arbitrary key-value tags.
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// A network device definition from Warewulf.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WarewulfNetDev {
    /// Interface name (e.g. `eth0`).
    #[serde(default)]
    pub device: Option<String>,

    /// IPv4 address.
    #[serde(default, alias = "ipaddr")]
    pub ipaddr: Option<String>,

    /// Network mask.
    #[serde(default)]
    pub netmask: Option<String>,

    /// Hardware (MAC) address.
    #[serde(default)]
    pub hwaddr: Option<String>,

    /// Default gateway.
    #[serde(default)]
    pub gateway: Option<String>,
}

/// Kernel settings from Warewulf.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WarewulfKernel {
    /// Kernel version string.
    #[serde(default)]
    pub version: Option<String>,

    /// Kernel boot arguments.
    #[serde(default)]
    pub args: Option<String>,
}

/// IPMI / BMC settings from Warewulf.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WarewulfIpmi {
    /// IPMI IP address.
    #[serde(default, alias = "ipaddr")]
    pub ipaddr: Option<String>,

    /// IPMI username.
    #[serde(default)]
    pub username: Option<String>,

    /// IPMI interface type (e.g. `lanplus`).
    #[serde(default)]
    pub interface: Option<String>,
}

// ---------------------------------------------------------------------------
// Import result types
// ---------------------------------------------------------------------------

/// A host imported from Warewulf into Rustible inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedHost {
    /// Hostname (derived from Warewulf node id/name).
    pub name: String,

    /// The `ansible_host` value (typically the primary IP address).
    pub ansible_host: Option<String>,

    /// Groups this host belongs to (derived from Warewulf profiles).
    pub groups: Vec<String>,

    /// Host variables merged from tags, kernel, container, and IPMI fields.
    pub vars: HashMap<String, serde_yaml::Value>,
}

/// A group imported from Warewulf profile names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedGroup {
    /// Group name (derived from a Warewulf profile name).
    pub name: String,

    /// Hostnames belonging to this group.
    pub hosts: Vec<String>,

    /// Group-level variables (currently empty; can be populated later).
    pub vars: HashMap<String, serde_yaml::Value>,
}

/// Result of a Warewulf profile import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileImportResult {
    /// Imported hosts.
    pub hosts: Vec<ImportedHost>,

    /// Imported groups.
    pub groups: Vec<ImportedGroup>,

    /// Migration report with diagnostics and findings.
    pub report: MigrationReport,
}

// ---------------------------------------------------------------------------
// Importer
// ---------------------------------------------------------------------------

/// Importer for Warewulf 4 node/profile YAML data.
///
/// # Example YAML input
///
/// ```yaml
/// noderange:
///   compute-01:
///     profiles:
///       - default
///       - compute
///     net_devs:
///       - device: eth0
///         ipaddr: 10.0.0.101
///         netmask: 255.255.255.0
///     kernel:
///       version: 5.14.0-284.el9.x86_64
///     container: rocky-9
///     tags:
///       rack: A01
///       role: compute
/// ```
pub struct WarewulfProfileImporter;

impl WarewulfProfileImporter {
    /// Import Warewulf nodes from a YAML file on disk.
    ///
    /// The YAML is expected to be either:
    /// - A top-level mapping of `node_id -> WarewulfNode`, or
    /// - A mapping with a `noderange` key containing the node mapping.
    pub fn import_from_yaml(path: &Path) -> MigrationResult<ProfileImportResult> {
        let content = std::fs::read_to_string(path).map_err(|e| MigrationError::ParseError {
            file: path.display().to_string(),
            message: format!("could not read Warewulf YAML file: {}", e),
        })?;

        Self::import_from_str(&content)
    }

    /// Import Warewulf nodes from a YAML string.
    pub fn import_from_str(yaml: &str) -> MigrationResult<ProfileImportResult> {
        let raw: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| MigrationError::ParseError {
                file: "warewulf-nodes.yaml".into(),
                message: format!("invalid YAML: {}", e),
            })?;

        // Support both top-level mapping and `noderange:` wrapper.
        let nodes_mapping = if let Some(mapping) = raw.as_mapping() {
            if let Some(noderange) = mapping.get(&serde_yaml::Value::String("noderange".into())) {
                noderange
                    .as_mapping()
                    .ok_or_else(|| MigrationError::ParseError {
                        file: "warewulf-nodes.yaml".into(),
                        message: "'noderange' must be a mapping".to_string(),
                    })?
                    .clone()
            } else {
                mapping.clone()
            }
        } else {
            return Err(MigrationError::ParseError {
                file: "warewulf-nodes.yaml".into(),
                message: "expected a YAML mapping at the top level".to_string(),
            });
        };

        let mut report = MigrationReport::new("Warewulf 4 profiles", "profile import");
        let mut hosts: Vec<ImportedHost> = Vec::new();
        let mut group_map: HashMap<String, Vec<String>> = HashMap::new();

        for (key, value) in &nodes_mapping {
            let node_id = match key.as_str() {
                Some(id) => id.to_string(),
                None => {
                    report.findings.push(MigrationFinding {
                        source_item: "Non-string node key".into(),
                        target_item: None,
                        status: FindingStatus::Skipped,
                        diagnostics: vec![MigrationDiagnostic {
                            category: DiagnosticCategory::TypeMismatch,
                            severity: MigrationSeverity::Warning,
                            source_path: None,
                            source_field: None,
                            message: "Skipping non-string node key".to_string(),
                            suggestion: None,
                        }],
                    });
                    continue;
                }
            };

            // Skip metadata keys that are not nodes.
            if node_id == "noderange" || node_id == "warewulf" {
                continue;
            }

            match serde_yaml::from_value::<WarewulfNode>(value.clone()) {
                Ok(mut node) => {
                    node.id = node_id.clone();
                    match Self::map_node(&node, &mut report) {
                        Some(host) => {
                            // Register host into its profile groups.
                            for group in &host.groups {
                                group_map
                                    .entry(group.clone())
                                    .or_default()
                                    .push(host.name.clone());
                            }
                            hosts.push(host);
                        }
                        None => {
                            report.findings.push(MigrationFinding {
                                source_item: format!("Node '{}'", node_id),
                                target_item: None,
                                status: FindingStatus::Divergent,
                                diagnostics: vec![MigrationDiagnostic {
                                    category: DiagnosticCategory::AttributeMismatch,
                                    severity: MigrationSeverity::Error,
                                    source_path: None,
                                    source_field: None,
                                    message: format!("Failed to map node '{}'", node_id),
                                    suggestion: None,
                                }],
                            });
                        }
                    }
                }
                Err(e) => {
                    report.findings.push(MigrationFinding {
                        source_item: format!("Node '{}'", node_id),
                        target_item: None,
                        status: FindingStatus::Divergent,
                        diagnostics: vec![MigrationDiagnostic {
                            category: DiagnosticCategory::TypeMismatch,
                            severity: MigrationSeverity::Error,
                            source_path: None,
                            source_field: None,
                            message: format!("Failed to parse node '{}': {}", node_id, e),
                            suggestion: None,
                        }],
                    });
                }
            }
        }

        // Build groups from the accumulated map.
        let groups: Vec<ImportedGroup> = group_map
            .into_iter()
            .map(|(name, members)| ImportedGroup {
                name,
                hosts: members,
                vars: HashMap::new(),
            })
            .collect();

        report.compute_summary();
        report.compute_outcome(0.1);

        Ok(ProfileImportResult {
            hosts,
            groups,
            report,
        })
    }

    /// Map a single `WarewulfNode` to an `ImportedHost`.
    ///
    /// Returns `None` if the node cannot be meaningfully mapped.
    fn map_node(node: &WarewulfNode, report: &mut MigrationReport) -> Option<ImportedHost> {
        let hostname = node.name.as_deref().unwrap_or(&node.id).to_string();

        if hostname.is_empty() {
            return None;
        }

        let mut vars: HashMap<String, serde_yaml::Value> = HashMap::new();

        // Map primary IP from first network device.
        let ansible_host = node.net_devs.first().and_then(|nd| nd.ipaddr.clone());

        if ansible_host.is_none() {
            report.findings.push(MigrationFinding {
                source_item: format!("Node '{}' network config", hostname),
                target_item: None,
                status: FindingStatus::PartiallyMapped,
                diagnostics: vec![MigrationDiagnostic {
                    category: DiagnosticCategory::CompatibilityGap,
                    severity: MigrationSeverity::Warning,
                    source_path: None,
                    source_field: Some("net_devs".into()),
                    message: format!(
                        "Node '{}' has no network device IP; ansible_host will be unset",
                        hostname
                    ),
                    suggestion: Some(
                        "Add a network device with an IP address to the node definition"
                            .to_string(),
                    ),
                }],
            });
        }

        // Map tags to host vars.
        for (k, v) in &node.tags {
            vars.insert(k.clone(), serde_yaml::Value::String(v.clone()));
        }

        // Map container.
        if let Some(ref container) = node.container {
            vars.insert(
                "warewulf_container".to_string(),
                serde_yaml::Value::String(container.clone()),
            );
        }

        // Map kernel settings.
        if let Some(ref kernel) = node.kernel {
            if let Some(ref ver) = kernel.version {
                vars.insert(
                    "warewulf_kernel_version".to_string(),
                    serde_yaml::Value::String(ver.clone()),
                );
            }
            if let Some(ref args) = kernel.args {
                vars.insert(
                    "warewulf_kernel_args".to_string(),
                    serde_yaml::Value::String(args.clone()),
                );
            }
        }

        // Map IPMI settings.
        if let Some(ref ipmi) = node.ipmi {
            if let Some(ref ip) = ipmi.ipaddr {
                vars.insert(
                    "ipmi_address".to_string(),
                    serde_yaml::Value::String(ip.clone()),
                );
            }
            if let Some(ref user) = ipmi.username {
                vars.insert(
                    "ipmi_username".to_string(),
                    serde_yaml::Value::String(user.clone()),
                );
            }
            if let Some(ref iface) = ipmi.interface {
                vars.insert(
                    "ipmi_interface".to_string(),
                    serde_yaml::Value::String(iface.clone()),
                );
            }
        }

        // Map network device metadata beyond the primary IP.
        for (i, nd) in node.net_devs.iter().enumerate() {
            if let Some(ref dev) = nd.device {
                vars.insert(
                    format!("warewulf_netdev_{}_device", i),
                    serde_yaml::Value::String(dev.clone()),
                );
            }
            if let Some(ref mask) = nd.netmask {
                vars.insert(
                    format!("warewulf_netdev_{}_netmask", i),
                    serde_yaml::Value::String(mask.clone()),
                );
            }
            if let Some(ref mac) = nd.hwaddr {
                vars.insert(
                    format!("warewulf_netdev_{}_hwaddr", i),
                    serde_yaml::Value::String(mac.clone()),
                );
            }
            if let Some(ref gw) = nd.gateway {
                vars.insert(
                    format!("warewulf_netdev_{}_gateway", i),
                    serde_yaml::Value::String(gw.clone()),
                );
            }
        }

        // Emit a finding if the node has no profiles.
        if node.profiles.is_empty() {
            report.findings.push(MigrationFinding {
                source_item: format!("Node '{}' profiles", hostname),
                target_item: None,
                status: FindingStatus::Mapped,
                diagnostics: vec![MigrationDiagnostic {
                    category: DiagnosticCategory::CompatibilityGap,
                    severity: MigrationSeverity::Info,
                    source_path: None,
                    source_field: Some("profiles".into()),
                    message: format!(
                        "Node '{}' has no profiles; will not be assigned to any group",
                        hostname
                    ),
                    suggestion: Some(
                        "Consider assigning at least one profile/group for organisation"
                            .to_string(),
                    ),
                }],
            });
        }

        Some(ImportedHost {
            name: hostname,
            ansible_host,
            groups: node.profiles.clone(),
            vars,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::report::MigrationOutcome;

    #[test]
    fn test_import_basic_nodes() {
        let yaml = r#"
compute-01:
  profiles:
    - default
    - compute
  net_devs:
    - device: eth0
      ipaddr: "10.0.0.101"
      netmask: "255.255.255.0"
      hwaddr: "00:11:22:33:44:55"
  kernel:
    version: "5.14.0-284.el9.x86_64"
    args: "quiet"
  container: rocky-9
  tags:
    rack: A01
    role: compute
compute-02:
  profiles:
    - default
    - compute
  net_devs:
    - device: eth0
      ipaddr: "10.0.0.102"
      netmask: "255.255.255.0"
  container: rocky-9
"#;

        let result = WarewulfProfileImporter::import_from_str(yaml).unwrap();
        assert_eq!(result.hosts.len(), 2);

        let host1 = result
            .hosts
            .iter()
            .find(|h| h.name == "compute-01")
            .unwrap();
        assert_eq!(host1.ansible_host.as_deref(), Some("10.0.0.101"));
        assert_eq!(host1.groups, vec!["default", "compute"]);
        assert_eq!(
            host1.vars.get("warewulf_container"),
            Some(&serde_yaml::Value::String("rocky-9".to_string()))
        );
        assert_eq!(
            host1.vars.get("warewulf_kernel_version"),
            Some(&serde_yaml::Value::String(
                "5.14.0-284.el9.x86_64".to_string()
            ))
        );
        assert_eq!(
            host1.vars.get("rack"),
            Some(&serde_yaml::Value::String("A01".to_string()))
        );

        // Check groups were created.
        let compute_group = result.groups.iter().find(|g| g.name == "compute").unwrap();
        assert_eq!(compute_group.hosts.len(), 2);
        assert!(compute_group.hosts.contains(&"compute-01".to_string()));
        assert!(compute_group.hosts.contains(&"compute-02".to_string()));

        // Report should show success.
        assert_ne!(result.report.outcome, MigrationOutcome::Fail);
    }

    #[test]
    fn test_import_noderange_wrapper() {
        let yaml = r#"
noderange:
  gpu-node-01:
    profiles:
      - gpu
    net_devs:
      - device: ib0
        ipaddr: "10.10.0.1"
    ipmi:
      ipaddr: "192.168.1.101"
      username: admin
      interface: lanplus
"#;

        let result = WarewulfProfileImporter::import_from_str(yaml).unwrap();
        assert_eq!(result.hosts.len(), 1);

        let host = &result.hosts[0];
        assert_eq!(host.name, "gpu-node-01");
        assert_eq!(host.ansible_host.as_deref(), Some("10.10.0.1"));
        assert_eq!(host.groups, vec!["gpu"]);
        assert_eq!(
            host.vars.get("ipmi_address"),
            Some(&serde_yaml::Value::String("192.168.1.101".to_string()))
        );
        assert_eq!(
            host.vars.get("ipmi_username"),
            Some(&serde_yaml::Value::String("admin".to_string()))
        );
        assert_eq!(
            host.vars.get("ipmi_interface"),
            Some(&serde_yaml::Value::String("lanplus".to_string()))
        );
    }

    #[test]
    fn test_import_node_without_ip_emits_warning() {
        let yaml = r#"
bare-node:
  profiles:
    - default
  net_devs: []
  tags:
    location: lab
"#;

        let result = WarewulfProfileImporter::import_from_str(yaml).unwrap();
        assert_eq!(result.hosts.len(), 1);
        assert!(result.hosts[0].ansible_host.is_none());

        // Should have a warning in findings about missing IP.
        let has_warning = result.report.findings.iter().any(|f| {
            f.diagnostics
                .iter()
                .any(|d| d.severity == MigrationSeverity::Warning)
        });
        assert!(
            has_warning,
            "expected a warning about missing network device IP"
        );
    }

    #[test]
    fn test_import_invalid_yaml_returns_error() {
        let yaml = "not: valid: yaml: [broken";
        let result = WarewulfProfileImporter::import_from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_node_without_profiles_emits_finding() {
        let yaml = r#"
lonely-node:
  net_devs:
    - ipaddr: "10.0.0.50"
"#;

        let result = WarewulfProfileImporter::import_from_str(yaml).unwrap();
        assert_eq!(result.hosts.len(), 1);
        assert!(result.hosts[0].groups.is_empty());

        // Should have a finding about no profiles.
        let findings: Vec<_> = result
            .report
            .findings
            .iter()
            .filter(|f| f.source_item.contains("profiles"))
            .collect();
        assert!(
            !findings.is_empty(),
            "expected finding about missing profiles"
        );
    }
}
