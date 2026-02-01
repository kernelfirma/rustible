//! Policy-as-code enforcement (OPA/Rego).
//!
//! This module provides an optional policy gate that evaluates playbooks
//! against Open Policy Agent (OPA) rules before execution.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
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

    #[test]
    fn test_decode_policy_value_bool() {
        let decision = decode_policy_value(Value::Bool(true));
        assert!(decision.allowed);
    }

    #[test]
    fn test_decode_policy_value_object() {
        let value = serde_json::json!({"allow": false, "deny": ["nope"]});
        let decision = decode_policy_value(value);
        assert!(!decision.allowed);
        assert_eq!(decision.reasons, vec!["nope".to_string()]);
    }
}
