//! RBAC configuration storage and built-in role definitions
//!
//! Provides serialization-friendly configuration and a set of
//! sensible default roles for common enterprise use cases.

use super::model::{Action, Effect, Permission, ResourcePattern, Role};
use serde::{Deserialize, Serialize};

/// Top-level RBAC configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacConfig {
    /// All configured roles.
    pub roles: Vec<Role>,
}

impl RbacConfig {
    /// Create a configuration pre-populated with built-in roles.
    ///
    /// Built-in roles:
    /// - **admin**: full access to all resources and actions.
    /// - **operator**: read, write, and execute on all resources.
    /// - **viewer**: read-only access to all resources.
    /// - **auditor**: read-only access to audit-related resources.
    pub fn with_builtins() -> Self {
        let admin = Role {
            name: "admin".into(),
            permissions: vec![Permission {
                resource: ResourcePattern::new("*"),
                actions: vec![Action::Read, Action::Write, Action::Execute, Action::Admin],
                effect: Effect::Allow,
            }],
            inherits: vec![],
            description: "Full administrative access to all resources".into(),
        };

        let operator = Role {
            name: "operator".into(),
            permissions: vec![Permission {
                resource: ResourcePattern::new("*"),
                actions: vec![Action::Read, Action::Write, Action::Execute],
                effect: Effect::Allow,
            }],
            inherits: vec![],
            description: "Operational access: read, write, and execute".into(),
        };

        let viewer = Role {
            name: "viewer".into(),
            permissions: vec![Permission {
                resource: ResourcePattern::new("*"),
                actions: vec![Action::Read],
                effect: Effect::Allow,
            }],
            inherits: vec![],
            description: "Read-only access to all resources".into(),
        };

        let auditor = Role {
            name: "auditor".into(),
            permissions: vec![Permission {
                resource: ResourcePattern::new("audit:*"),
                actions: vec![Action::Read],
                effect: Effect::Allow,
            }],
            inherits: vec![],
            description: "Read-only access to audit resources".into(),
        };

        Self {
            roles: vec![admin, operator, viewer, auditor],
        }
    }

    /// Parse an `RbacConfig` from a YAML string.
    pub fn from_yaml(content: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(content)
    }
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_contain_expected_roles() {
        let config = RbacConfig::with_builtins();
        let names: Vec<&str> = config.roles.iter().map(|r| r.name.as_str()).collect();

        assert!(names.contains(&"admin"));
        assert!(names.contains(&"operator"));
        assert!(names.contains(&"viewer"));
        assert!(names.contains(&"auditor"));
        assert_eq!(config.roles.len(), 4);
    }

    #[test]
    fn test_from_yaml_roundtrip() {
        let config = RbacConfig::with_builtins();
        let yaml = serde_yaml::to_string(&config).expect("serialize");
        let parsed = RbacConfig::from_yaml(&yaml).expect("deserialize");

        assert_eq!(parsed.roles.len(), config.roles.len());
        assert_eq!(parsed.roles[0].name, "admin");
    }

    #[test]
    fn test_from_yaml_custom_roles() {
        let yaml = r#"
roles:
  - name: deployer
    description: Can deploy playbooks
    permissions:
      - resource:
          pattern: "playbooks:*"
        actions:
          - read
          - execute
        effect: allow
    inherits: []
"#;
        let config = RbacConfig::from_yaml(yaml).expect("parse custom yaml");
        assert_eq!(config.roles.len(), 1);
        assert_eq!(config.roles[0].name, "deployer");
        assert_eq!(config.roles[0].permissions[0].actions.len(), 2);
    }
}
