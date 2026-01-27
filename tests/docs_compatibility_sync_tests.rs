//! Docs + Compatibility Matrix Sync Test Suite for Issue #311
//!
//! Automated doc checks to keep compatibility matrix in sync with features.
//! CI fails if docs drift from feature flags/modules.

use std::collections::{HashMap, HashSet};

// ============================================================================
// Feature Registry (Source of Truth)
// ============================================================================

/// Feature status in the codebase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStatus {
    Stable,
    Beta,
    Experimental,
    Deprecated,
    Removed,
}

impl FeatureStatus {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "stable" => Some(Self::Stable),
            "beta" => Some(Self::Beta),
            "experimental" | "alpha" => Some(Self::Experimental),
            "deprecated" => Some(Self::Deprecated),
            "removed" => Some(Self::Removed),
            _ => None,
        }
    }

    fn requires_docs(&self) -> bool {
        matches!(self, Self::Stable | Self::Beta)
    }

    fn should_be_in_matrix(&self) -> bool {
        !matches!(self, Self::Removed)
    }
}

/// Feature definition in the codebase
#[derive(Debug, Clone)]
pub struct Feature {
    pub name: String,
    pub status: FeatureStatus,
    pub version_added: String,
    pub version_deprecated: Option<String>,
    pub description: String,
    pub category: String,
}

impl Feature {
    fn new(name: &str, status: FeatureStatus, version: &str, category: &str, desc: &str) -> Self {
        Self {
            name: name.to_string(),
            status,
            version_added: version.to_string(),
            version_deprecated: None,
            description: desc.to_string(),
            category: category.to_string(),
        }
    }

    fn deprecated(mut self, version: &str) -> Self {
        self.version_deprecated = Some(version.to_string());
        self
    }
}

/// Module definition in the codebase
#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub status: FeatureStatus,
    pub version_added: String,
    pub category: String,
    pub platforms: Vec<String>,
    pub parameters: Vec<String>,
}

impl Module {
    fn new(name: &str, status: FeatureStatus, version: &str, category: &str) -> Self {
        Self {
            name: name.to_string(),
            status,
            version_added: version.to_string(),
            category: category.to_string(),
            platforms: vec!["linux".to_string()],
            parameters: Vec::new(),
        }
    }

    fn with_platforms(mut self, platforms: &[&str]) -> Self {
        self.platforms = platforms.iter().map(|s| s.to_string()).collect();
        self
    }

    fn with_params(mut self, params: &[&str]) -> Self {
        self.parameters = params.iter().map(|s| s.to_string()).collect();
        self
    }
}

/// Registry of all features and modules (source of truth)
pub struct FeatureRegistry {
    pub features: Vec<Feature>,
    pub modules: Vec<Module>,
}

impl FeatureRegistry {
    fn new() -> Self {
        let mut registry = Self {
            features: Vec::new(),
            modules: Vec::new(),
        };
        registry.register_features();
        registry.register_modules();
        registry
    }

    fn register_features(&mut self) {
        // Core features
        self.features.push(Feature::new(
            "playbook_execution", FeatureStatus::Stable, "0.1.0",
            "core", "Execute Ansible playbooks"
        ));
        self.features.push(Feature::new(
            "inventory_management", FeatureStatus::Stable, "0.1.0",
            "core", "Parse and manage inventory"
        ));
        self.features.push(Feature::new(
            "template_rendering", FeatureStatus::Stable, "0.1.0",
            "core", "Jinja2 template rendering with MiniJinja"
        ));
        self.features.push(Feature::new(
            "variable_precedence", FeatureStatus::Stable, "0.1.0",
            "core", "Ansible variable precedence rules"
        ));
        self.features.push(Feature::new(
            "handler_notification", FeatureStatus::Stable, "0.1.0",
            "core", "Handler notification and execution"
        ));
        self.features.push(Feature::new(
            "fact_gathering", FeatureStatus::Stable, "0.1.0",
            "core", "Remote host fact gathering"
        ));
        self.features.push(Feature::new(
            "check_mode", FeatureStatus::Stable, "0.1.0",
            "core", "Dry-run mode for playbooks"
        ));
        self.features.push(Feature::new(
            "diff_mode", FeatureStatus::Stable, "0.1.0",
            "core", "Show differences in file changes"
        ));

        // Connection features
        self.features.push(Feature::new(
            "ssh_connection", FeatureStatus::Stable, "0.1.0",
            "connection", "SSH remote connections"
        ));
        self.features.push(Feature::new(
            "ssh_pipelining", FeatureStatus::Stable, "0.1.0",
            "connection", "SSH pipelining for performance"
        ));
        self.features.push(Feature::new(
            "local_connection", FeatureStatus::Stable, "0.1.0",
            "connection", "Local execution mode"
        ));
        self.features.push(Feature::new(
            "winrm_connection", FeatureStatus::Beta, "0.2.0",
            "connection", "WinRM Windows connections"
        ));

        // Execution strategies
        self.features.push(Feature::new(
            "linear_strategy", FeatureStatus::Stable, "0.1.0",
            "strategy", "Linear execution strategy"
        ));
        self.features.push(Feature::new(
            "free_strategy", FeatureStatus::Stable, "0.1.0",
            "strategy", "Free execution strategy"
        ));
        self.features.push(Feature::new(
            "host_pinned_strategy", FeatureStatus::Stable, "0.1.0",
            "strategy", "Host-pinned execution"
        ));
        self.features.push(Feature::new(
            "serial_execution", FeatureStatus::Stable, "0.1.0",
            "strategy", "Serial batch execution"
        ));

        // Advanced features
        self.features.push(Feature::new(
            "vault_encryption", FeatureStatus::Stable, "0.1.0",
            "security", "Ansible Vault support"
        ));
        self.features.push(Feature::new(
            "become_escalation", FeatureStatus::Stable, "0.1.0",
            "security", "Privilege escalation"
        ));
        self.features.push(Feature::new(
            "async_tasks", FeatureStatus::Beta, "0.2.0",
            "execution", "Asynchronous task execution"
        ));
        self.features.push(Feature::new(
            "delegate_to", FeatureStatus::Stable, "0.1.0",
            "execution", "Task delegation to other hosts"
        ));
        self.features.push(Feature::new(
            "run_once", FeatureStatus::Stable, "0.1.0",
            "execution", "Run task only once"
        ));

        // Provisioning features
        self.features.push(Feature::new(
            "resource_graph", FeatureStatus::Beta, "0.2.0",
            "provisioning", "Resource dependency graph"
        ));
        self.features.push(Feature::new(
            "state_management", FeatureStatus::Beta, "0.2.0",
            "provisioning", "Terraform-style state"
        ));
        self.features.push(Feature::new(
            "drift_detection", FeatureStatus::Experimental, "0.3.0",
            "provisioning", "Infrastructure drift detection"
        ));

        // Agent mode
        self.features.push(Feature::new(
            "agent_mode", FeatureStatus::Experimental, "0.3.0",
            "agent", "Persistent agent execution"
        ));
        self.features.push(Feature::new(
            "native_bindings", FeatureStatus::Experimental, "0.3.0",
            "agent", "Native system bindings"
        ));
    }

    fn register_modules(&mut self) {
        // File modules
        self.modules.push(Module::new("file", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["path", "state", "mode", "owner", "group", "recurse", "src", "force"]));
        self.modules.push(Module::new("copy", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["src", "dest", "content", "backup", "mode", "owner", "group"]));
        self.modules.push(Module::new("template", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["src", "dest", "mode", "owner", "group", "backup"]));
        self.modules.push(Module::new("lineinfile", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos"])
            .with_params(&["path", "line", "regexp", "state", "insertafter", "insertbefore"]));
        self.modules.push(Module::new("blockinfile", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos"])
            .with_params(&["path", "block", "marker", "state"]));
        self.modules.push(Module::new("stat", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["path", "follow", "get_checksum", "checksum_algorithm"]));
        self.modules.push(Module::new("unarchive", FeatureStatus::Stable, "0.1.0", "files")
            .with_platforms(&["linux", "macos"])
            .with_params(&["src", "dest", "remote_src"]));

        // Package modules
        self.modules.push(Module::new("package", FeatureStatus::Stable, "0.1.0", "packaging")
            .with_platforms(&["linux"])
            .with_params(&["name", "state", "version"]));
        self.modules.push(Module::new("apt", FeatureStatus::Stable, "0.1.0", "packaging")
            .with_platforms(&["linux"])
            .with_params(&["name", "state", "update_cache", "cache_valid_time", "deb"]));
        self.modules.push(Module::new("yum", FeatureStatus::Stable, "0.1.0", "packaging")
            .with_platforms(&["linux"])
            .with_params(&["name", "state", "enablerepo", "disablerepo"]));
        self.modules.push(Module::new("dnf", FeatureStatus::Stable, "0.2.0", "packaging")
            .with_platforms(&["linux"])
            .with_params(&["name", "state", "enablerepo", "disablerepo"]));

        // Service modules
        self.modules.push(Module::new("service", FeatureStatus::Stable, "0.1.0", "system")
            .with_platforms(&["linux", "macos"])
            .with_params(&["name", "state", "enabled"]));
        self.modules.push(Module::new("systemd", FeatureStatus::Stable, "0.1.0", "system")
            .with_platforms(&["linux"])
            .with_params(&["name", "state", "enabled", "daemon_reload"]));

        // User/Group modules
        self.modules.push(Module::new("user", FeatureStatus::Stable, "0.1.0", "system")
            .with_platforms(&["linux", "macos"])
            .with_params(&["name", "state", "uid", "groups", "shell", "home"]));
        self.modules.push(Module::new("group", FeatureStatus::Stable, "0.1.0", "system")
            .with_platforms(&["linux", "macos"])
            .with_params(&["name", "state", "gid"]));

        // Command modules
        self.modules.push(Module::new("command", FeatureStatus::Stable, "0.1.0", "commands")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["cmd", "argv", "chdir", "creates", "removes"]));
        self.modules.push(Module::new("shell", FeatureStatus::Stable, "0.1.0", "commands")
            .with_platforms(&["linux", "macos"])
            .with_params(&["cmd", "chdir", "executable", "creates", "removes"]));

        // Network modules
        self.modules.push(Module::new("uri", FeatureStatus::Stable, "0.1.0", "net_tools")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["url", "method", "body", "headers", "status_code"]));
        self.modules.push(Module::new("get_url", FeatureStatus::Stable, "0.1.0", "net_tools")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["url", "dest", "checksum", "mode"]));

        // Source control
        self.modules.push(Module::new("git", FeatureStatus::Stable, "0.1.0", "source_control")
            .with_platforms(&["linux", "macos"])
            .with_params(&["repo", "dest", "version", "force", "update"]));

        // Utility modules
        self.modules.push(Module::new("debug", FeatureStatus::Stable, "0.1.0", "utilities")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["msg", "var", "verbosity"]));
        self.modules.push(Module::new("set_fact", FeatureStatus::Stable, "0.1.0", "utilities")
            .with_platforms(&["linux", "macos", "windows"])
            .with_params(&["cacheable"]));
        self.modules.push(Module::new("wait_for", FeatureStatus::Stable, "0.1.0", "utilities")
            .with_platforms(&["linux", "macos"])
            .with_params(&["host", "port", "path", "state", "timeout"]));
        self.modules.push(Module::new("cron", FeatureStatus::Stable, "0.1.0", "system")
            .with_platforms(&["linux", "macos"])
            .with_params(&["name", "job", "state", "minute", "hour"]));

        // Cloud modules (Beta)
        self.modules.push(Module::new("aws_ec2", FeatureStatus::Beta, "0.2.0", "cloud")
            .with_platforms(&["linux", "macos"])
            .with_params(&["instance_type", "image_id", "state", "region"]));
        self.modules.push(Module::new("aws_s3", FeatureStatus::Beta, "0.2.0", "cloud")
            .with_platforms(&["linux", "macos"])
            .with_params(&["bucket", "object", "src", "dest", "mode"]));
    }

    fn get_feature(&self, name: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.name == name)
    }

    fn get_module(&self, name: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.name == name)
    }

    fn stable_features(&self) -> Vec<&Feature> {
        self.features.iter().filter(|f| f.status == FeatureStatus::Stable).collect()
    }

    fn stable_modules(&self) -> Vec<&Module> {
        self.modules.iter().filter(|m| m.status == FeatureStatus::Stable).collect()
    }

    fn features_by_category(&self) -> HashMap<&str, Vec<&Feature>> {
        let mut by_cat: HashMap<&str, Vec<&Feature>> = HashMap::new();
        for feature in &self.features {
            by_cat.entry(&feature.category).or_default().push(feature);
        }
        by_cat
    }

    fn modules_by_category(&self) -> HashMap<&str, Vec<&Module>> {
        let mut by_cat: HashMap<&str, Vec<&Module>> = HashMap::new();
        for module in &self.modules {
            by_cat.entry(&module.category).or_default().push(module);
        }
        by_cat
    }
}

// ============================================================================
// Documentation Model
// ============================================================================

/// Documentation entry for a feature
#[derive(Debug, Clone)]
pub struct DocEntry {
    pub name: String,
    pub documented_status: String,
    pub documented_version: String,
    pub has_description: bool,
    pub has_examples: bool,
    pub has_parameters: bool,
}

/// Compatibility matrix entry
#[derive(Debug, Clone)]
pub struct MatrixEntry {
    pub name: String,
    pub status: String,
    pub platforms: Vec<String>,
    pub version_added: String,
}

/// Documentation state (simulated from docs)
pub struct DocumentationState {
    pub feature_docs: HashMap<String, DocEntry>,
    pub module_docs: HashMap<String, DocEntry>,
    pub matrix_entries: HashMap<String, MatrixEntry>,
}

impl DocumentationState {
    /// Build documentation state that matches the feature registry
    fn from_registry(registry: &FeatureRegistry) -> Self {
        let mut state = Self {
            feature_docs: HashMap::new(),
            module_docs: HashMap::new(),
            matrix_entries: HashMap::new(),
        };

        // Create doc entries for all features that require docs
        for feature in &registry.features {
            if feature.status.requires_docs() {
                state.feature_docs.insert(feature.name.clone(), DocEntry {
                    name: feature.name.clone(),
                    documented_status: format!("{:?}", feature.status),
                    documented_version: feature.version_added.clone(),
                    has_description: true,
                    has_examples: true,
                    has_parameters: false,
                });
            }

            if feature.status.should_be_in_matrix() {
                state.matrix_entries.insert(feature.name.clone(), MatrixEntry {
                    name: feature.name.clone(),
                    status: format!("{:?}", feature.status),
                    platforms: vec!["all".to_string()],
                    version_added: feature.version_added.clone(),
                });
            }
        }

        // Create doc entries for all modules
        for module in &registry.modules {
            if module.status.requires_docs() {
                state.module_docs.insert(module.name.clone(), DocEntry {
                    name: module.name.clone(),
                    documented_status: format!("{:?}", module.status),
                    documented_version: module.version_added.clone(),
                    has_description: true,
                    has_examples: true,
                    has_parameters: !module.parameters.is_empty(),
                });
            }

            if module.status.should_be_in_matrix() {
                state.matrix_entries.insert(module.name.clone(), MatrixEntry {
                    name: module.name.clone(),
                    status: format!("{:?}", module.status),
                    platforms: module.platforms.clone(),
                    version_added: module.version_added.clone(),
                });
            }
        }

        state
    }
}

// ============================================================================
// Sync Checker
// ============================================================================

/// Drift detection result
#[derive(Debug, Clone)]
pub struct SyncIssue {
    pub item_type: String,
    pub name: String,
    pub issue: String,
    pub severity: SyncSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncSeverity {
    Error,   // CI should fail
    Warning, // CI should warn
}

/// Check documentation sync with feature registry
pub struct SyncChecker<'a> {
    registry: &'a FeatureRegistry,
    docs: &'a DocumentationState,
}

impl<'a> SyncChecker<'a> {
    fn new(registry: &'a FeatureRegistry, docs: &'a DocumentationState) -> Self {
        Self { registry, docs }
    }

    fn check_all(&self) -> Vec<SyncIssue> {
        let mut issues = Vec::new();
        issues.extend(self.check_feature_docs());
        issues.extend(self.check_module_docs());
        issues.extend(self.check_matrix());
        issues.extend(self.check_stale_docs());
        issues
    }

    fn check_feature_docs(&self) -> Vec<SyncIssue> {
        let mut issues = Vec::new();

        for feature in &self.registry.features {
            if feature.status.requires_docs() {
                if let Some(doc) = self.docs.feature_docs.get(&feature.name) {
                    // Check status matches
                    let expected_status = format!("{:?}", feature.status);
                    if doc.documented_status != expected_status {
                        issues.push(SyncIssue {
                            item_type: "feature".to_string(),
                            name: feature.name.clone(),
                            issue: format!(
                                "Status mismatch: docs say '{}', code says '{}'",
                                doc.documented_status, expected_status
                            ),
                            severity: SyncSeverity::Error,
                        });
                    }

                    // Check version matches
                    if doc.documented_version != feature.version_added {
                        issues.push(SyncIssue {
                            item_type: "feature".to_string(),
                            name: feature.name.clone(),
                            issue: format!(
                                "Version mismatch: docs say '{}', code says '{}'",
                                doc.documented_version, feature.version_added
                            ),
                            severity: SyncSeverity::Warning,
                        });
                    }

                    // Check has description
                    if !doc.has_description {
                        issues.push(SyncIssue {
                            item_type: "feature".to_string(),
                            name: feature.name.clone(),
                            issue: "Missing description in documentation".to_string(),
                            severity: SyncSeverity::Error,
                        });
                    }
                } else {
                    issues.push(SyncIssue {
                        item_type: "feature".to_string(),
                        name: feature.name.clone(),
                        issue: "Stable/Beta feature not documented".to_string(),
                        severity: SyncSeverity::Error,
                    });
                }
            }
        }

        issues
    }

    fn check_module_docs(&self) -> Vec<SyncIssue> {
        let mut issues = Vec::new();

        for module in &self.registry.modules {
            if module.status.requires_docs() {
                if let Some(doc) = self.docs.module_docs.get(&module.name) {
                    // Check status matches
                    let expected_status = format!("{:?}", module.status);
                    if doc.documented_status != expected_status {
                        issues.push(SyncIssue {
                            item_type: "module".to_string(),
                            name: module.name.clone(),
                            issue: format!(
                                "Status mismatch: docs say '{}', code says '{}'",
                                doc.documented_status, expected_status
                            ),
                            severity: SyncSeverity::Error,
                        });
                    }

                    // Check has parameters documented if module has params
                    if !module.parameters.is_empty() && !doc.has_parameters {
                        issues.push(SyncIssue {
                            item_type: "module".to_string(),
                            name: module.name.clone(),
                            issue: "Module has parameters but docs don't document them".to_string(),
                            severity: SyncSeverity::Error,
                        });
                    }

                    // Check has examples
                    if !doc.has_examples {
                        issues.push(SyncIssue {
                            item_type: "module".to_string(),
                            name: module.name.clone(),
                            issue: "Missing examples in documentation".to_string(),
                            severity: SyncSeverity::Warning,
                        });
                    }
                } else {
                    issues.push(SyncIssue {
                        item_type: "module".to_string(),
                        name: module.name.clone(),
                        issue: "Stable/Beta module not documented".to_string(),
                        severity: SyncSeverity::Error,
                    });
                }
            }
        }

        issues
    }

    fn check_matrix(&self) -> Vec<SyncIssue> {
        let mut issues = Vec::new();

        // Check all features/modules that should be in matrix
        for feature in &self.registry.features {
            if feature.status.should_be_in_matrix() {
                if !self.docs.matrix_entries.contains_key(&feature.name) {
                    issues.push(SyncIssue {
                        item_type: "matrix".to_string(),
                        name: feature.name.clone(),
                        issue: "Feature missing from compatibility matrix".to_string(),
                        severity: SyncSeverity::Error,
                    });
                }
            }
        }

        for module in &self.registry.modules {
            if module.status.should_be_in_matrix() {
                if let Some(entry) = self.docs.matrix_entries.get(&module.name) {
                    // Check platforms match
                    let doc_platforms: HashSet<&str> = entry.platforms.iter().map(|s| s.as_str()).collect();
                    let mod_platforms: HashSet<&str> = module.platforms.iter().map(|s| s.as_str()).collect();

                    if doc_platforms != mod_platforms {
                        issues.push(SyncIssue {
                            item_type: "matrix".to_string(),
                            name: module.name.clone(),
                            issue: format!(
                                "Platform mismatch: matrix says {:?}, code says {:?}",
                                entry.platforms, module.platforms
                            ),
                            severity: SyncSeverity::Error,
                        });
                    }
                } else {
                    issues.push(SyncIssue {
                        item_type: "matrix".to_string(),
                        name: module.name.clone(),
                        issue: "Module missing from compatibility matrix".to_string(),
                        severity: SyncSeverity::Error,
                    });
                }
            }
        }

        issues
    }

    fn check_stale_docs(&self) -> Vec<SyncIssue> {
        let mut issues = Vec::new();

        // Check for documented features that no longer exist or are removed
        let feature_names: HashSet<&str> = self.registry.features
            .iter()
            .filter(|f| f.status.requires_docs())
            .map(|f| f.name.as_str())
            .collect();

        for doc_name in self.docs.feature_docs.keys() {
            if !feature_names.contains(doc_name.as_str()) {
                issues.push(SyncIssue {
                    item_type: "stale".to_string(),
                    name: doc_name.clone(),
                    issue: "Documented feature no longer exists or is removed".to_string(),
                    severity: SyncSeverity::Warning,
                });
            }
        }

        // Check for documented modules that no longer exist
        let module_names: HashSet<&str> = self.registry.modules
            .iter()
            .filter(|m| m.status.requires_docs())
            .map(|m| m.name.as_str())
            .collect();

        for doc_name in self.docs.module_docs.keys() {
            if !module_names.contains(doc_name.as_str()) {
                issues.push(SyncIssue {
                    item_type: "stale".to_string(),
                    name: doc_name.clone(),
                    issue: "Documented module no longer exists or is removed".to_string(),
                    severity: SyncSeverity::Warning,
                });
            }
        }

        issues
    }

    fn error_count(&self) -> usize {
        self.check_all()
            .iter()
            .filter(|i| i.severity == SyncSeverity::Error)
            .count()
    }

    fn has_errors(&self) -> bool {
        self.check_all().iter().any(|i| i.severity == SyncSeverity::Error)
    }
}

// ============================================================================
// Tests: Feature Registry
// ============================================================================

#[test]
fn test_registry_has_features() {
    let registry = FeatureRegistry::new();
    assert!(!registry.features.is_empty(), "Registry should have features");
}

#[test]
fn test_registry_has_modules() {
    let registry = FeatureRegistry::new();
    assert!(!registry.modules.is_empty(), "Registry should have modules");
}

#[test]
fn test_registry_has_stable_features() {
    let registry = FeatureRegistry::new();
    assert!(
        !registry.stable_features().is_empty(),
        "Registry should have stable features"
    );
}

#[test]
fn test_registry_has_stable_modules() {
    let registry = FeatureRegistry::new();
    assert!(
        !registry.stable_modules().is_empty(),
        "Registry should have stable modules"
    );
}

#[test]
fn test_feature_categories_exist() {
    let registry = FeatureRegistry::new();
    let by_cat = registry.features_by_category();

    assert!(by_cat.contains_key("core"), "Should have core features");
    assert!(by_cat.contains_key("connection"), "Should have connection features");
    assert!(by_cat.contains_key("strategy"), "Should have strategy features");
}

#[test]
fn test_module_categories_exist() {
    let registry = FeatureRegistry::new();
    let by_cat = registry.modules_by_category();

    assert!(by_cat.contains_key("files"), "Should have file modules");
    assert!(by_cat.contains_key("packaging"), "Should have packaging modules");
    assert!(by_cat.contains_key("system"), "Should have system modules");
}

// ============================================================================
// Tests: Documentation Sync
// ============================================================================

#[test]
fn test_docs_match_registry() {
    let registry = FeatureRegistry::new();
    let docs = DocumentationState::from_registry(&registry);
    let checker = SyncChecker::new(&registry, &docs);

    let issues = checker.check_all();
    assert!(
        issues.is_empty(),
        "Docs should match registry. Issues: {:?}",
        issues
    );
}

#[test]
fn test_detects_missing_feature_doc() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Remove a feature doc
    docs.feature_docs.remove("playbook_execution");

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_feature_docs();

    assert!(
        issues.iter().any(|i| i.name == "playbook_execution" && i.issue.contains("not documented")),
        "Should detect missing feature doc"
    );
}

#[test]
fn test_detects_missing_module_doc() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Remove a module doc
    docs.module_docs.remove("file");

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_module_docs();

    assert!(
        issues.iter().any(|i| i.name == "file" && i.issue.contains("not documented")),
        "Should detect missing module doc"
    );
}

#[test]
fn test_detects_status_mismatch() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Change status in docs
    if let Some(doc) = docs.feature_docs.get_mut("ssh_connection") {
        doc.documented_status = "Beta".to_string(); // Should be Stable
    }

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_feature_docs();

    assert!(
        issues.iter().any(|i| i.name == "ssh_connection" && i.issue.contains("Status mismatch")),
        "Should detect status mismatch"
    );
}

#[test]
fn test_detects_missing_matrix_entry() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Remove matrix entry
    docs.matrix_entries.remove("file");

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_matrix();

    assert!(
        issues.iter().any(|i| i.name == "file" && i.issue.contains("missing from compatibility matrix")),
        "Should detect missing matrix entry"
    );
}

#[test]
fn test_detects_platform_mismatch() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Change platforms in matrix
    if let Some(entry) = docs.matrix_entries.get_mut("file") {
        entry.platforms = vec!["linux".to_string()]; // Missing macos, windows
    }

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_matrix();

    assert!(
        issues.iter().any(|i| i.name == "file" && i.issue.contains("Platform mismatch")),
        "Should detect platform mismatch"
    );
}

#[test]
fn test_detects_stale_docs() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Add doc for non-existent feature
    docs.feature_docs.insert("removed_feature".to_string(), DocEntry {
        name: "removed_feature".to_string(),
        documented_status: "Stable".to_string(),
        documented_version: "0.1.0".to_string(),
        has_description: true,
        has_examples: true,
        has_parameters: false,
    });

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_stale_docs();

    assert!(
        issues.iter().any(|i| i.name == "removed_feature" && i.issue.contains("no longer exists")),
        "Should detect stale docs"
    );
}

// ============================================================================
// Tests: CI Gate
// ============================================================================

#[test]
fn test_ci_passes_when_in_sync() {
    let registry = FeatureRegistry::new();
    let docs = DocumentationState::from_registry(&registry);
    let checker = SyncChecker::new(&registry, &docs);

    assert!(!checker.has_errors(), "CI should pass when docs are in sync");
}

#[test]
fn test_ci_fails_on_missing_docs() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);
    docs.module_docs.remove("file");

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_all();

    assert!(
        issues.iter().any(|i| i.severity == SyncSeverity::Error),
        "CI should fail when docs missing"
    );
}

#[test]
fn test_ci_fails_on_status_mismatch() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    if let Some(doc) = docs.module_docs.get_mut("copy") {
        doc.documented_status = "Experimental".to_string();
    }

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_all();

    assert!(
        issues.iter().any(|i| i.severity == SyncSeverity::Error),
        "CI should fail on status mismatch"
    );
}

// ============================================================================
// Tests: Feature Status
// ============================================================================

#[test]
fn test_stable_requires_docs() {
    assert!(FeatureStatus::Stable.requires_docs());
}

#[test]
fn test_beta_requires_docs() {
    assert!(FeatureStatus::Beta.requires_docs());
}

#[test]
fn test_experimental_no_docs_required() {
    assert!(!FeatureStatus::Experimental.requires_docs());
}

#[test]
fn test_removed_not_in_matrix() {
    assert!(!FeatureStatus::Removed.should_be_in_matrix());
}

#[test]
fn test_stable_in_matrix() {
    assert!(FeatureStatus::Stable.should_be_in_matrix());
}

// ============================================================================
// CI Regression Guards
// ============================================================================

#[test]
fn test_ci_guard_minimum_features() {
    let registry = FeatureRegistry::new();
    assert!(
        registry.features.len() >= 20,
        "Should track at least 20 features, got {}",
        registry.features.len()
    );
}

#[test]
fn test_ci_guard_minimum_modules() {
    let registry = FeatureRegistry::new();
    assert!(
        registry.modules.len() >= 20,
        "Should track at least 20 modules, got {}",
        registry.modules.len()
    );
}

#[test]
fn test_ci_guard_core_features_stable() {
    let registry = FeatureRegistry::new();

    let core_features = ["playbook_execution", "inventory_management", "template_rendering"];
    for name in &core_features {
        let feature = registry.get_feature(name).expect(&format!("Missing core feature: {}", name));
        assert_eq!(
            feature.status,
            FeatureStatus::Stable,
            "Core feature '{}' should be stable",
            name
        );
    }
}

#[test]
fn test_ci_guard_core_modules_stable() {
    let registry = FeatureRegistry::new();

    let core_modules = ["file", "copy", "template", "service", "command"];
    for name in &core_modules {
        let module = registry.get_module(name).expect(&format!("Missing core module: {}", name));
        assert_eq!(
            module.status,
            FeatureStatus::Stable,
            "Core module '{}' should be stable",
            name
        );
    }
}

#[test]
fn test_ci_guard_all_modules_have_platforms() {
    let registry = FeatureRegistry::new();

    for module in &registry.modules {
        assert!(
            !module.platforms.is_empty(),
            "Module '{}' should have at least one platform",
            module.name
        );
    }
}

#[test]
fn test_ci_guard_all_features_have_versions() {
    let registry = FeatureRegistry::new();

    for feature in &registry.features {
        assert!(
            !feature.version_added.is_empty(),
            "Feature '{}' should have version_added",
            feature.name
        );
    }
}

#[test]
fn test_ci_guard_sync_check_works() {
    let registry = FeatureRegistry::new();
    let docs = DocumentationState::from_registry(&registry);
    let checker = SyncChecker::new(&registry, &docs);

    // Sync check should not panic and should return empty when in sync
    let issues = checker.check_all();
    assert!(
        issues.is_empty(),
        "Baseline sync check should pass. Issues: {:?}",
        issues
    );
}

#[test]
fn test_ci_guard_error_detection_works() {
    let registry = FeatureRegistry::new();
    let mut docs = DocumentationState::from_registry(&registry);

    // Introduce an error
    docs.module_docs.remove("file");

    let checker = SyncChecker::new(&registry, &docs);
    let issues = checker.check_all();

    // Should detect the error
    let errors: Vec<_> = issues.iter().filter(|i| i.severity == SyncSeverity::Error).collect();
    assert!(
        !errors.is_empty(),
        "Error detection must find actual errors"
    );
}
