//! Fully Qualified Collection Name (FQCN) parsing and handling
//!
//! FQCNs are the standard way to reference collection content in Ansible 2.10+.
//! They follow the format: `namespace.collection.resource_name`
//!
//! # Examples
//!
//! - `ansible.builtin.copy` - The copy module from ansible.builtin
//! - `community.general.json_query` - The json_query filter from community.general
//! - `amazon.aws.ec2_instance` - The ec2_instance module from amazon.aws

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Error parsing an FQCN
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FqcnParseError {
    #[error("FQCN must have at least 3 parts (namespace.collection.resource): got '{0}'")]
    TooFewParts(String),

    #[error("Invalid namespace: '{0}' (must be lowercase alphanumeric with underscores)")]
    InvalidNamespace(String),

    #[error("Invalid collection name: '{0}' (must be lowercase alphanumeric with underscores)")]
    InvalidCollectionName(String),

    #[error("Invalid resource name: '{0}' (must be lowercase alphanumeric with underscores)")]
    InvalidResourceName(String),

    #[error("Empty FQCN")]
    Empty,
}

/// Represents a Fully Qualified Collection Name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fqcn {
    /// The namespace (e.g., "ansible", "community", "amazon")
    pub namespace: String,

    /// The collection name (e.g., "builtin", "general", "aws")
    pub collection: String,

    /// The resource name (e.g., "copy", "json_query", "ec2_instance")
    pub resource: String,

    /// Optional sub-resource for nested references
    pub sub_resource: Option<String>,
}

impl Fqcn {
    /// Creates a new FQCN from components
    pub fn new(
        namespace: impl Into<String>,
        collection: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            collection: collection.into(),
            resource: resource.into(),
            sub_resource: None,
        }
    }

    /// Creates a new FQCN with a sub-resource
    pub fn with_sub_resource(
        namespace: impl Into<String>,
        collection: impl Into<String>,
        resource: impl Into<String>,
        sub_resource: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            collection: collection.into(),
            resource: resource.into(),
            sub_resource: Some(sub_resource.into()),
        }
    }

    /// Parses an FQCN from a string
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// use rustible::collection::Fqcn;
    ///
    /// let fqcn = Fqcn::parse("ansible.builtin.copy")?;
    /// assert_eq!(fqcn.namespace, "ansible");
    /// assert_eq!(fqcn.collection, "builtin");
    /// assert_eq!(fqcn.resource, "copy");
    /// # Ok(())
    /// # }
    /// ```
    pub fn parse(s: &str) -> Result<Self, FqcnParseError> {
        s.parse()
    }

    /// Returns the collection part (namespace.collection)
    pub fn collection_fqn(&self) -> String {
        format!("{}.{}", self.namespace, self.collection)
    }

    /// Returns the full FQCN as a string
    pub fn full(&self) -> String {
        if let Some(ref sub) = self.sub_resource {
            format!(
                "{}.{}.{}.{}",
                self.namespace, self.collection, self.resource, sub
            )
        } else {
            format!("{}.{}.{}", self.namespace, self.collection, self.resource)
        }
    }

    /// Checks if this FQCN is from the ansible.builtin namespace
    pub fn is_builtin(&self) -> bool {
        self.namespace == "ansible" && self.collection == "builtin"
    }

    /// Checks if this FQCN is from the ansible.legacy namespace
    pub fn is_legacy(&self) -> bool {
        self.namespace == "ansible" && self.collection == "legacy"
    }

    /// Attempts to match a simple module name to a builtin FQCN
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// use rustible::collection::Fqcn;
    ///
    /// let fqcn = Fqcn::from_short_name("copy");
    /// assert_eq!(fqcn.full(), "ansible.builtin.copy");
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_short_name(name: &str) -> Self {
        Self::new("ansible", "builtin", name)
    }

    /// Validates that the FQCN components are valid identifiers
    pub fn validate(&self) -> Result<(), FqcnParseError> {
        if !is_valid_identifier(&self.namespace) {
            return Err(FqcnParseError::InvalidNamespace(self.namespace.clone()));
        }
        if !is_valid_identifier(&self.collection) {
            return Err(FqcnParseError::InvalidCollectionName(
                self.collection.clone(),
            ));
        }
        if !is_valid_identifier(&self.resource) {
            return Err(FqcnParseError::InvalidResourceName(self.resource.clone()));
        }
        if let Some(ref sub) = self.sub_resource {
            if !is_valid_identifier(sub) {
                return Err(FqcnParseError::InvalidResourceName(sub.clone()));
            }
        }
        Ok(())
    }
}

/// Checks if a string is a valid Python/Ansible identifier
fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let mut chars = s.chars();

    // First character must be letter or underscore
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }

    // Rest must be lowercase alphanumeric or underscore
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl FromStr for Fqcn {
    type Err = FqcnParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.is_empty() {
            return Err(FqcnParseError::Empty);
        }

        let parts: Vec<&str> = s.split('.').collect();

        if parts.len() < 3 {
            return Err(FqcnParseError::TooFewParts(s.to_string()));
        }

        let namespace = parts[0].to_string();
        let collection = parts[1].to_string();

        // Handle potential sub-resources (e.g., namespace.collection.module.sub)
        let (resource, sub_resource) = if parts.len() > 3 {
            (parts[2].to_string(), Some(parts[3..].join(".")))
        } else {
            (parts[2].to_string(), None)
        };

        let fqcn = Fqcn {
            namespace,
            collection,
            resource,
            sub_resource,
        };

        fqcn.validate()?;
        Ok(fqcn)
    }
}

impl fmt::Display for Fqcn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.full())
    }
}

/// Type of resource referenced by an FQCN
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// A module (plugins/modules/)
    Module,
    /// A role (roles/)
    Role,
    /// A playbook (playbooks/)
    Playbook,
    /// An action plugin (plugins/action/)
    Action,
    /// A lookup plugin (plugins/lookup/)
    Lookup,
    /// A filter plugin (plugins/filter/)
    Filter,
    /// A test plugin (plugins/test/)
    Test,
    /// A connection plugin (plugins/connection/)
    Connection,
    /// A callback plugin (plugins/callback/)
    Callback,
    /// An inventory plugin (plugins/inventory/)
    Inventory,
    /// A vars plugin (plugins/vars/)
    Vars,
    /// A strategy plugin (plugins/strategy/)
    Strategy,
    /// Unknown resource type
    Unknown,
}

impl ResourceType {
    /// Returns the plugin directory name for this resource type
    pub fn plugin_dir(&self) -> Option<&'static str> {
        match self {
            ResourceType::Module => Some("modules"),
            ResourceType::Action => Some("action"),
            ResourceType::Lookup => Some("lookup"),
            ResourceType::Filter => Some("filter"),
            ResourceType::Test => Some("test"),
            ResourceType::Connection => Some("connection"),
            ResourceType::Callback => Some("callback"),
            ResourceType::Inventory => Some("inventory"),
            ResourceType::Vars => Some("vars"),
            ResourceType::Strategy => Some("strategy"),
            ResourceType::Role => None,
            ResourceType::Playbook => None,
            ResourceType::Unknown => None,
        }
    }
}

/// Represents either a simple module name or an FQCN
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRef {
    /// A simple module name (e.g., "copy")
    Simple(String),
    /// A fully qualified name (e.g., "ansible.builtin.copy")
    Fqcn(Fqcn),
}

impl ModuleRef {
    /// Parses a module reference (either simple or FQCN)
    pub fn parse(s: &str) -> Self {
        if s.contains('.') {
            match Fqcn::parse(s) {
                Ok(fqcn) => ModuleRef::Fqcn(fqcn),
                Err(_) => ModuleRef::Simple(s.to_string()),
            }
        } else {
            ModuleRef::Simple(s.to_string())
        }
    }

    /// Converts to an FQCN, using the default collection if simple
    pub fn to_fqcn(&self, default_collection: Option<&str>) -> Option<Fqcn> {
        match self {
            ModuleRef::Fqcn(fqcn) => Some(fqcn.clone()),
            ModuleRef::Simple(name) => {
                if let Some(collection) = default_collection {
                    let parts: Vec<&str> = collection.split('.').collect();
                    if parts.len() == 2 {
                        return Some(Fqcn::new(parts[0], parts[1], name));
                    }
                }
                // Default to ansible.builtin
                Some(Fqcn::from_short_name(name))
            }
        }
    }

    /// Returns the simple name (resource part)
    pub fn name(&self) -> &str {
        match self {
            ModuleRef::Simple(name) => name,
            ModuleRef::Fqcn(fqcn) => &fqcn.resource,
        }
    }

    /// Checks if this is an FQCN
    pub fn is_fqcn(&self) -> bool {
        matches!(self, ModuleRef::Fqcn(_))
    }
}

impl fmt::Display for ModuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleRef::Simple(name) => write!(f, "{}", name),
            ModuleRef::Fqcn(fqcn) => write!(f, "{}", fqcn),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_fqcn() {
        let fqcn = Fqcn::parse("ansible.builtin.copy").unwrap();
        assert_eq!(fqcn.namespace, "ansible");
        assert_eq!(fqcn.collection, "builtin");
        assert_eq!(fqcn.resource, "copy");
        assert!(fqcn.sub_resource.is_none());
    }

    #[test]
    fn test_parse_community_fqcn() {
        let fqcn = Fqcn::parse("community.general.json_query").unwrap();
        assert_eq!(fqcn.namespace, "community");
        assert_eq!(fqcn.collection, "general");
        assert_eq!(fqcn.resource, "json_query");
    }

    #[test]
    fn test_parse_with_subresource() {
        let fqcn = Fqcn::parse("namespace.collection.resource.sub").unwrap();
        assert_eq!(fqcn.namespace, "namespace");
        assert_eq!(fqcn.collection, "collection");
        assert_eq!(fqcn.resource, "resource");
        assert_eq!(fqcn.sub_resource, Some("sub".to_string()));
    }

    #[test]
    fn test_parse_invalid_too_few_parts() {
        let result = Fqcn::parse("ansible.builtin");
        assert!(matches!(result, Err(FqcnParseError::TooFewParts(_))));
    }

    #[test]
    fn test_parse_empty() {
        let result = Fqcn::parse("");
        assert!(matches!(result, Err(FqcnParseError::Empty)));
    }

    #[test]
    fn test_parse_invalid_namespace() {
        let result = Fqcn::parse("Ansible.builtin.copy");
        assert!(matches!(result, Err(FqcnParseError::InvalidNamespace(_))));
    }

    #[test]
    fn test_collection_fqn() {
        let fqcn = Fqcn::new("community", "general", "json_query");
        assert_eq!(fqcn.collection_fqn(), "community.general");
    }

    #[test]
    fn test_is_builtin() {
        let builtin = Fqcn::new("ansible", "builtin", "copy");
        assert!(builtin.is_builtin());

        let community = Fqcn::new("community", "general", "json_query");
        assert!(!community.is_builtin());
    }

    #[test]
    fn test_from_short_name() {
        let fqcn = Fqcn::from_short_name("copy");
        assert_eq!(fqcn.full(), "ansible.builtin.copy");
    }

    #[test]
    fn test_module_ref_parse() {
        let simple = ModuleRef::parse("copy");
        assert!(matches!(simple, ModuleRef::Simple(_)));

        let fqcn = ModuleRef::parse("ansible.builtin.copy");
        assert!(matches!(fqcn, ModuleRef::Fqcn(_)));
    }

    #[test]
    fn test_module_ref_to_fqcn() {
        let simple = ModuleRef::Simple("copy".to_string());
        let fqcn = simple.to_fqcn(Some("community.general")).unwrap();
        assert_eq!(fqcn.full(), "community.general.copy");

        let fqcn_ref = ModuleRef::Fqcn(Fqcn::new("amazon", "aws", "ec2"));
        let fqcn = fqcn_ref.to_fqcn(Some("community.general")).unwrap();
        assert_eq!(fqcn.full(), "amazon.aws.ec2");
    }

    #[test]
    fn test_valid_identifier() {
        assert!(is_valid_identifier("copy"));
        assert!(is_valid_identifier("json_query"));
        assert!(is_valid_identifier("ec2_instance"));
        assert!(is_valid_identifier("_private"));

        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("123"));
        assert!(!is_valid_identifier("Uppercase"));
        assert!(!is_valid_identifier("with-hyphen"));
        assert!(!is_valid_identifier("with.dot"));
    }

    #[test]
    fn test_display() {
        let fqcn = Fqcn::new("ansible", "builtin", "copy");
        assert_eq!(format!("{}", fqcn), "ansible.builtin.copy");

        let with_sub = Fqcn::with_sub_resource("ns", "coll", "res", "sub");
        assert_eq!(format!("{}", with_sub), "ns.coll.res.sub");
    }
}
