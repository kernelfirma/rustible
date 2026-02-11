//! RBAC authorization engine
//!
//! Evaluates authorization requests against loaded roles, resolving
//! inheritance and enforcing deny-takes-precedence semantics.

use super::model::{AuthzDecision, AuthzRequest, Effect, Role};
use std::collections::{HashMap, HashSet};

/// The core RBAC authorization engine.
#[derive(Debug, Clone)]
pub struct RbacEngine {
    /// All known roles indexed by name.
    roles: HashMap<String, Role>,
}

impl RbacEngine {
    /// Create an empty engine.
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
        }
    }

    /// Bulk-load a set of roles, replacing any existing ones with the same name.
    pub fn load_roles(&mut self, roles: Vec<Role>) {
        for role in roles {
            self.roles.insert(role.name.clone(), role);
        }
    }

    /// Add a single role.
    pub fn add_role(&mut self, role: Role) {
        self.roles.insert(role.name.clone(), role);
    }

    /// Resolve a list of role names into their `Role` references, following
    /// inheritance chains. Cycles are detected and avoided.
    pub fn resolve_roles<'a>(&'a self, role_names: &[String]) -> Vec<&'a Role> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut stack: Vec<&str> = role_names.iter().map(|s| s.as_str()).collect();

        while let Some(name) = stack.pop() {
            if !visited.insert(name.to_string()) {
                continue;
            }
            if let Some(role) = self.roles.get(name) {
                result.push(role);
                for parent in &role.inherits {
                    stack.push(parent.as_str());
                }
            }
        }

        result
    }

    /// Evaluate an authorization request and return a decision.
    ///
    /// Algorithm:
    /// 1. Resolve all roles (including inherited ones).
    /// 2. If any resolved permission explicitly *denies* the requested
    ///    resource+action, the request is denied (deny takes precedence).
    /// 3. If at least one permission *allows* the requested resource+action
    ///    and no deny was found, the request is allowed.
    /// 4. Otherwise, the request is denied by default (implicit deny).
    pub fn authorize(&self, request: &AuthzRequest) -> AuthzDecision {
        let resolved = self.resolve_roles(&request.roles);

        if resolved.is_empty() {
            return AuthzDecision {
                allowed: false,
                reason: format!("no roles found for principal '{}'", request.principal),
                matched_role: None,
            };
        }

        // First pass: check for explicit deny.
        for role in &resolved {
            for perm in &role.permissions {
                if perm.effect == Effect::Deny
                    && perm.resource.matches(&request.resource)
                    && perm.actions.contains(&request.action)
                {
                    return AuthzDecision {
                        allowed: false,
                        reason: format!(
                            "denied by role '{}' on resource '{}' for action '{}'",
                            role.name, request.resource, request.action
                        ),
                        matched_role: Some(role.name.clone()),
                    };
                }
            }
        }

        // Second pass: check for explicit allow.
        for role in &resolved {
            for perm in &role.permissions {
                if perm.effect == Effect::Allow
                    && perm.resource.matches(&request.resource)
                    && perm.actions.contains(&request.action)
                {
                    return AuthzDecision {
                        allowed: true,
                        reason: format!(
                            "allowed by role '{}' on resource '{}' for action '{}'",
                            role.name, request.resource, request.action
                        ),
                        matched_role: Some(role.name.clone()),
                    };
                }
            }
        }

        // Implicit deny.
        AuthzDecision {
            allowed: false,
            reason: format!(
                "no matching permission for action '{}' on resource '{}'",
                request.action, request.resource
            ),
            matched_role: None,
        }
    }
}

impl Default for RbacEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::rbac::model::{Action, Effect, Permission, ResourcePattern};

    fn make_role(name: &str, perms: Vec<Permission>, inherits: Vec<String>) -> Role {
        Role {
            name: name.to_string(),
            permissions: perms,
            inherits,
            description: String::new(),
        }
    }

    fn allow_perm(pattern: &str, actions: Vec<Action>) -> Permission {
        Permission {
            resource: ResourcePattern::new(pattern),
            actions,
            effect: Effect::Allow,
        }
    }

    fn deny_perm(pattern: &str, actions: Vec<Action>) -> Permission {
        Permission {
            resource: ResourcePattern::new(pattern),
            actions,
            effect: Effect::Deny,
        }
    }

    #[test]
    fn test_basic_allow() {
        let mut engine = RbacEngine::new();
        engine.add_role(make_role(
            "viewer",
            vec![allow_perm("*", vec![Action::Read])],
            vec![],
        ));

        let decision = engine.authorize(&AuthzRequest {
            principal: "alice".into(),
            roles: vec!["viewer".into()],
            resource: "hosts:web01".into(),
            action: Action::Read,
        });

        assert!(decision.allowed);
        assert_eq!(decision.matched_role.as_deref(), Some("viewer"));
    }

    #[test]
    fn test_deny_takes_precedence() {
        let mut engine = RbacEngine::new();
        engine.load_roles(vec![
            make_role(
                "writer",
                vec![allow_perm("*", vec![Action::Read, Action::Write])],
                vec![],
            ),
            make_role(
                "restricted",
                vec![deny_perm("secrets:*", vec![Action::Write])],
                vec![],
            ),
        ]);

        let decision = engine.authorize(&AuthzRequest {
            principal: "bob".into(),
            roles: vec!["writer".into(), "restricted".into()],
            resource: "secrets:vault".into(),
            action: Action::Write,
        });

        assert!(!decision.allowed);
        assert!(decision.reason.contains("denied"));
    }

    #[test]
    fn test_role_inheritance() {
        let mut engine = RbacEngine::new();
        engine.load_roles(vec![
            make_role(
                "base",
                vec![allow_perm("*", vec![Action::Read])],
                vec![],
            ),
            make_role(
                "operator",
                vec![allow_perm("hosts:*", vec![Action::Execute])],
                vec!["base".into()],
            ),
        ]);

        // Operator should inherit read from base.
        let decision = engine.authorize(&AuthzRequest {
            principal: "carol".into(),
            roles: vec!["operator".into()],
            resource: "config:main".into(),
            action: Action::Read,
        });

        assert!(decision.allowed);
        assert_eq!(decision.matched_role.as_deref(), Some("base"));
    }

    #[test]
    fn test_implicit_deny_for_unknown_role() {
        let engine = RbacEngine::new();

        let decision = engine.authorize(&AuthzRequest {
            principal: "eve".into(),
            roles: vec!["nonexistent".into()],
            resource: "anything".into(),
            action: Action::Read,
        });

        assert!(!decision.allowed);
        assert!(decision.reason.contains("no roles found"));
    }
}
