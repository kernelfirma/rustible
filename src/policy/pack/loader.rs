//! Policy pack loader.
//!
//! Parses a YAML manifest into a [`PolicyPack`] containing concrete
//! [`PackRule`] instances that can be evaluated against playbook data.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::manifest::PolicyPackManifest;
use crate::policy::RuleSeverity;

/// A loaded policy pack ready for evaluation.
#[derive(Debug, Clone)]
pub struct PolicyPack {
    /// The manifest that describes this pack.
    pub manifest: PolicyPackManifest,
    /// Concrete rules derived from the manifest.
    pub rules: Vec<PackRule>,
}

/// A single rule within a policy pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackRule {
    /// Rule name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Severity when the rule is violated.
    pub severity: RuleSeverity,
    /// The check to perform.
    pub check: RuleCheck,
}

/// The type of check a pack rule performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCheck {
    /// Deny usage of any of the listed modules.
    ModuleBlacklist(Vec<String>),
    /// Require that a specific tag is present on every play.
    RequireTag(String),
    /// Enforce a maximum number of tasks per play.
    MaxTasks(usize),
    /// Require that every task has a `name` field.
    RequireName,
    /// A custom check identified by a string key (for extensibility).
    Custom(String),
}

/// Loader that converts YAML manifests into [`PolicyPack`] instances.
pub struct PackLoader;

impl PackLoader {
    /// Parse a YAML string into a [`PolicyPack`].
    ///
    /// The YAML is expected to deserialise into a [`PolicyPackManifest`].  The
    /// loader then derives concrete [`PackRule`] instances from the rule names
    /// listed in the manifest.
    pub fn load_from_manifest(manifest_yaml: &str) -> Result<PolicyPack, String> {
        let manifest: PolicyPackManifest = serde_yaml::from_str(manifest_yaml)
            .map_err(|e| format!("invalid manifest YAML: {}", e))?;

        let rules = manifest
            .rules
            .iter()
            .map(|rule_name| Self::rule_from_name(rule_name))
            .collect();

        Ok(PolicyPack { manifest, rules })
    }

    /// Build a [`PolicyPack`] directly from an already-parsed manifest.
    pub fn load_from_parsed(manifest: PolicyPackManifest) -> PolicyPack {
        let rules = manifest
            .rules
            .iter()
            .map(|rule_name| Self::rule_from_name(rule_name))
            .collect();

        PolicyPack { manifest, rules }
    }

    /// Map a rule name to a concrete [`PackRule`].
    fn rule_from_name(name: &str) -> PackRule {
        match name {
            "no-shell" => PackRule {
                name: "no-shell".into(),
                description: "Deny use of the shell module".into(),
                severity: RuleSeverity::Error,
                check: RuleCheck::ModuleBlacklist(vec!["shell".into()]),
            },
            "no-raw" => PackRule {
                name: "no-raw".into(),
                description: "Deny use of the raw module".into(),
                severity: RuleSeverity::Error,
                check: RuleCheck::ModuleBlacklist(vec!["raw".into()]),
            },
            "require-become-explicit" => PackRule {
                name: "require-become-explicit".into(),
                description: "Require explicit become declaration".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::Custom("require-become-explicit".into()),
            },
            "require-tags" => PackRule {
                name: "require-tags".into(),
                description: "Require tags on every play".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::RequireTag("tags".into()),
            },
            "max-tasks" => PackRule {
                name: "max-tasks".into(),
                description: "Limit the number of tasks per play".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::MaxTasks(20),
            },
            "require-name" => PackRule {
                name: "require-name".into(),
                description: "Require a name on every task".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::RequireName,
            },
            "max-forks" => PackRule {
                name: "max-forks".into(),
                description: "Warn when forks exceed a safe limit".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::Custom("max-forks".into()),
            },
            "require-limit" => PackRule {
                name: "require-limit".into(),
                description: "Require a limit pattern for production".into(),
                severity: RuleSeverity::Warning,
                check: RuleCheck::Custom("require-limit".into()),
            },
            "deny-localhost-in-prod" => PackRule {
                name: "deny-localhost-in-prod".into(),
                description: "Deny localhost as a target in production plays".into(),
                severity: RuleSeverity::Error,
                check: RuleCheck::Custom("deny-localhost-in-prod".into()),
            },
            other => PackRule {
                name: other.into(),
                description: format!("Custom rule: {}", other),
                severity: RuleSeverity::Info,
                check: RuleCheck::Custom(other.into()),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Rule evaluation helpers
// ---------------------------------------------------------------------------

impl PackRule {
    /// Evaluate this rule against the given playbook JSON, returning a list of
    /// violation messages (empty means the rule passed).
    pub fn evaluate(&self, input: &Value) -> Vec<String> {
        match &self.check {
            RuleCheck::ModuleBlacklist(modules) => eval_module_blacklist(modules, input),
            RuleCheck::RequireTag(tag_field) => eval_require_tag(tag_field, input),
            RuleCheck::MaxTasks(max) => eval_max_tasks(*max, input),
            RuleCheck::RequireName => eval_require_name(input),
            RuleCheck::Custom(_key) => {
                // Custom checks are extensibility points; they pass by default.
                Vec::new()
            }
        }
    }
}

fn plays_from_input(input: &Value) -> Vec<&Value> {
    if let Some(arr) = input.as_array() {
        arr.iter().collect()
    } else if let Some(arr) = input.get("plays").and_then(|v| v.as_array()) {
        arr.iter().collect()
    } else {
        vec![input]
    }
}

fn tasks_from_play(play: &Value) -> Vec<&Value> {
    play.get("tasks")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default()
}

fn eval_module_blacklist(modules: &[String], input: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    for play in plays_from_input(input) {
        for task in tasks_from_play(play) {
            for module in modules {
                if task.get(module.as_str()).is_some() {
                    let task_name = task
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unnamed>");
                    violations.push(format!(
                        "task '{}' uses denied module '{}'",
                        task_name, module
                    ));
                }
            }
        }
    }
    violations
}

fn eval_require_tag(tag_field: &str, input: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    for (i, play) in plays_from_input(input).iter().enumerate() {
        if play.get(tag_field).is_none() {
            let play_name = play
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unnamed>");
            violations.push(format!(
                "play '{}' (index {}) is missing required field '{}'",
                play_name, i, tag_field
            ));
        }
    }
    violations
}

fn eval_max_tasks(max: usize, input: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    for (i, play) in plays_from_input(input).iter().enumerate() {
        let tasks = tasks_from_play(play);
        if tasks.len() > max {
            let play_name = play
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unnamed>");
            violations.push(format!(
                "play '{}' (index {}) has {} tasks, exceeding maximum of {}",
                play_name,
                i,
                tasks.len(),
                max
            ));
        }
    }
    violations
}

fn eval_require_name(input: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    for play in plays_from_input(input) {
        for (j, task) in tasks_from_play(play).iter().enumerate() {
            if task.get("name").is_none() {
                violations.push(format!("task at index {} is missing a 'name' field", j));
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_manifest_yaml() -> String {
        r#"
name: test-security
version: "1.0.0"
description: Test security pack
category: Security
rules:
  - no-shell
  - no-raw
parameters: []
"#
        .to_string()
    }

    #[test]
    fn test_load_from_manifest_yaml() {
        let pack =
            PackLoader::load_from_manifest(&sample_manifest_yaml()).expect("should parse manifest");

        assert_eq!(pack.manifest.name, "test-security");
        assert_eq!(pack.rules.len(), 2);
        assert_eq!(pack.rules[0].name, "no-shell");
        assert_eq!(pack.rules[1].name, "no-raw");
    }

    #[test]
    fn test_load_from_manifest_invalid_yaml() {
        let result = PackLoader::load_from_manifest("{{invalid yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_module_blacklist_rule_detects_violations() {
        let rule = PackRule {
            name: "no-shell".into(),
            description: "Deny shell".into(),
            severity: RuleSeverity::Error,
            check: RuleCheck::ModuleBlacklist(vec!["shell".into()]),
        };

        let input = json!([{
            "name": "Test play",
            "hosts": "all",
            "tasks": [
                {"name": "Bad task", "shell": "echo hello"},
                {"name": "Good task", "debug": {"msg": "hi"}}
            ]
        }]);

        let violations = rule.evaluate(&input);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("Bad task"));
        assert!(violations[0].contains("shell"));
    }

    #[test]
    fn test_require_name_rule() {
        let rule = PackRule {
            name: "require-name".into(),
            description: "Require name".into(),
            severity: RuleSeverity::Warning,
            check: RuleCheck::RequireName,
        };

        let input = json!([{
            "name": "Play",
            "hosts": "all",
            "tasks": [
                {"debug": {"msg": "no name"}},
                {"name": "Has name", "debug": {"msg": "ok"}}
            ]
        }]);

        let violations = rule.evaluate(&input);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("index 0"));
    }

    #[test]
    fn test_max_tasks_rule() {
        let rule = PackRule {
            name: "max-tasks".into(),
            description: "Max 1 task".into(),
            severity: RuleSeverity::Warning,
            check: RuleCheck::MaxTasks(1),
        };

        let input = json!([{
            "name": "Big play",
            "hosts": "all",
            "tasks": [
                {"name": "t1", "debug": {}},
                {"name": "t2", "debug": {}}
            ]
        }]);

        let violations = rule.evaluate(&input);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("2 tasks"));
    }

    #[test]
    fn test_require_tag_rule() {
        let rule = PackRule {
            name: "require-tags".into(),
            description: "Tags required".into(),
            severity: RuleSeverity::Warning,
            check: RuleCheck::RequireTag("tags".into()),
        };

        let input = json!([
            {"name": "No tags", "hosts": "all", "tasks": []},
            {"name": "Has tags", "hosts": "all", "tags": ["deploy"], "tasks": []}
        ]);

        let violations = rule.evaluate(&input);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("No tags"));
    }
}
