//! Ansible compatibility verifier.
//!
//! Scans Ansible playbooks and inventories to determine compatibility
//! with Rustible, producing a migration readiness report.

use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};
use serde::{Deserialize, Serialize};

/// Category of compatibility check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatCategory {
    Module,
    ConnectionPlugin,
    CallbackPlugin,
    Filter,
    Lookup,
    Inventory,
    Syntax,
}

/// Result of an individual compatibility check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatCheck {
    pub category: CompatCategory,
    pub name: String,
    pub compatible: CompatLevel,
    pub notes: Option<String>,
}

/// Compatibility level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatLevel {
    Full,
    Partial,
    Unsupported,
}

/// Result of a compatibility verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatResult {
    pub checks: Vec<CompatCheck>,
    pub score: f64,
    pub total: usize,
    pub compatible: usize,
    pub partial: usize,
    pub unsupported: usize,
}

/// Known supported modules in Rustible.
const SUPPORTED_MODULES: &[&str] = &[
    "command", "shell", "copy", "template", "file", "lineinfile",
    "apt", "yum", "dnf", "package", "pip", "service", "systemd",
    "user", "group", "cron", "git", "debug", "set_fact", "assert",
    "fail", "pause", "wait_for", "uri", "get_url", "stat",
    "find", "fetch", "synchronize", "unarchive", "archive",
    "docker_container", "docker_image", "docker_network",
    "include_tasks", "import_tasks", "include_role", "import_role",
    "include_vars", "block", "rescue", "always",
];

/// Partially supported modules.
const PARTIAL_MODULES: &[&str] = &[
    "raw", "script", "expect", "mount", "hostname", "sysctl",
    "firewalld", "iptables", "selinux", "nmcli",
];

/// Supported Jinja2 filters.
const SUPPORTED_FILTERS: &[&str] = &[
    "default", "d", "bool", "int", "float", "string", "list",
    "dict", "join", "split", "lower", "upper", "capitalize",
    "replace", "regex_replace", "regex_search", "regex_findall",
    "basename", "dirname", "expanduser", "realpath",
    "to_json", "from_json", "to_yaml", "from_yaml",
    "to_nice_json", "to_nice_yaml", "b64encode", "b64decode",
    "hash", "password_hash", "combine", "flatten",
    "map", "select", "reject", "selectattr", "rejectattr",
    "sort", "reverse", "unique", "union", "intersect", "difference",
    "length", "count", "first", "last", "min", "max", "sum",
    "ipaddr", "ipv4", "ipv6",
];

/// Verifies Ansible playbook compatibility with Rustible.
pub struct AnsibleCompatVerifier {
    threshold: f64,
}

impl AnsibleCompatVerifier {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    /// Verify a playbook YAML string.
    pub fn verify_playbook(&self, playbook_yaml: &str) -> crate::migration::MigrationResult<MigrationReport> {
        let plays: Vec<serde_yaml::Value> = serde_yaml::from_str(playbook_yaml)
            .map_err(|e| crate::migration::MigrationError::ParseError(format!("YAML: {}", e)))?;

        let mut checks = Vec::new();

        for play in &plays {
            // Check modules used in tasks
            if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                for task in tasks {
                    if let Some(mapping) = task.as_mapping() {
                        for (key, _) in mapping {
                            if let Some(module_name) = key.as_str() {
                                if matches!(module_name,
                                    "name" | "when" | "tags" | "register" | "ignore_errors" |
                                    "become" | "become_user" | "delegate_to" | "notify" |
                                    "loop" | "with_items" | "vars" | "changed_when" |
                                    "failed_when" | "no_log" | "retries" | "delay" |
                                    "until" | "environment" | "any_errors_fatal" |
                                    "listen" | "block" | "rescue" | "always"
                                ) {
                                    continue;
                                }
                                checks.push(self.check_module(module_name));
                            }
                        }
                    }
                }
            }

            // Check connection plugin
            if let Some(conn) = play.get("connection").and_then(|c| c.as_str()) {
                checks.push(self.check_connection(conn));
            }
        }

        // Check filters used in templates
        let content_str = playbook_yaml;
        let filter_re = regex::Regex::new(r"\|\s*(\w+)").unwrap();
        for cap in filter_re.captures_iter(content_str) {
            if let Some(filter_name) = cap.get(1) {
                checks.push(self.check_filter(filter_name.as_str()));
            }
        }

        // Deduplicate checks by name+category
        checks.sort_by(|a, b| a.name.cmp(&b.name));
        checks.dedup_by(|a, b| a.name == b.name && a.category == b.category);

        let result = self.compute_result(&checks);
        let mut report = self.build_report(&result);
        report.compute_outcome(self.threshold);
        Ok(report)
    }

    fn check_module(&self, name: &str) -> CompatCheck {
        let compatible = if SUPPORTED_MODULES.contains(&name) {
            CompatLevel::Full
        } else if PARTIAL_MODULES.contains(&name) {
            CompatLevel::Partial
        } else {
            CompatLevel::Unsupported
        };
        CompatCheck {
            category: CompatCategory::Module,
            name: name.to_string(),
            compatible,
            notes: if compatible == CompatLevel::Unsupported {
                Some(format!("Module '{}' is not yet supported in Rustible", name))
            } else {
                None
            },
        }
    }

    fn check_connection(&self, name: &str) -> CompatCheck {
        let compatible = match name {
            "ssh" | "local" | "paramiko" => CompatLevel::Full,
            "winrm" | "psrp" => CompatLevel::Partial,
            _ => CompatLevel::Unsupported,
        };
        CompatCheck {
            category: CompatCategory::ConnectionPlugin,
            name: name.to_string(),
            compatible,
            notes: None,
        }
    }

    fn check_filter(&self, name: &str) -> CompatCheck {
        let compatible = if SUPPORTED_FILTERS.contains(&name) {
            CompatLevel::Full
        } else {
            CompatLevel::Unsupported
        };
        CompatCheck {
            category: CompatCategory::Filter,
            name: name.to_string(),
            compatible,
            notes: None,
        }
    }

    fn compute_result(&self, checks: &[CompatCheck]) -> CompatResult {
        let total = checks.len();
        let compatible = checks.iter().filter(|c| c.compatible == CompatLevel::Full).count();
        let partial = checks.iter().filter(|c| c.compatible == CompatLevel::Partial).count();
        let unsupported = checks.iter().filter(|c| c.compatible == CompatLevel::Unsupported).count();
        let score = if total > 0 {
            (compatible as f64 + 0.5 * partial as f64) / total as f64 * 100.0
        } else {
            100.0
        };
        CompatResult { checks: checks.to_vec(), score, total, compatible, partial, unsupported }
    }

    fn build_report(&self, result: &CompatResult) -> MigrationReport {
        let mut report = MigrationReport::new(
            "Ansible Compatibility Check",
            "Ansible playbook",
            "Rustible",
        );

        // Module compatibility finding
        let module_checks: Vec<&CompatCheck> = result.checks.iter()
            .filter(|c| c.category == CompatCategory::Module)
            .collect();
        if !module_checks.is_empty() {
            let unsupported: Vec<&CompatCheck> = module_checks.iter()
                .filter(|c| c.compatible == CompatLevel::Unsupported)
                .copied()
                .collect();
            report.add_finding(MigrationFinding {
                name: "Module Compatibility".into(),
                status: if unsupported.is_empty() { FindingStatus::Pass } else { FindingStatus::Partial },
                severity: MigrationSeverity::Warning,
                diagnostics: unsupported.iter().map(|c| MigrationDiagnostic {
                    category: DiagnosticCategory::UnsupportedFeature,
                    severity: MigrationSeverity::Warning,
                    message: format!("Module '{}' not supported", c.name),
                    context: c.notes.clone(),
                }).collect(),
            });
        }

        // Connection compatibility finding
        let conn_checks: Vec<&CompatCheck> = result.checks.iter()
            .filter(|c| c.category == CompatCategory::ConnectionPlugin)
            .collect();
        if !conn_checks.is_empty() {
            let unsupported: Vec<&CompatCheck> = conn_checks.iter()
                .filter(|c| c.compatible == CompatLevel::Unsupported)
                .copied()
                .collect();
            report.add_finding(MigrationFinding {
                name: "Connection Plugin Compatibility".into(),
                status: if unsupported.is_empty() { FindingStatus::Pass } else { FindingStatus::Fail },
                severity: MigrationSeverity::Error,
                diagnostics: unsupported.iter().map(|c| MigrationDiagnostic {
                    category: DiagnosticCategory::UnsupportedFeature,
                    severity: MigrationSeverity::Error,
                    message: format!("Connection plugin '{}' not supported", c.name),
                    context: None,
                }).collect(),
            });
        }

        // Filter compatibility finding
        let filter_checks: Vec<&CompatCheck> = result.checks.iter()
            .filter(|c| c.category == CompatCategory::Filter)
            .collect();
        if !filter_checks.is_empty() {
            let unsupported: Vec<&CompatCheck> = filter_checks.iter()
                .filter(|c| c.compatible == CompatLevel::Unsupported)
                .copied()
                .collect();
            report.add_finding(MigrationFinding {
                name: "Filter Compatibility".into(),
                status: if unsupported.is_empty() { FindingStatus::Pass } else { FindingStatus::Partial },
                severity: MigrationSeverity::Info,
                diagnostics: unsupported.iter().map(|c| MigrationDiagnostic {
                    category: DiagnosticCategory::UnsupportedFeature,
                    severity: MigrationSeverity::Info,
                    message: format!("Filter '{}' not supported", c.name),
                    context: None,
                }).collect(),
            });
        }

        // Overall score finding
        report.add_finding(MigrationFinding {
            name: "Overall Compatibility Score".into(),
            status: if result.score >= self.threshold { FindingStatus::Pass } else { FindingStatus::Fail },
            severity: MigrationSeverity::Info,
            diagnostics: vec![MigrationDiagnostic {
                category: DiagnosticCategory::CompatibilityIssue,
                severity: MigrationSeverity::Info,
                message: format!(
                    "Score: {:.1}% ({} full, {} partial, {} unsupported of {} total)",
                    result.score, result.compatible, result.partial, result.unsupported, result.total
                ),
                context: None,
            }],
        });

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fully_compatible_playbook() {
        let yaml = r#"---
- name: Test play
  hosts: all
  tasks:
    - name: Run command
      command: echo hello
    - name: Copy file
      copy:
        src: /tmp/a
        dest: /tmp/b
    - name: Debug msg
      debug:
        msg: "{{ greeting | default('hi') }}"
"#;
        let verifier = AnsibleCompatVerifier::new(80.0);
        let report = verifier.verify_playbook(yaml).unwrap();
        assert_eq!(report.outcome, Some(crate::migration::MigrationOutcome::Pass));
    }

    #[test]
    fn test_partially_compatible_playbook() {
        let yaml = r#"---
- name: Test play
  hosts: all
  tasks:
    - name: Use unsupported module
      custom_module:
        arg: value
    - name: Use supported module
      command: echo hello
"#;
        let verifier = AnsibleCompatVerifier::new(80.0);
        let report = verifier.verify_playbook(yaml).unwrap();
        // custom_module is unsupported, so score < 100
        let summary = report.summary.as_ref().unwrap();
        assert!(summary.score < 100.0);
    }

    #[test]
    fn test_empty_playbook() {
        let yaml = "---\n- name: Empty\n  hosts: all\n  tasks: []\n";
        let verifier = AnsibleCompatVerifier::new(80.0);
        let report = verifier.verify_playbook(yaml).unwrap();
        assert_eq!(report.outcome, Some(crate::migration::MigrationOutcome::Pass));
    }
}
