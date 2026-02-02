//! Policy-as-code enforcement (OPA/Rego and built-in Sentinel-like rules).
//!
//! This module provides an optional policy gate that evaluates playbooks
//! against Open Policy Agent (OPA) rules or built-in declarative rules
//! before execution.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("OPA binary not found in PATH")]
    OpaNotFound,

    #[error("OPA evaluation failed: {0}")]
    OpaEvalFailed(String),

    #[error("Invalid OPA output: {0}")]
    InvalidOutput(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid regex pattern '{pattern}': {source}")]
    InvalidRegex {
        pattern: String,
        source: regex::Error,
    },
}

// ---------------------------------------------------------------------------
// Policy engine selection
// ---------------------------------------------------------------------------

/// Which policy engine to use for evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyEngine {
    Opa,
    Builtin,
}

// ---------------------------------------------------------------------------
// Built-in rule types
// ---------------------------------------------------------------------------

/// Severity of a built-in rule violation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuleSeverity {
    Error,
    Warning,
    Info,
}

/// A condition that a built-in rule checks against the playbook input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCondition {
    /// Deny if a field matches a pattern.
    DenyFieldPattern { field: String, pattern: String },
    /// Require that a field exists.
    RequireField { field: String },
    /// Deny if a module is used.
    DenyModule { module_name: String },
    /// Require `become: false` for hosts matching the pattern.
    DenyPrivilegeEscalation { host_pattern: String },
    /// Enforce maximum number of tasks per play.
    MaxTasksPerPlay { max: usize },
}

/// A declarative rule that can be evaluated without OPA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinRule {
    pub name: String,
    pub description: String,
    pub severity: RuleSeverity,
    pub condition: RuleCondition,
}

// ---------------------------------------------------------------------------
// PolicySet — unified evaluation entry-point
// ---------------------------------------------------------------------------

/// A set of policies that can be evaluated against playbook input using either
/// the OPA engine or built-in declarative rules.
pub struct PolicySet {
    pub engine: PolicyEngine,
    pub opa_policy_path: Option<PathBuf>,
    pub builtin_rules: Vec<BuiltinRule>,
}

impl PolicySet {
    /// Evaluate the policy set against the given playbook input.
    ///
    /// When the engine is `Opa`, delegates to `evaluate_opa_policy` using the
    /// configured `opa_policy_path`.  When the engine is `Builtin`, evaluates
    /// all `builtin_rules` against `input`.
    pub fn evaluate(&self, input: &Value) -> PolicyResult<PolicyDecision> {
        match &self.engine {
            PolicyEngine::Opa => {
                let path = self.opa_policy_path.as_deref().ok_or_else(|| {
                    PolicyError::OpaEvalFailed(
                        "No OPA policy path configured".to_string(),
                    )
                })?;
                evaluate_opa_policy(path, "data.rustible.deny", input)
            }
            PolicyEngine::Builtin => evaluate_builtin_rules(&self.builtin_rules, input),
        }
    }
}

pub type PolicyResult<T> = Result<T, PolicyError>;

/// A decision returned by policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub raw: Option<Value>,
}

impl PolicyDecision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reasons: Vec::new(),
            raw: None,
        }
    }
}

/// Evaluate an OPA policy (Rego) using the `opa` CLI.
pub fn evaluate_opa_policy(
    policy_path: &Path,
    query: &str,
    input: &Value,
) -> PolicyResult<PolicyDecision> {
    let temp = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&temp, input)?;

    let output = Command::new("opa")
        .arg("eval")
        .arg("-f")
        .arg("json")
        .arg("-d")
        .arg(policy_path)
        .arg("-i")
        .arg(temp.path())
        .arg(query)
        .output();

    let output = match output {
        Ok(out) => out,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                return Err(PolicyError::OpaNotFound);
            }
            return Err(PolicyError::Io(err));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyError::OpaEvalFailed(stderr.trim().to_string()));
    }

    let response: Value = serde_json::from_slice(&output.stdout)?;
    let value = response
        .get("result")
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
        .and_then(|r| r.get("expressions"))
        .and_then(|e| e.as_array())
        .and_then(|e| e.first())
        .and_then(|e| e.get("value"))
        .cloned()
        .ok_or_else(|| PolicyError::InvalidOutput("Missing result value".to_string()))?;

    Ok(decode_policy_value(value))
}

// ---------------------------------------------------------------------------
// Built-in rule evaluation
// ---------------------------------------------------------------------------

/// Evaluate a set of built-in declarative rules against playbook input JSON.
///
/// The `input` is expected to be either a JSON array of plays or a JSON object
/// that contains a `plays` array.  Each play may contain a `tasks` array and
/// metadata such as `hosts` and `become`.
pub fn evaluate_builtin_rules(
    rules: &[BuiltinRule],
    input: &Value,
) -> PolicyResult<PolicyDecision> {
    let mut reasons: Vec<String> = Vec::new();

    for rule in rules {
        let mut violations = evaluate_condition(&rule.condition, input)?;
        for v in &mut violations {
            *v = format!("[{}] {}: {}", severity_label(&rule.severity), rule.name, v);
        }
        reasons.extend(violations);
    }

    Ok(PolicyDecision {
        allowed: reasons.is_empty(),
        reasons,
        raw: None,
    })
}

fn severity_label(s: &RuleSeverity) -> &'static str {
    match s {
        RuleSeverity::Error => "ERROR",
        RuleSeverity::Warning => "WARN",
        RuleSeverity::Info => "INFO",
    }
}

/// Evaluate a single condition, returning a list of violation messages (empty
/// means the rule passed).
fn evaluate_condition(
    condition: &RuleCondition,
    input: &Value,
) -> PolicyResult<Vec<String>> {
    match condition {
        RuleCondition::DenyFieldPattern { field, pattern } => {
            eval_deny_field_pattern(field, pattern, input)
        }
        RuleCondition::RequireField { field } => eval_require_field(field, input),
        RuleCondition::DenyModule { module_name } => eval_deny_module(module_name, input),
        RuleCondition::DenyPrivilegeEscalation { host_pattern } => {
            eval_deny_privilege_escalation(host_pattern, input)
        }
        RuleCondition::MaxTasksPerPlay { max } => eval_max_tasks_per_play(*max, input),
    }
}

// --- Individual condition evaluators ---

fn eval_deny_field_pattern(
    field: &str,
    pattern: &str,
    input: &Value,
) -> PolicyResult<Vec<String>> {
    let re = Regex::new(pattern).map_err(|e| PolicyError::InvalidRegex {
        pattern: pattern.to_string(),
        source: e,
    })?;
    let mut violations = Vec::new();
    let values = collect_field_values(field, input);
    for val in values {
        if re.is_match(&val) {
            violations.push(format!(
                "field '{}' value '{}' matches denied pattern '{}'",
                field, val, pattern
            ));
        }
    }
    Ok(violations)
}

fn eval_require_field(field: &str, input: &Value) -> PolicyResult<Vec<String>> {
    let values = collect_field_values(field, input);
    if values.is_empty() {
        Ok(vec![format!("required field '{}' is missing", field)])
    } else {
        Ok(Vec::new())
    }
}

fn eval_deny_module(module_name: &str, input: &Value) -> PolicyResult<Vec<String>> {
    let plays = plays_from_input(input);
    let mut violations = Vec::new();
    for play in plays {
        let tasks = tasks_from_play(play);
        for task in tasks {
            if task.get(module_name).is_some() {
                let task_name = task
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>");
                violations.push(format!(
                    "task '{}' uses denied module '{}'",
                    task_name, module_name
                ));
            }
        }
    }
    Ok(violations)
}

fn eval_deny_privilege_escalation(
    host_pattern: &str,
    input: &Value,
) -> PolicyResult<Vec<String>> {
    let re = Regex::new(host_pattern).map_err(|e| PolicyError::InvalidRegex {
        pattern: host_pattern.to_string(),
        source: e,
    })?;
    let plays = plays_from_input(input);
    let mut violations = Vec::new();
    for play in plays {
        let hosts = play
            .get("hosts")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if re.is_match(hosts) {
            let play_become = play
                .get("become")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if play_become {
                violations.push(format!(
                    "privilege escalation (become: true) denied for hosts '{}'",
                    hosts
                ));
            }
            // Also check individual tasks within the play
            let tasks = tasks_from_play(play);
            for task in tasks {
                let task_become = task
                    .get("become")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if task_become {
                    let task_name = task
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unnamed>");
                    violations.push(format!(
                        "task '{}' uses privilege escalation (become: true) on hosts '{}'",
                        task_name, hosts
                    ));
                }
            }
        }
    }
    Ok(violations)
}

fn eval_max_tasks_per_play(max: usize, input: &Value) -> PolicyResult<Vec<String>> {
    let plays = plays_from_input(input);
    let mut violations = Vec::new();
    for (i, play) in plays.iter().enumerate() {
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
    Ok(violations)
}

// --- Helpers ---

/// Extract the list of plays from the input.  Accepts either a JSON array
/// (each element is a play) or an object with a `plays` key.
fn plays_from_input(input: &Value) -> Vec<&Value> {
    if let Some(arr) = input.as_array() {
        arr.iter().collect()
    } else if let Some(arr) = input.get("plays").and_then(|v| v.as_array()) {
        arr.iter().collect()
    } else {
        // Treat the whole input as a single play
        vec![input]
    }
}

/// Extract the tasks list from a play object.
fn tasks_from_play(play: &Value) -> Vec<&Value> {
    play.get("tasks")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default()
}

/// Walk the JSON structure and collect string representations of all values
/// found at the given dot-separated field path.
fn collect_field_values(field: &str, input: &Value) -> Vec<String> {
    let parts: Vec<&str> = field.split('.').collect();
    let mut results = Vec::new();
    collect_field_recursive(&parts, input, &mut results);
    results
}

fn collect_field_recursive(parts: &[&str], value: &Value, out: &mut Vec<String>) {
    if parts.is_empty() {
        match value {
            Value::String(s) => out.push(s.clone()),
            Value::Number(n) => out.push(n.to_string()),
            Value::Bool(b) => out.push(b.to_string()),
            Value::Null => out.push("null".to_string()),
            _ => out.push(value.to_string()),
        }
        return;
    }

    let key = parts[0];
    let rest = &parts[1..];

    match value {
        Value::Object(map) => {
            if let Some(child) = map.get(key) {
                collect_field_recursive(rest, child, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_field_recursive(parts, item, out);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// OPA support (original)
// ---------------------------------------------------------------------------

fn decode_policy_value(value: Value) -> PolicyDecision {
    match value {
        Value::Bool(allowed) => PolicyDecision {
            allowed,
            reasons: Vec::new(),
            raw: None,
        },
        Value::Array(arr) => {
            let reasons = arr
                .into_iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>();
            PolicyDecision {
                allowed: reasons.is_empty(),
                reasons,
                raw: None,
            }
        }
        Value::String(reason) => PolicyDecision {
            allowed: false,
            reasons: vec![reason],
            raw: None,
        },
        Value::Object(map) => {
            let allow = map
                .get("allow")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let deny = map
                .get("deny")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let reasons = if !deny.is_empty() {
                deny
            } else {
                map.get("reasons")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };

            PolicyDecision {
                allowed: allow && reasons.is_empty(),
                reasons,
                raw: Some(Value::Object(map)),
            }
        }
        other => PolicyDecision {
            allowed: false,
            reasons: vec![format!("Unsupported policy response: {}", other)],
            raw: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // Original OPA decode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_decode_policy_value_bool() {
        let decision = decode_policy_value(Value::Bool(true));
        assert!(decision.allowed);
    }

    #[test]
    fn test_decode_policy_value_object() {
        let value = json!({"allow": false, "deny": ["nope"]});
        let decision = decode_policy_value(value);
        assert!(!decision.allowed);
        assert_eq!(decision.reasons, vec!["nope".to_string()]);
    }

    // -----------------------------------------------------------------------
    // Built-in rule evaluation tests
    // -----------------------------------------------------------------------

    fn sample_playbook() -> Value {
        json!([
            {
                "name": "Setup web servers",
                "hosts": "webservers",
                "become": true,
                "tasks": [
                    {"name": "Install nginx", "apt": {"name": "nginx", "state": "present"}},
                    {"name": "Copy config", "copy": {"src": "nginx.conf", "dest": "/etc/nginx/nginx.conf"}},
                    {"name": "Start nginx", "service": {"name": "nginx", "state": "started"}}
                ]
            },
            {
                "name": "Configure database",
                "hosts": "dbservers",
                "become": false,
                "tasks": [
                    {"name": "Check status", "shell": "pg_isready"}
                ]
            }
        ])
    }

    #[test]
    fn test_deny_field_pattern_matches() {
        let rules = vec![BuiltinRule {
            name: "no-shell".into(),
            description: "Deny shell module usage via field pattern".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyFieldPattern {
                field: "tasks.shell".into(),
                pattern: ".*".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        assert!(!decision.reasons.is_empty());
    }

    #[test]
    fn test_deny_field_pattern_no_match() {
        let rules = vec![BuiltinRule {
            name: "no-raw".into(),
            description: "Deny raw module".into(),
            severity: RuleSeverity::Warning,
            condition: RuleCondition::DenyFieldPattern {
                field: "tasks.raw".into(),
                pattern: ".*".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.allowed);
    }

    #[test]
    fn test_require_field_present() {
        let rules = vec![BuiltinRule {
            name: "require-hosts".into(),
            description: "Require hosts field".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::RequireField {
                field: "hosts".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.allowed);
    }

    #[test]
    fn test_require_field_missing() {
        let rules = vec![BuiltinRule {
            name: "require-tags".into(),
            description: "Require tags field".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::RequireField {
                field: "tags".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        assert!(decision.reasons[0].contains("required field 'tags' is missing"));
    }

    #[test]
    fn test_deny_module() {
        let rules = vec![BuiltinRule {
            name: "no-shell".into(),
            description: "Shell module is forbidden".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyModule {
                module_name: "shell".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        assert!(decision.reasons[0].contains("denied module 'shell'"));
    }

    #[test]
    fn test_deny_module_not_used() {
        let rules = vec![BuiltinRule {
            name: "no-raw".into(),
            description: "Raw module is forbidden".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyModule {
                module_name: "raw".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.allowed);
    }

    #[test]
    fn test_deny_privilege_escalation() {
        let rules = vec![BuiltinRule {
            name: "no-become-web".into(),
            description: "No privilege escalation on webservers".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyPrivilegeEscalation {
                host_pattern: "web.*".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        assert!(decision.reasons[0].contains("privilege escalation"));
        assert!(decision.reasons[0].contains("webservers"));
    }

    #[test]
    fn test_deny_privilege_escalation_no_match() {
        let rules = vec![BuiltinRule {
            name: "no-become-staging".into(),
            description: "No privilege escalation on staging".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyPrivilegeEscalation {
                host_pattern: "staging.*".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.allowed);
    }

    #[test]
    fn test_max_tasks_per_play_within_limit() {
        let rules = vec![BuiltinRule {
            name: "max-tasks".into(),
            description: "Limit tasks per play".into(),
            severity: RuleSeverity::Warning,
            condition: RuleCondition::MaxTasksPerPlay { max: 5 },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.allowed);
    }

    #[test]
    fn test_max_tasks_per_play_exceeded() {
        let rules = vec![BuiltinRule {
            name: "max-tasks".into(),
            description: "Limit tasks per play".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::MaxTasksPerPlay { max: 2 },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        assert!(decision.reasons[0].contains("3 tasks, exceeding maximum of 2"));
    }

    #[test]
    fn test_multiple_rules() {
        let rules = vec![
            BuiltinRule {
                name: "no-shell".into(),
                description: "Deny shell".into(),
                severity: RuleSeverity::Error,
                condition: RuleCondition::DenyModule {
                    module_name: "shell".into(),
                },
            },
            BuiltinRule {
                name: "max-tasks".into(),
                description: "Max 2 tasks".into(),
                severity: RuleSeverity::Warning,
                condition: RuleCondition::MaxTasksPerPlay { max: 2 },
            },
        ];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(!decision.allowed);
        // Both rules should fire
        assert!(decision.reasons.len() >= 2);
    }

    #[test]
    fn test_empty_rules_allows() {
        let decision = evaluate_builtin_rules(&[], &sample_playbook()).unwrap();
        assert!(decision.allowed);
        assert!(decision.reasons.is_empty());
    }

    #[test]
    fn test_invalid_regex_returns_error() {
        let rules = vec![BuiltinRule {
            name: "bad-regex".into(),
            description: "Has invalid regex".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyFieldPattern {
                field: "hosts".into(),
                pattern: "[invalid".into(),
            },
        }];
        let result = evaluate_builtin_rules(&rules, &sample_playbook());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid regex"));
    }

    #[test]
    fn test_severity_labels_in_output() {
        let rules = vec![BuiltinRule {
            name: "test-rule".into(),
            description: "desc".into(),
            severity: RuleSeverity::Warning,
            condition: RuleCondition::DenyModule {
                module_name: "shell".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &sample_playbook()).unwrap();
        assert!(decision.reasons[0].starts_with("[WARN]"));
    }

    #[test]
    fn test_policy_set_builtin_dispatch() {
        let ps = PolicySet {
            engine: PolicyEngine::Builtin,
            opa_policy_path: None,
            builtin_rules: vec![BuiltinRule {
                name: "no-shell".into(),
                description: "Deny shell".into(),
                severity: RuleSeverity::Error,
                condition: RuleCondition::DenyModule {
                    module_name: "shell".into(),
                },
            }],
        };
        let decision = ps.evaluate(&sample_playbook()).unwrap();
        assert!(!decision.allowed);
    }

    #[test]
    fn test_policy_set_opa_missing_path() {
        let ps = PolicySet {
            engine: PolicyEngine::Opa,
            opa_policy_path: None,
            builtin_rules: vec![],
        };
        let result = ps.evaluate(&sample_playbook());
        assert!(result.is_err());
    }

    #[test]
    fn test_deny_privilege_escalation_task_level() {
        let input = json!([{
            "name": "Deploy",
            "hosts": "production",
            "become": false,
            "tasks": [
                {"name": "Dangerous task", "shell": "rm -rf /", "become": true}
            ]
        }]);
        let rules = vec![BuiltinRule {
            name: "no-become-prod".into(),
            description: "No become on production".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::DenyPrivilegeEscalation {
                host_pattern: "production".into(),
            },
        }];
        let decision = evaluate_builtin_rules(&rules, &input).unwrap();
        assert!(!decision.allowed);
        assert!(decision.reasons[0].contains("Dangerous task"));
    }

    #[test]
    fn test_plays_from_object_input() {
        let input = json!({
            "plays": [{
                "name": "Single play",
                "hosts": "all",
                "tasks": [
                    {"name": "Ping", "ping": {}}
                ]
            }]
        });
        let rules = vec![BuiltinRule {
            name: "max-tasks".into(),
            description: "Max 0 tasks".into(),
            severity: RuleSeverity::Error,
            condition: RuleCondition::MaxTasksPerPlay { max: 0 },
        }];
        let decision = evaluate_builtin_rules(&rules, &input).unwrap();
        assert!(!decision.allowed);
    }

    #[test]
    fn test_collect_nested_field() {
        let input = json!([{"tasks": [{"apt": {"name": "nginx"}}]}]);
        let values = collect_field_values("tasks.apt.name", &input);
        assert_eq!(values, vec!["nginx"]);
    }
}
