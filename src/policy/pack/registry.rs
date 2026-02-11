//! Policy pack registry.
//!
//! The registry discovers built-in packs, loads them, and provides a
//! single entry-point to evaluate all registered packs against playbook
//! data.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::builtins;
use super::loader::{PackLoader, PolicyPack};
use super::manifest::PolicyPackManifest;

/// Result of evaluating a single policy pack against playbook data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackEvaluationResult {
    /// Name of the evaluated pack.
    pub pack_name: String,
    /// Number of rules that passed.
    pub passed: usize,
    /// Number of rules that failed (severity = Error).
    pub failed: usize,
    /// Number of rules that produced warnings.
    pub warnings: usize,
    /// Detailed violation messages.
    pub details: Vec<String>,
}

/// Registry that holds discovered and loaded policy packs.
pub struct PackRegistry {
    packs: Vec<PolicyPack>,
}

impl PackRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self { packs: Vec::new() }
    }

    /// Discover and register all built-in packs.
    pub fn discover(&mut self) {
        let builtin_manifests: Vec<PolicyPackManifest> = vec![
            builtins::security::manifest(),
            builtins::compliance::manifest(),
            builtins::operations::manifest(),
        ];

        for manifest in builtin_manifests {
            let pack = PackLoader::load_from_parsed(manifest);
            self.packs.push(pack);
        }
    }

    /// Load a pack from a YAML manifest string and add it to the registry.
    pub fn load(&mut self, manifest_yaml: &str) -> Result<(), String> {
        let pack = PackLoader::load_from_manifest(manifest_yaml)?;
        self.packs.push(pack);
        Ok(())
    }

    /// Return a list of all registered pack manifests.
    pub fn list(&self) -> Vec<&PolicyPackManifest> {
        self.packs.iter().map(|p| &p.manifest).collect()
    }

    /// Find a pack by name.
    pub fn get(&self, name: &str) -> Option<&PolicyPack> {
        self.packs.iter().find(|p| p.manifest.name == name)
    }

    /// Evaluate all registered packs against the given playbook data.
    pub fn evaluate_all(&self, playbook_data: &Value) -> Vec<PackEvaluationResult> {
        self.packs
            .iter()
            .map(|pack| Self::evaluate_pack(pack, playbook_data))
            .collect()
    }

    fn evaluate_pack(pack: &PolicyPack, playbook_data: &Value) -> PackEvaluationResult {
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut warnings = 0usize;
        let mut details = Vec::new();

        for rule in &pack.rules {
            let violations = rule.evaluate(playbook_data);
            if violations.is_empty() {
                passed += 1;
            } else {
                let severity_label = match rule.severity {
                    crate::policy::RuleSeverity::Error => {
                        failed += 1;
                        "ERROR"
                    }
                    crate::policy::RuleSeverity::Warning => {
                        warnings += 1;
                        "WARN"
                    }
                    crate::policy::RuleSeverity::Info => {
                        // Info-level violations don't count as failures or warnings.
                        passed += 1;
                        "INFO"
                    }
                };
                for v in violations {
                    details.push(format!("[{}] {}: {}", severity_label, rule.name, v));
                }
            }
        }

        PackEvaluationResult {
            pack_name: pack.manifest.name.clone(),
            passed,
            failed,
            warnings,
            details,
        }
    }
}

impl Default for PackRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_discover_loads_builtins() {
        let mut registry = PackRegistry::new();
        registry.discover();

        let packs = registry.list();
        assert!(packs.len() >= 3, "should have at least 3 built-in packs");

        let names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"security-baseline"));
        assert!(names.contains(&"compliance-baseline"));
        assert!(names.contains(&"operations-baseline"));
    }

    #[test]
    fn test_evaluate_all_detects_violations() {
        let mut registry = PackRegistry::new();
        registry.discover();

        let input = json!([{
            "name": "Test play",
            "hosts": "all",
            "tasks": [
                {"name": "Run shell", "shell": "echo bad"},
                {"name": "Good task", "debug": {"msg": "ok"}}
            ]
        }]);

        let results = registry.evaluate_all(&input);
        assert!(!results.is_empty());

        // The security pack should flag the shell usage.
        let security = results.iter().find(|r| r.pack_name == "security-baseline");
        assert!(security.is_some());
        let sec = security.unwrap();
        assert!(sec.failed > 0 || !sec.details.is_empty());
    }

    #[test]
    fn test_load_custom_pack() {
        let mut registry = PackRegistry::new();
        let yaml = r#"
name: my-custom
version: "0.1.0"
description: Custom pack
category: !Custom "testing"
rules:
  - require-name
parameters: []
"#;
        registry.load(yaml).expect("load should succeed");
        let packs = registry.list();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "my-custom");
    }
}
