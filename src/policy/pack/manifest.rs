//! Policy pack manifest definitions.
//!
//! A manifest describes a policy pack's metadata, the rules it contains,
//! and any configurable parameters.

use serde::{Deserialize, Serialize};

/// Metadata describing a policy pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyPackManifest {
    /// Unique name for the pack (e.g. "security-baseline").
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Category the pack belongs to.
    pub category: PackCategory,
    /// List of rule names included in this pack.
    pub rules: Vec<String>,
    /// Configurable parameters that influence rule behaviour.
    pub parameters: Vec<PackParameter>,
}

/// Category of a policy pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PackCategory {
    /// Security-oriented rules (e.g. deny dangerous modules).
    Security,
    /// Compliance-oriented rules (e.g. require tagging).
    Compliance,
    /// Operational best-practice rules.
    Operations,
    /// User-defined category.
    Custom(String),
}

impl std::fmt::Display for PackCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackCategory::Security => write!(f, "Security"),
            PackCategory::Compliance => write!(f, "Compliance"),
            PackCategory::Operations => write!(f, "Operations"),
            PackCategory::Custom(name) => write!(f, "Custom({})", name),
        }
    }
}

/// A configurable parameter for a policy pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackParameter {
    /// Parameter name.
    pub name: String,
    /// Human-readable description of the parameter.
    pub description: String,
    /// Type of the parameter (e.g. "integer", "string", "boolean").
    pub param_type: String,
    /// Optional default value (serialised as a string).
    pub default_value: Option<String>,
    /// Whether the parameter is required.
    pub required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip_yaml() {
        let manifest = PolicyPackManifest {
            name: "test-pack".into(),
            version: "1.0.0".into(),
            description: "A test policy pack".into(),
            category: PackCategory::Security,
            rules: vec!["no-shell".into(), "no-raw".into()],
            parameters: vec![PackParameter {
                name: "max_tasks".into(),
                description: "Maximum tasks per play".into(),
                param_type: "integer".into(),
                default_value: Some("20".into()),
                required: false,
            }],
        };

        let yaml = serde_yaml::to_string(&manifest).expect("serialize");
        let deserialized: PolicyPackManifest =
            serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(deserialized.name, "test-pack");
        assert_eq!(deserialized.version, "1.0.0");
        assert_eq!(deserialized.category, PackCategory::Security);
        assert_eq!(deserialized.rules.len(), 2);
        assert_eq!(deserialized.parameters.len(), 1);
        assert_eq!(deserialized.parameters[0].name, "max_tasks");
        assert!(!deserialized.parameters[0].required);
    }

    #[test]
    fn test_pack_category_display() {
        assert_eq!(PackCategory::Security.to_string(), "Security");
        assert_eq!(PackCategory::Compliance.to_string(), "Compliance");
        assert_eq!(PackCategory::Operations.to_string(), "Operations");
        assert_eq!(
            PackCategory::Custom("my-cat".into()).to_string(),
            "Custom(my-cat)"
        );
    }
}
