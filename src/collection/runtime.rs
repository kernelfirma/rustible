//! Runtime configuration parsing (meta/runtime.yml)
//!
//! Handles parsing of the runtime.yml file that provides runtime configuration
//! including plugin routing, deprecations, and required Ansible versions.

use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

use super::CollectionResult;

/// Runtime configuration from meta/runtime.yml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Required Ansible version
    #[serde(default)]
    pub requires_ansible: Option<String>,

    /// Plugin routing redirects
    #[serde(default)]
    pub plugin_routing: PluginRouting,

    /// Action groups
    #[serde(default)]
    pub action_groups: HashMap<String, Vec<String>>,
}

impl RuntimeConfig {
    /// Load from a file
    pub fn from_file(path: impl AsRef<Path>) -> CollectionResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: RuntimeConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Parse from a string
    pub fn from_str(yaml: &str) -> CollectionResult<Self> {
        let config: RuntimeConfig = serde_yaml::from_str(yaml)?;
        Ok(config)
    }

    /// Check if any routing is defined
    pub fn has_routing(&self) -> bool {
        !self.plugin_routing.modules.is_empty()
            || !self.plugin_routing.actions.is_empty()
            || !self.plugin_routing.lookups.is_empty()
            || !self.plugin_routing.filters.is_empty()
    }

    /// Get module routing for a specific module
    pub fn get_module_routing(&self, name: &str) -> Option<&RoutingEntry> {
        self.plugin_routing.modules.get(name)
    }

    /// Get action routing for a specific action
    pub fn get_action_routing(&self, name: &str) -> Option<&RoutingEntry> {
        self.plugin_routing.actions.get(name)
    }

    /// Get lookup routing for a specific lookup
    pub fn get_lookup_routing(&self, name: &str) -> Option<&RoutingEntry> {
        self.plugin_routing.lookups.get(name)
    }

    /// Get filter routing for a specific filter
    pub fn get_filter_routing(&self, name: &str) -> Option<&RoutingEntry> {
        self.plugin_routing.filters.get(name)
    }
}

impl std::str::FromStr for RuntimeConfig {
    type Err = super::CollectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RuntimeConfig::from_str(s)
    }
}

/// Plugin routing configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginRouting {
    /// Module redirects
    #[serde(default)]
    pub modules: HashMap<String, RoutingEntry>,

    /// Action redirects
    #[serde(default, alias = "action")]
    pub actions: HashMap<String, RoutingEntry>,

    /// Lookup redirects
    #[serde(default, alias = "lookup")]
    pub lookups: HashMap<String, RoutingEntry>,

    /// Filter redirects
    #[serde(default, alias = "filter")]
    pub filters: HashMap<String, RoutingEntry>,

    /// Test redirects
    #[serde(default, alias = "test")]
    pub tests: HashMap<String, RoutingEntry>,

    /// Callback redirects
    #[serde(default, alias = "callback")]
    pub callbacks: HashMap<String, RoutingEntry>,

    /// Connection redirects
    #[serde(default, alias = "connection")]
    pub connections: HashMap<String, RoutingEntry>,

    /// Inventory redirects
    #[serde(default, alias = "inventory")]
    pub inventories: HashMap<String, RoutingEntry>,

    /// Vars redirects
    #[serde(default, alias = "vars")]
    pub vars_plugins: HashMap<String, RoutingEntry>,
}

/// A single routing entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingEntry {
    /// Redirect to another plugin
    #[serde(default)]
    pub redirect: Option<String>,

    /// Deprecation warning
    #[serde(default)]
    pub deprecation: Option<DeprecationInfo>,

    /// Tombstone (removed plugin)
    #[serde(default)]
    pub tombstone: Option<TombstoneInfo>,
}

impl RoutingEntry {
    /// Check if this is a redirect
    pub fn is_redirect(&self) -> bool {
        self.redirect.is_some()
    }

    /// Check if this is deprecated
    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }

    /// Check if this is a tombstone
    pub fn is_tombstone(&self) -> bool {
        self.tombstone.is_some()
    }

    /// Get the redirect target if any
    pub fn redirect_to(&self) -> Option<&str> {
        self.redirect.as_deref()
    }
}

/// Deprecation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecationInfo {
    /// Warning message
    #[serde(default)]
    pub warning_text: Option<String>,

    /// Version when it will be removed
    #[serde(default)]
    pub removal_version: Option<String>,

    /// Date when it will be removed
    #[serde(default)]
    pub removal_date: Option<String>,
}

/// Tombstone information (for removed plugins)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstoneInfo {
    /// Removal message
    #[serde(default)]
    pub note: Option<String>,

    /// Alternative to use
    #[serde(default)]
    pub alternatives: Option<Vec<String>>,

    /// Version when it was removed
    #[serde(default)]
    pub removed_in: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_runtime_yml() {
        let yaml = r#"
requires_ansible: ">=2.10"

plugin_routing:
  modules:
    old_module:
      redirect: new_module
      deprecation:
        warning_text: "old_module has been deprecated"
        removal_version: "3.0.0"
    removed_module:
      tombstone:
        note: "This module has been removed"
        alternatives:
          - new_module
          - other_module
        removed_in: "2.0.0"
"#;

        let config = RuntimeConfig::from_str(yaml).unwrap();
        assert_eq!(config.requires_ansible, Some(">=2.10".to_string()));
        assert!(config.has_routing());

        let old = config.get_module_routing("old_module").unwrap();
        assert!(old.is_redirect());
        assert!(old.is_deprecated());
        assert_eq!(old.redirect_to(), Some("new_module"));

        let removed = config.get_module_routing("removed_module").unwrap();
        assert!(removed.is_tombstone());
    }

    #[test]
    fn test_empty_runtime() {
        let config = RuntimeConfig::default();
        assert!(!config.has_routing());
        assert!(config.requires_ansible.is_none());
    }
}
