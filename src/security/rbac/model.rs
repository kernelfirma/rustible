//! RBAC data model types
//!
//! Defines roles, permissions, resource patterns, and authorization
//! request/decision structures.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A named role containing a set of permissions and optional inheritance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique role name (e.g. "admin", "operator").
    pub name: String,
    /// Permissions granted (or denied) by this role.
    pub permissions: Vec<Permission>,
    /// Names of parent roles whose permissions are inherited.
    #[serde(default)]
    pub inherits: Vec<String>,
    /// Human-readable description of the role.
    #[serde(default)]
    pub description: String,
}

/// A single permission entry that maps a resource pattern + actions to an effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    /// The resource pattern this permission applies to.
    pub resource: ResourcePattern,
    /// The actions covered by this permission.
    pub actions: Vec<Action>,
    /// Whether to allow or deny.
    pub effect: Effect,
}

/// A glob-like pattern for matching resource identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePattern {
    /// The pattern string. Supports `*` as a wildcard prefix/suffix.
    pub pattern: String,
}

impl ResourcePattern {
    /// Create a new resource pattern.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
        }
    }

    /// Check whether the given resource string matches this pattern.
    ///
    /// Matching rules:
    /// - `"*"` matches everything.
    /// - A pattern ending with `*` matches any resource that starts with the
    ///   prefix (e.g. `"hosts:*"` matches `"hosts:web01"`).
    /// - A pattern starting with `*` matches any resource that ends with the
    ///   suffix (e.g. `"*:read"` matches `"config:read"`).
    /// - Otherwise, exact match is required.
    pub fn matches(&self, resource: &str) -> bool {
        if self.pattern == "*" {
            return true;
        }
        if self.pattern.ends_with('*') {
            let prefix = &self.pattern[..self.pattern.len() - 1];
            return resource.starts_with(prefix);
        }
        if self.pattern.starts_with('*') {
            let suffix = &self.pattern[1..];
            return resource.ends_with(suffix);
        }
        self.pattern == resource
    }
}

/// An action that can be performed on a resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Read / view access.
    Read,
    /// Write / modify access.
    Write,
    /// Execute / run access.
    Execute,
    /// Full administrative access.
    Admin,
    /// A custom action defined by the user.
    Custom(String),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Read => write!(f, "read"),
            Action::Write => write!(f, "write"),
            Action::Execute => write!(f, "execute"),
            Action::Admin => write!(f, "admin"),
            Action::Custom(s) => write!(f, "{}", s),
        }
    }
}

impl Action {
    /// Parse an action from a string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "read" => Action::Read,
            "write" => Action::Write,
            "execute" => Action::Execute,
            "admin" => Action::Admin,
            other => Action::Custom(other.to_string()),
        }
    }
}

/// Whether a permission grants or denies access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Grant access.
    Allow,
    /// Deny access (takes precedence over Allow).
    Deny,
}

/// An authorization request submitted to the RBAC engine.
#[derive(Debug, Clone)]
pub struct AuthzRequest {
    /// The principal (user or service) requesting access.
    pub principal: String,
    /// Roles assigned to the principal.
    pub roles: Vec<String>,
    /// The target resource identifier.
    pub resource: String,
    /// The action being requested.
    pub action: Action,
}

/// The decision returned by the RBAC engine.
#[derive(Debug, Clone)]
pub struct AuthzDecision {
    /// Whether access is allowed.
    pub allowed: bool,
    /// Human-readable explanation.
    pub reason: String,
    /// The role that matched (if any).
    pub matched_role: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_pattern_matching() {
        let wildcard = ResourcePattern::new("*");
        assert!(wildcard.matches("anything"));
        assert!(wildcard.matches(""));

        let prefix = ResourcePattern::new("hosts:*");
        assert!(prefix.matches("hosts:web01"));
        assert!(prefix.matches("hosts:"));
        assert!(!prefix.matches("config:web01"));

        let suffix = ResourcePattern::new("*:read");
        assert!(suffix.matches("config:read"));
        assert!(!suffix.matches("config:write"));

        let exact = ResourcePattern::new("playbooks:deploy");
        assert!(exact.matches("playbooks:deploy"));
        assert!(!exact.matches("playbooks:deploy2"));
    }

    #[test]
    fn test_action_display_and_parse() {
        assert_eq!(Action::Read.to_string(), "read");
        assert_eq!(Action::Write.to_string(), "write");
        assert_eq!(Action::Execute.to_string(), "execute");
        assert_eq!(Action::Admin.to_string(), "admin");
        assert_eq!(Action::Custom("deploy".into()).to_string(), "deploy");

        assert_eq!(Action::from_str("READ"), Action::Read);
        assert_eq!(Action::from_str("Write"), Action::Write);
        assert_eq!(Action::from_str("deploy"), Action::Custom("deploy".into()));
    }
}
