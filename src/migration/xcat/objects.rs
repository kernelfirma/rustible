//! xCAT object definition parser and importer.
//!
//! Parses the output of `lsdef -l` (long listing) which produces
//! key=value attribute listings per object, and maps them into
//! Rustible [`Host`] and [`Group`] structures.
//!
//! # xCAT `lsdef` format
//!
//! ```text
//! Object name: node01
//!     arch=x86_64
//!     groups=compute,rack1
//!     ip=10.0.0.101
//!     mac=aa:bb:cc:dd:ee:01
//!     bmc=10.0.1.101
//!     os=rhels8.5
//!     status=booted
//! ```

use std::collections::{HashMap, HashSet};

use crate::inventory::{Group, Host};
use crate::migration::error::MigrationResult;
use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};

/// The type of an xCAT object as determined from context or the
/// `objtype` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XcatObjectType {
    /// A compute or management node.
    Node,
    /// A logical group of nodes.
    Group,
    /// A network definition.
    Network,
    /// An OS image definition.
    Osimage,
    /// A policy rule.
    Policy,
}

impl XcatObjectType {
    /// Try to parse an object type from a string value.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "node" => Some(Self::Node),
            "group" => Some(Self::Group),
            "network" => Some(Self::Network),
            "osimage" => Some(Self::Osimage),
            "policy" => Some(Self::Policy),
            _ => None,
        }
    }
}

/// A parsed xCAT object with its raw attributes.
#[derive(Debug, Clone)]
pub struct XcatObject {
    /// Object name as reported by `lsdef`.
    pub name: String,
    /// Object type, if determinable.
    pub object_type: Option<XcatObjectType>,
    /// Raw key=value attributes.
    pub attributes: HashMap<String, String>,
}

/// Result of importing a collection of xCAT objects.
#[derive(Debug, Clone)]
pub struct ObjectImportResult {
    /// Successfully mapped hosts.
    pub hosts: Vec<Host>,
    /// Derived groups (from the `groups` attribute on nodes).
    pub groups: Vec<Group>,
    /// Migration report with diagnostics.
    pub report: MigrationReport,
}

/// Importer that converts xCAT `lsdef` output into Rustible inventory.
pub struct XcatObjectImporter {
    /// Default object type to assume when `objtype` is not present.
    default_type: Option<XcatObjectType>,
}

impl XcatObjectImporter {
    /// Create a new importer with no default type assumption.
    pub fn new() -> Self {
        Self { default_type: None }
    }

    /// Create a new importer that assumes all objects are the given type
    /// unless an explicit `objtype` attribute overrides it.
    pub fn with_default_type(object_type: XcatObjectType) -> Self {
        Self {
            default_type: Some(object_type),
        }
    }

    /// Parse raw `lsdef -l` output into a list of [`XcatObject`]s.
    pub fn parse_lsdef(&self, input: &str) -> MigrationResult<Vec<XcatObject>> {
        let mut objects = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_attrs: HashMap<String, String> = HashMap::new();

        for line in input.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Object header: "Object name: <name>"
            if let Some(rest) = trimmed.strip_prefix("Object name:") {
                // Flush previous object
                if let Some(name) = current_name.take() {
                    let object_type = self.resolve_type(&current_attrs);
                    objects.push(XcatObject {
                        name,
                        object_type,
                        attributes: std::mem::take(&mut current_attrs),
                    });
                }
                current_name = Some(rest.trim().to_string());
                continue;
            }

            // Attribute line: "    key=value"
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                current_attrs.insert(key, value);
            }
        }

        // Flush last object
        if let Some(name) = current_name.take() {
            let object_type = self.resolve_type(&current_attrs);
            objects.push(XcatObject {
                name,
                object_type,
                attributes: current_attrs,
            });
        }

        Ok(objects)
    }

    /// Import parsed xCAT objects into Rustible inventory structures.
    pub fn import(&self, objects: &[XcatObject]) -> ObjectImportResult {
        let mut hosts = Vec::new();
        let mut group_members: HashMap<String, HashSet<String>> = HashMap::new();
        let mut report = MigrationReport::new("xCAT lsdef objects", "object import");

        for obj in objects {
            let obj_type = obj.object_type.as_ref().or(self.default_type.as_ref());

            match obj_type {
                Some(XcatObjectType::Node) => {
                    let host = self.map_node_to_host(obj);
                    // Derive groups from the node's groups attribute
                    if let Some(groups_str) = obj.attributes.get("groups") {
                        for g in groups_str.split(',') {
                            let g = g.trim();
                            if !g.is_empty() {
                                group_members
                                    .entry(g.to_string())
                                    .or_default()
                                    .insert(obj.name.clone());
                            }
                        }
                    }
                    hosts.push(host);
                }
                Some(XcatObjectType::Group) => {
                    // xCAT group objects define group-level attributes.
                    // We track them so they appear in derived groups.
                    if let Some(members) = obj.attributes.get("members") {
                        for m in members.split(',') {
                            let m = m.trim();
                            if !m.is_empty() {
                                group_members
                                    .entry(obj.name.clone())
                                    .or_default()
                                    .insert(m.to_string());
                            }
                        }
                    } else {
                        // Ensure the group exists even without explicit members
                        group_members.entry(obj.name.clone()).or_default();
                    }
                }
                Some(other_type) => {
                    report.findings.push(MigrationFinding {
                        source_item: format!("Object '{}' ({:?})", obj.name, other_type),
                        target_item: None,
                        status: FindingStatus::Skipped,
                        diagnostics: vec![MigrationDiagnostic {
                            category: DiagnosticCategory::UnsupportedField,
                            severity: MigrationSeverity::Warning,
                            source_path: None,
                            source_field: Some("objtype".into()),
                            message: format!(
                                "Object '{}' has type {:?} which is not mapped to inventory",
                                obj.name, other_type
                            ),
                            suggestion: None,
                        }],
                    });
                }
                None => {
                    report.findings.push(MigrationFinding {
                        source_item: format!("Object '{}'", obj.name),
                        target_item: None,
                        status: FindingStatus::Skipped,
                        diagnostics: vec![MigrationDiagnostic {
                            category: DiagnosticCategory::UnsupportedField,
                            severity: MigrationSeverity::Warning,
                            source_path: None,
                            source_field: Some("objtype".into()),
                            message: format!(
                                "Object '{}' has no determinable type; skipping",
                                obj.name
                            ),
                            suggestion: None,
                        }],
                    });
                }
            }
        }

        // Build Group structs from collected membership data
        let groups: Vec<Group> = group_members
            .into_iter()
            .map(|(name, members)| {
                let mut group = Group::new(name);
                for m in members {
                    group.hosts.insert(m);
                }
                group
            })
            .collect();

        ObjectImportResult {
            hosts,
            groups,
            report,
        }
    }

    /// Map a single xCAT node object to a Rustible [`Host`].
    fn map_node_to_host(&self, obj: &XcatObject) -> Host {
        let mut host = Host::new(&obj.name);

        // Map ip -> ansible_host
        if let Some(ip) = obj.attributes.get("ip") {
            if !ip.is_empty() {
                host.ansible_host = Some(ip.clone());
            }
        }

        // Map groups -> host.groups
        if let Some(groups_str) = obj.attributes.get("groups") {
            for g in groups_str.split(',') {
                let g = g.trim();
                if !g.is_empty() {
                    host.groups.insert(g.to_string());
                }
            }
        }

        // Map interesting xCAT attributes to host vars
        let var_mappings = [
            ("mac", "xcat_mac"),
            ("bmc", "xcat_bmc"),
            ("os", "xcat_os"),
            ("arch", "xcat_arch"),
            ("status", "xcat_status"),
            ("serial", "xcat_serial"),
            ("room", "xcat_room"),
            ("rack", "xcat_rack"),
            ("unit", "xcat_unit"),
            ("height", "xcat_height"),
            ("weight", "xcat_weight"),
            ("cpucount", "xcat_cpucount"),
            ("memory", "xcat_memory"),
            ("disksize", "xcat_disksize"),
        ];

        for (xcat_key, var_key) in &var_mappings {
            if let Some(value) = obj.attributes.get(*xcat_key) {
                if !value.is_empty() {
                    host.vars.insert(
                        var_key.to_string(),
                        serde_yaml::Value::String(value.clone()),
                    );
                }
            }
        }

        host
    }

    /// Determine the object type from attributes or the default.
    fn resolve_type(&self, attrs: &HashMap<String, String>) -> Option<XcatObjectType> {
        if let Some(objtype) = attrs.get("objtype") {
            XcatObjectType::from_str_opt(objtype)
        } else {
            self.default_type.clone()
        }
    }
}

impl Default for XcatObjectImporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LSDEF: &str = r#"
Object name: node01
    objtype=node
    arch=x86_64
    groups=compute,rack1
    ip=10.0.0.101
    mac=aa:bb:cc:dd:ee:01
    bmc=10.0.1.101
    os=rhels8.5
    status=booted

Object name: node02
    objtype=node
    arch=x86_64
    groups=compute,rack2
    ip=10.0.0.102
    mac=aa:bb:cc:dd:ee:02
    bmc=10.0.1.102
    os=rhels8.5
    status=booted
"#;

    #[test]
    fn test_parse_lsdef_basic() {
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(SAMPLE_LSDEF).unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].name, "node01");
        assert_eq!(objects[1].name, "node02");
        assert_eq!(objects[0].object_type, Some(XcatObjectType::Node));
    }

    #[test]
    fn test_parse_lsdef_attributes() {
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(SAMPLE_LSDEF).unwrap();
        let node01 = &objects[0];
        assert_eq!(
            node01.attributes.get("arch").map(|s| s.as_str()),
            Some("x86_64")
        );
        assert_eq!(
            node01.attributes.get("ip").map(|s| s.as_str()),
            Some("10.0.0.101")
        );
        assert_eq!(
            node01.attributes.get("mac").map(|s| s.as_str()),
            Some("aa:bb:cc:dd:ee:01")
        );
    }

    #[test]
    fn test_import_nodes_to_hosts() {
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(SAMPLE_LSDEF).unwrap();
        let result = importer.import(&objects);

        assert_eq!(result.hosts.len(), 2);

        let host = &result.hosts[0];
        assert_eq!(host.name, "node01");
        assert_eq!(host.ansible_host.as_deref(), Some("10.0.0.101"));
        assert!(host.groups.contains("compute"));
        assert!(host.groups.contains("rack1"));

        // Check vars mapping
        assert_eq!(
            host.vars.get("xcat_mac").and_then(|v| v.as_str()),
            Some("aa:bb:cc:dd:ee:01")
        );
        assert_eq!(
            host.vars.get("xcat_bmc").and_then(|v| v.as_str()),
            Some("10.0.1.101")
        );
        assert_eq!(
            host.vars.get("xcat_os").and_then(|v| v.as_str()),
            Some("rhels8.5")
        );
        assert_eq!(
            host.vars.get("xcat_arch").and_then(|v| v.as_str()),
            Some("x86_64")
        );
    }

    #[test]
    fn test_import_derives_groups() {
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(SAMPLE_LSDEF).unwrap();
        let result = importer.import(&objects);

        // Groups derived: compute, rack1, rack2
        let group_names: HashSet<String> = result.groups.iter().map(|g| g.name.clone()).collect();
        assert!(group_names.contains("compute"), "expected 'compute' group");
        assert!(group_names.contains("rack1"), "expected 'rack1' group");
        assert!(group_names.contains("rack2"), "expected 'rack2' group");

        // compute group should contain both nodes
        let compute = result.groups.iter().find(|g| g.name == "compute").unwrap();
        assert!(compute.hosts.contains("node01"));
        assert!(compute.hosts.contains("node02"));

        // rack1 should contain only node01
        let rack1 = result.groups.iter().find(|g| g.name == "rack1").unwrap();
        assert!(rack1.hosts.contains("node01"));
        assert!(!rack1.hosts.contains("node02"));
    }

    #[test]
    fn test_import_unsupported_type_generates_warning() {
        let input = r#"
Object name: testnet
    objtype=network
    net=10.0.0.0
    mask=255.255.255.0
"#;
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(input).unwrap();
        let result = importer.import(&objects);

        assert!(result.hosts.is_empty());
        assert!(!result.report.findings.is_empty());
        // The finding should have a warning diagnostic
        let has_warning = result.report.findings[0]
            .diagnostics
            .iter()
            .any(|d| d.severity == MigrationSeverity::Warning);
        assert!(has_warning);
    }

    #[test]
    fn test_import_with_default_type() {
        let input = r#"
Object name: gpu01
    arch=x86_64
    ip=10.0.0.201
    groups=gpu
"#;
        let importer = XcatObjectImporter::with_default_type(XcatObjectType::Node);
        let objects = importer.parse_lsdef(input).unwrap();
        let result = importer.import(&objects);

        assert_eq!(result.hosts.len(), 1);
        assert_eq!(result.hosts[0].name, "gpu01");
        assert_eq!(result.hosts[0].ansible_host.as_deref(), Some("10.0.0.201"));
    }

    #[test]
    fn test_import_group_object() {
        let input = r#"
Object name: compute
    objtype=group
    members=node01,node02,node03
"#;
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef(input).unwrap();
        let result = importer.import(&objects);

        assert!(result.hosts.is_empty());
        assert_eq!(result.groups.len(), 1);
        let group = &result.groups[0];
        assert_eq!(group.name, "compute");
        assert!(group.hosts.contains("node01"));
        assert!(group.hosts.contains("node02"));
        assert!(group.hosts.contains("node03"));
    }

    #[test]
    fn test_empty_input() {
        let importer = XcatObjectImporter::new();
        let objects = importer.parse_lsdef("").unwrap();
        assert!(objects.is_empty());
        let result = importer.import(&objects);
        assert!(result.hosts.is_empty());
        assert!(result.groups.is_empty());
    }
}
