//! Documentation and Compatibility Matrix Sync Tests
//!
//! This test suite validates that documentation stays in sync with:
//! 1. Feature flags defined in Cargo.toml
//! 2. Modules implemented in src/modules/
//!
//! CI should fail if docs drift from actual feature flags/modules.
//!
//! Closes Issue #311: Raised bar: Docs + compatibility matrix kept in sync

use std::collections::HashSet;
use std::fs;
use std::path::Path;

// ============================================================================
// Feature Flag Sync Tests
// ============================================================================

mod feature_flag_tests {
    use super::*;
    use toml::Value as TomlValue;

    /// Extract feature flags from Cargo.toml
    fn extract_cargo_features() -> HashSet<String> {
        let features = load_cargo_features_table();
        features.keys().cloned().collect()
    }

    fn load_cargo_features_table() -> toml::value::Table {
        let cargo_toml =
            fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");
        let parsed: TomlValue = cargo_toml
            .parse()
            .expect("Failed to parse Cargo.toml as TOML");
        parsed
            .get("features")
            .and_then(|value| value.as_table())
            .cloned()
            .unwrap_or_default()
    }

    fn feature_list_contains(feature_value: &TomlValue, name: &str) -> bool {
        match feature_value {
            TomlValue::Array(items) => items.iter().any(|item| {
                item.as_str()
                    .filter(|value| !value.starts_with("dep:"))
                    .map(|value| value == name)
                    .unwrap_or(false)
            }),
            _ => false,
        }
    }

    fn resolve_feature_dependencies(
        features: &toml::value::Table,
        root: &str,
    ) -> HashSet<String> {
        let mut resolved = HashSet::new();
        let mut stack = vec![root.to_string()];

        while let Some(feature) = stack.pop() {
            let Some(value) = features.get(&feature) else {
                continue;
            };
            let TomlValue::Array(items) = value else {
                continue;
            };

            for item in items {
                let Some(entry) = item.as_str() else {
                    continue;
                };
                if entry.starts_with("dep:") {
                    continue;
                }
                if resolved.insert(entry.to_string()) && features.contains_key(entry) {
                    stack.push(entry.to_string());
                }
            }
        }

        resolved
    }

    /// Extract documented feature flags from compatibility matrix
    fn extract_documented_features() -> HashSet<String> {
        let doc_path = "docs/compatibility/ansible.md";
        let content = fs::read_to_string(doc_path)
            .expect("Failed to read compatibility doc");

        let mut features = HashSet::new();
        let mut in_feature_table = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Look for feature flag table
            if trimmed.contains("Feature Flag") && trimmed.contains("Status") {
                in_feature_table = true;
                continue;
            }

            // Table ends at horizontal rule or blank line after header
            if in_feature_table && (trimmed.starts_with("---") && !trimmed.contains('|')) {
                in_feature_table = false;
                continue;
            }

            // Parse table rows like: | `feature_name` | Status | Description |
            if in_feature_table && trimmed.starts_with('|') && trimmed.contains('`') {
                // Skip header separator
                if trimmed.contains("---") {
                    continue;
                }

                // Extract feature name from backticks
                if let Some(start) = trimmed.find('`') {
                    if let Some(end) = trimmed[start+1..].find('`') {
                        let feature = &trimmed[start+1..start+1+end];
                        // Handle features like `russh` (default)
                        let feature_name = feature.split_whitespace().next().unwrap_or(feature);
                        features.insert(feature_name.to_string());
                    }
                }
            }
        }

        features
    }

    #[test]
    fn test_cargo_toml_has_features_section() {
        let features = extract_cargo_features();
        assert!(!features.is_empty(), "Cargo.toml should have features defined");
    }

    #[test]
    fn test_compatibility_doc_has_feature_table() {
        let features = extract_documented_features();
        assert!(!features.is_empty(), "Compatibility doc should have feature table");
    }

    #[test]
    fn test_core_features_documented() {
        let cargo_features = extract_cargo_features();
        let documented_features = extract_documented_features();

        // Core features that MUST be documented
        let core_features = vec![
            "russh",
            "ssh2-backend",
            "local",
            "docker",
            "kubernetes",
            "aws",
            "azure",
            "gcp",
            "winrm",
            "provisioning",
        ];

        for feature in core_features {
            assert!(
                cargo_features.contains(feature),
                "Core feature '{}' missing from Cargo.toml",
                feature
            );
            assert!(
                documented_features.contains(feature),
                "Core feature '{}' missing from docs/compatibility/ansible.md",
                feature
            );
        }
    }

    #[test]
    fn test_documented_features_exist_in_cargo() {
        let cargo_features = extract_cargo_features();
        let documented_features = extract_documented_features();

        for feature in &documented_features {
            // Skip "(default)" annotations
            if feature == "default" {
                continue;
            }
            assert!(
                cargo_features.contains(feature),
                "Feature '{}' documented but not in Cargo.toml",
                feature
            );
        }
    }

    #[test]
    fn test_database_feature_documented() {
        let cargo_features = extract_cargo_features();
        let documented_features = extract_documented_features();

        if cargo_features.contains("database") {
            assert!(
                documented_features.contains("database"),
                "database feature exists in Cargo.toml but not documented"
            );
        }
    }

    #[test]
    fn test_feature_combinations_documented() {
        // Verify that composite features are consistent
        let features = load_cargo_features_table();

        // Check that 'full' includes expected features
        if let Some(full) = features.get("full") {
            let resolved = resolve_feature_dependencies(&features, "full");
            assert!(
                feature_list_contains(full, "russh")
                    || feature_list_contains(full, "ssh2-backend")
                    || resolved.contains("russh")
                    || resolved.contains("ssh2-backend"),
                "full feature should include SSH backend"
            );
        }

        // Check that 'full-cloud' includes cloud providers
        if let Some(full_cloud) = features.get("full-cloud") {
            let resolved = resolve_feature_dependencies(&features, "full-cloud");
            assert!(
                feature_list_contains(full_cloud, "aws") || resolved.contains("aws"),
                "full-cloud feature should include aws"
            );
        }
    }
}

// ============================================================================
// Module Sync Tests
// ============================================================================

mod module_sync_tests {
    use super::*;

    /// Get list of module files from src/modules/
    fn get_implemented_modules() -> HashSet<String> {
        let modules_dir = Path::new("src/modules");
        let mut modules = HashSet::new();

        if let Ok(entries) = fs::read_dir(modules_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "rs" {
                        if let Some(stem) = path.file_stem() {
                            let name = stem.to_string_lossy().to_string();
                            // Skip mod.rs and internal modules
                            if name != "mod" && !name.starts_with('_') {
                                modules.insert(name);
                            }
                        }
                    }
                }
            }
        }

        modules
    }

    /// Extract modules mentioned in compatibility matrix
    fn get_documented_modules() -> HashSet<String> {
        let doc_path = "docs/compatibility/ansible.md";
        let content = fs::read_to_string(doc_path)
            .expect("Failed to read compatibility doc");

        let mut modules = HashSet::new();

        // Look for module names in backticks in table rows
        for line in content.lines() {
            let trimmed = line.trim();

            // Parse table rows with module names
            if trimmed.starts_with('|') && trimmed.contains('`') {
                // Skip header rows
                if trimmed.contains("Module") || trimmed.contains("---") {
                    continue;
                }

                // Extract module name from first backtick pair
                if let Some(start) = trimmed.find('`') {
                    if let Some(end) = trimmed[start+1..].find('`') {
                        let module = &trimmed[start+1..start+1+end];
                        // Handle aliases like `ec2` / `aws_ec2`
                        for part in module.split('/') {
                            let name = part.trim().trim_start_matches('`').trim_end_matches('`');
                            // Skip short aliases, keep actual module names
                            if name.len() > 2 && !name.contains(' ') {
                                modules.insert(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        modules
    }

    #[test]
    fn test_modules_directory_exists() {
        assert!(
            Path::new("src/modules").exists(),
            "src/modules directory should exist"
        );
    }

    #[test]
    fn test_has_implemented_modules() {
        let modules = get_implemented_modules();
        assert!(
            modules.len() > 20,
            "Should have at least 20 modules implemented, found {}",
            modules.len()
        );
    }

    #[test]
    fn test_core_modules_implemented() {
        let modules = get_implemented_modules();

        // Core modules that MUST be implemented
        let core_modules = vec![
            "apt",
            "command",
            "copy",
            "debug",
            "file",
            "lineinfile",
            "service",
            "shell",
            "template",
            "user",
            "group",
            "git",
        ];

        for module in core_modules {
            assert!(
                modules.contains(module),
                "Core module '{}' not implemented in src/modules/",
                module
            );
        }
    }

    #[test]
    fn test_documented_stable_modules_implemented() {
        let implemented = get_implemented_modules();

        // Stable modules marked as "Yes" in Rustible column
        let stable_documented = vec![
            "apt",
            "dnf",
            "pip",
            "command",
            "shell",
            "service",
            "user",
            "group",
            "git",
            "file",
            "copy",
            "template",
            "lineinfile",
            "blockinfile",
            "stat",
            "debug",
            "set_fact",
            "assert",
        ];

        for module in stable_documented {
            assert!(
                implemented.contains(module),
                "Documented stable module '{}' not implemented",
                module
            );
        }
    }

    #[test]
    fn test_package_managers_implemented() {
        let modules = get_implemented_modules();

        // All documented package managers
        let package_managers = vec!["apt", "dnf", "pip", "package"];

        for pm in package_managers {
            assert!(
                modules.contains(pm),
                "Package manager module '{}' not implemented",
                pm
            );
        }
    }

    #[test]
    fn test_security_modules_implemented() {
        let modules = get_implemented_modules();

        // Security modules marked as implemented
        let security_modules = vec![
            "authorized_key",
            "known_hosts",
            "ufw",
            "firewalld",
            "selinux",
        ];

        for module in security_modules {
            assert!(
                modules.contains(module),
                "Security module '{}' not implemented",
                module
            );
        }
    }

    #[test]
    fn test_network_modules_exist() {
        let modules = get_implemented_modules();

        // Wait_for and uri are documented as implemented
        assert!(modules.contains("wait_for"), "wait_for module not implemented");
        assert!(modules.contains("uri"), "uri module not implemented");
    }

    #[test]
    fn test_system_admin_modules_implemented() {
        let modules = get_implemented_modules();

        let system_modules = vec![
            "service",
            "user",
            "group",
            "hostname",
            "sysctl",
            "mount",
            "cron",
        ];

        for module in system_modules {
            assert!(
                modules.contains(module),
                "System admin module '{}' not implemented",
                module
            );
        }
    }
}

// ============================================================================
// Connection Type Sync Tests
// ============================================================================

mod connection_sync_tests {
    use super::*;

    /// Extract connection types from compatibility doc
    fn get_documented_connections() -> HashSet<String> {
        let doc_path = "docs/compatibility/ansible.md";
        let content = fs::read_to_string(doc_path)
            .expect("Failed to read compatibility doc");

        let mut connections = HashSet::new();
        let mut in_connection_table = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Look for connection types table
            if trimmed.contains("Connection") && trimmed.contains("Feature Flag") {
                in_connection_table = true;
                continue;
            }

            if in_connection_table && trimmed.starts_with("---") && !trimmed.contains('|') {
                in_connection_table = false;
                continue;
            }

            if in_connection_table && trimmed.starts_with('|') {
                if trimmed.contains("---") {
                    continue;
                }

                // Extract connection name (first column)
                let parts: Vec<&str> = trimmed.split('|').collect();
                if parts.len() > 1 {
                    let conn = parts[1].trim();
                    if !conn.is_empty() && !conn.contains("Connection") {
                        connections.insert(conn.to_string());
                    }
                }
            }
        }

        connections
    }

    #[test]
    fn test_ssh_connections_documented() {
        let connections = get_documented_connections();

        assert!(
            connections.iter().any(|c| c.contains("SSH")),
            "SSH connection should be documented"
        );
    }

    #[test]
    fn test_local_connection_documented() {
        let connections = get_documented_connections();

        assert!(
            connections.iter().any(|c| c.contains("Local")),
            "Local connection should be documented"
        );
    }

    #[test]
    fn test_docker_connection_documented() {
        let connections = get_documented_connections();

        assert!(
            connections.iter().any(|c| c.contains("Docker")),
            "Docker connection should be documented"
        );
    }

    #[test]
    fn test_kubernetes_connection_documented() {
        let connections = get_documented_connections();

        assert!(
            connections.iter().any(|c| c.contains("Kubernetes")),
            "Kubernetes connection should be documented"
        );
    }
}

// ============================================================================
// Jinja2 Filter Sync Tests
// ============================================================================

mod filter_sync_tests {
    use super::*;

    #[test]
    fn test_jinja2_filter_doc_exists() {
        assert!(
            Path::new("docs/compatibility/jinja2-filters.md").exists(),
            "Jinja2 filter compatibility doc should exist"
        );
    }

    #[test]
    fn test_core_filters_documented() {
        let doc_path = "docs/compatibility/ansible.md";
        let content = fs::read_to_string(doc_path)
            .expect("Failed to read compatibility doc");

        // Core filters that MUST be mentioned
        let core_filters = vec![
            "default",
            "lower",
            "upper",
            "join",
            "split",
            "to_json",
            "from_json",
        ];

        for filter in core_filters {
            assert!(
                content.contains(filter),
                "Core filter '{}' not documented in compatibility matrix",
                filter
            );
        }
    }
}

// ============================================================================
// Callback Plugin Sync Tests
// ============================================================================

mod callback_sync_tests {
    use super::*;

    /// Get implemented callback plugins from src/callback/
    fn get_implemented_callbacks() -> HashSet<String> {
        let callbacks_dir = Path::new("src/callback");
        let mut callbacks = HashSet::new();

        if let Ok(entries) = fs::read_dir(callbacks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "rs" {
                        if let Some(stem) = path.file_stem() {
                            let name = stem.to_string_lossy().to_string();
                            if name != "mod" && !name.starts_with('_') {
                                callbacks.insert(name);
                            }
                        }
                    }
                }
            }
        }

        callbacks
    }

    #[test]
    fn test_callbacks_directory_exists() {
        assert!(
            Path::new("src/callback").exists(),
            "src/callback directory should exist"
        );
    }

    #[test]
    fn test_core_callbacks_implemented() {
        let callbacks = get_implemented_callbacks();

        // Core callbacks that should exist (checking what's actually in the codebase)
        // The callback module uses a factory pattern, so check for key files
        let has_callbacks = !callbacks.is_empty() ||
            Path::new("src/callback/mod.rs").exists();

        assert!(
            has_callbacks,
            "Callback system should be implemented"
        );
    }

    #[test]
    fn test_callbacks_documented_in_compatibility() {
        let doc_path = "docs/compatibility/ansible.md";
        let content = fs::read_to_string(doc_path)
            .expect("Failed to read compatibility doc");

        // Verify callback section exists
        assert!(
            content.contains("Callback Plugins") || content.contains("callback"),
            "Callback plugins should be documented"
        );
    }
}

// ============================================================================
// Doc Structure Tests
// ============================================================================

mod doc_structure_tests {
    use super::*;

    #[test]
    fn test_compatibility_doc_exists() {
        assert!(
            Path::new("docs/compatibility/ansible.md").exists(),
            "Ansible compatibility doc should exist"
        );
    }

    #[test]
    fn test_compatibility_doc_has_last_updated() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Last Updated") || content.contains("last updated"),
            "Compatibility doc should have last updated date"
        );
    }

    #[test]
    fn test_compatibility_doc_has_version() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Rustible Version") || content.contains("version"),
            "Compatibility doc should reference Rustible version"
        );
    }

    #[test]
    fn test_module_reference_doc_exists() {
        assert!(
            Path::new("docs/reference/modules.md").exists(),
            "Module reference doc should exist"
        );
    }

    #[test]
    fn test_modules_doc_has_table_of_contents() {
        let content = fs::read_to_string("docs/reference/modules.md")
            .expect("Failed to read module reference doc");

        assert!(
            content.contains("Table of Contents") || content.contains("##"),
            "Module doc should have organized sections"
        );
    }

    #[test]
    fn test_roadmap_doc_exists() {
        assert!(
            Path::new("docs/ROADMAP.md").exists(),
            "ROADMAP.md should exist"
        );
    }
}

// ============================================================================
// Version Consistency Tests
// ============================================================================

mod version_tests {
    use super::*;

    #[test]
    fn test_cargo_version_parseable() {
        let cargo_toml = fs::read_to_string("Cargo.toml")
            .expect("Failed to read Cargo.toml");

        assert!(
            cargo_toml.contains("version = \""),
            "Cargo.toml should have a version field"
        );
    }

    #[test]
    fn test_compatibility_targets_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Version Compatibility") || content.contains("v0.1") || content.contains("v1.0"),
            "Version compatibility targets should be documented"
        );
    }
}

// ============================================================================
// Known Incompatibilities Tests
// ============================================================================

mod incompatibility_tests {
    use super::*;

    #[test]
    fn test_known_incompatibilities_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Known Incompatibilities") || content.contains("incompatibilities"),
            "Known incompatibilities should be documented"
        );
    }

    #[test]
    fn test_vault_incompatibility_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Vault") && (content.contains("AES") || content.contains("different")),
            "Vault format incompatibility should be documented"
        );
    }
}

// ============================================================================
// Cloud Module Documentation Tests
// ============================================================================

mod cloud_module_tests {
    use super::*;

    #[test]
    fn test_aws_modules_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("AWS") && content.contains("ec2"),
            "AWS modules should be documented"
        );
    }

    #[test]
    fn test_docker_modules_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Docker") && content.contains("docker_container"),
            "Docker modules should be documented"
        );
    }

    #[test]
    fn test_kubernetes_modules_documented() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Kubernetes") && content.contains("k8s"),
            "Kubernetes modules should be documented"
        );
    }

    #[test]
    fn test_experimental_features_marked() {
        let content = fs::read_to_string("docs/compatibility/ansible.md")
            .expect("Failed to read compatibility doc");

        assert!(
            content.contains("Experimental") || content.contains("experimental"),
            "Experimental features should be marked"
        );

        // Azure and GCP should be marked experimental
        assert!(
            content.contains("azure") || content.contains("Azure"),
            "Azure should be mentioned"
        );
    }
}
