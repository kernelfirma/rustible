//! Variable Usage Analysis
//!
//! This module provides analysis of variable definitions, usage, and detection of
//! undefined or unused variables in playbooks.

use super::{helpers, AnalysisCategory, AnalysisFinding, AnalysisResult, Severity, SourceLocation};
use crate::playbook::{Play, Playbook};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Scope in which a variable is defined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VariableScope {
    /// Global/extra vars level
    Global,
    /// Playbook level
    Playbook,
    /// Play level (vars, vars_files)
    Play,
    /// Task level (vars, set_fact, register)
    Task,
    /// Loop variable (item, loop_var)
    Loop,
    /// Built-in variable (ansible_facts, inventory_hostname, etc.)
    BuiltIn,
    /// Role default
    RoleDefault,
    /// Role var
    RoleVar,
}

impl std::fmt::Display for VariableScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableScope::Global => write!(f, "global"),
            VariableScope::Playbook => write!(f, "playbook"),
            VariableScope::Play => write!(f, "play"),
            VariableScope::Task => write!(f, "task"),
            VariableScope::Loop => write!(f, "loop"),
            VariableScope::BuiltIn => write!(f, "built-in"),
            VariableScope::RoleDefault => write!(f, "role_default"),
            VariableScope::RoleVar => write!(f, "role_var"),
        }
    }
}

/// Information about a variable usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableUsage {
    /// Variable name
    pub name: String,
    /// Where the variable is defined (if known)
    pub defined_at: Option<SourceLocation>,
    /// Scope of definition
    pub scope: VariableScope,
    /// Locations where the variable is used
    pub used_at: Vec<SourceLocation>,
    /// Whether the variable is a fact
    pub is_fact: bool,
    /// Whether the variable is registered from a task
    pub is_registered: bool,
}

impl VariableUsage {
    pub fn new(name: impl Into<String>, scope: VariableScope) -> Self {
        Self {
            name: name.into(),
            defined_at: None,
            scope,
            used_at: Vec::new(),
            is_fact: false,
            is_registered: false,
        }
    }

    pub fn with_definition(mut self, location: SourceLocation) -> Self {
        self.defined_at = Some(location);
        self
    }

    pub fn add_usage(&mut self, location: SourceLocation) {
        self.used_at.push(location);
    }

    /// Returns true if the variable is defined but never used
    pub fn is_unused(&self) -> bool {
        self.used_at.is_empty() && self.defined_at.is_some()
    }

    /// Returns true if the variable is used but never defined
    pub fn is_undefined(&self) -> bool {
        self.defined_at.is_none() && !self.used_at.is_empty()
    }
}

/// Variable usage analyzer
pub struct VariableAnalyzer {
    /// Built-in variable names that should be considered always defined
    builtin_vars: HashSet<String>,
}

impl VariableAnalyzer {
    pub fn new() -> Self {
        Self {
            builtin_vars: Self::default_builtin_vars(),
        }
    }

    /// Get the default set of built-in variables
    fn default_builtin_vars() -> HashSet<String> {
        [
            // Ansible magic variables
            "ansible_facts",
            "ansible_local",
            "ansible_play_batch",
            "ansible_play_hosts",
            "ansible_play_hosts_all",
            "ansible_play_name",
            "ansible_playbook_python",
            "ansible_version",
            "ansible_check_mode",
            "ansible_diff_mode",
            "ansible_forks",
            "ansible_inventory_sources",
            "ansible_limit",
            "ansible_loop",
            "ansible_loop_var",
            "ansible_index_var",
            "ansible_parent_role_names",
            "ansible_parent_role_paths",
            "ansible_role_name",
            "ansible_role_names",
            "ansible_run_tags",
            "ansible_search_path",
            "ansible_skip_tags",
            "ansible_verbosity",
            // Host/inventory variables
            "inventory_hostname",
            "inventory_hostname_short",
            "inventory_file",
            "inventory_dir",
            "ansible_host",
            "ansible_port",
            "ansible_user",
            "ansible_connection",
            "ansible_python_interpreter",
            // Group variables
            "groups",
            "group_names",
            "hostvars",
            // Play context
            "play_hosts",
            "ansible_play_role_names",
            // Common facts
            "ansible_os_family",
            "ansible_distribution",
            "ansible_distribution_version",
            "ansible_distribution_major_version",
            "ansible_distribution_release",
            "ansible_pkg_mgr",
            "ansible_service_mgr",
            "ansible_hostname",
            "ansible_fqdn",
            "ansible_domain",
            "ansible_nodename",
            "ansible_machine",
            "ansible_architecture",
            "ansible_kernel",
            "ansible_kernel_version",
            "ansible_system",
            "ansible_system_vendor",
            "ansible_product_name",
            "ansible_product_version",
            "ansible_bios_version",
            "ansible_virtualization_type",
            "ansible_virtualization_role",
            "ansible_env",
            "ansible_user_id",
            "ansible_user_uid",
            "ansible_user_gid",
            "ansible_user_gecos",
            "ansible_user_dir",
            "ansible_user_shell",
            "ansible_default_ipv4",
            "ansible_default_ipv6",
            "ansible_all_ipv4_addresses",
            "ansible_all_ipv6_addresses",
            "ansible_interfaces",
            "ansible_memtotal_mb",
            "ansible_memfree_mb",
            "ansible_swaptotal_mb",
            "ansible_swapfree_mb",
            "ansible_processor",
            "ansible_processor_count",
            "ansible_processor_cores",
            "ansible_processor_threads_per_core",
            "ansible_processor_vcpus",
            "ansible_devices",
            "ansible_mounts",
            "ansible_selinux",
            "ansible_apparmor",
            // Loop variables
            "item",
            "ansible_loop",
            // Role variables
            "role_path",
            "role_name",
            // Special variables
            "omit",
            "none",
            "true",
            "false",
            "undefined",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// Add custom built-in variable names
    pub fn add_builtin(&mut self, name: impl Into<String>) {
        self.builtin_vars.insert(name.into());
    }

    /// Analyze a playbook for variable usage issues
    pub fn analyze(&self, playbook: &Playbook) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();
        let mut all_definitions: HashMap<String, VariableUsage> = HashMap::new();
        let mut all_usages: HashMap<String, Vec<SourceLocation>> = HashMap::new();

        let source_file = playbook
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        // First pass: collect all variable definitions
        for (play_idx, play) in playbook.plays.iter().enumerate() {
            self.collect_play_definitions(play, play_idx, &source_file, &mut all_definitions);
        }

        // Second pass: collect all variable usages
        for (play_idx, play) in playbook.plays.iter().enumerate() {
            self.collect_play_usages(play, play_idx, &source_file, &mut all_usages);
        }

        // Analyze for issues
        // Check for undefined variables
        for (var_name, locations) in &all_usages {
            if !all_definitions.contains_key(var_name) && !self.builtin_vars.contains(var_name) {
                for location in locations {
                    findings.push(
                        AnalysisFinding::new(
                            "VAR001",
                            AnalysisCategory::Variable,
                            Severity::Warning,
                            format!("Variable '{}' may be undefined", var_name),
                        )
                        .with_description(
                            "This variable is used but not defined in the playbook. \
                             It might be defined in inventory, extra vars, or role defaults.",
                        )
                        .with_location(location.clone())
                        .with_suggestion(format!(
                            "Ensure '{}' is defined before use, or use the 'default' filter: \
                             {{{{ {} | default('value') }}}}",
                            var_name, var_name
                        )),
                    );
                }
            }
        }

        // Check for unused variables (only in play/task scope)
        for (var_name, usage) in &all_definitions {
            if !all_usages.contains_key(var_name) {
                // Only report for play/task scope variables
                if matches!(usage.scope, VariableScope::Play | VariableScope::Task) {
                    if let Some(location) = &usage.defined_at {
                        findings.push(
                            AnalysisFinding::new(
                                "VAR002",
                                AnalysisCategory::Variable,
                                Severity::Hint,
                                format!("Variable '{}' is defined but never used", var_name),
                            )
                            .with_description(
                                "This variable is defined but not referenced anywhere in the playbook."
                            )
                            .with_location(location.clone())
                            .with_suggestion(
                                "Remove the unused variable or verify it's needed for external consumers."
                            ),
                        );
                    }
                }
            }
        }

        // Check for variable shadowing
        findings.extend(self.check_variable_shadowing(playbook, &source_file)?);

        // Check for potential typos (similar variable names)
        findings.extend(self.check_potential_typos(&all_definitions, &all_usages)?);

        Ok(findings)
    }

    /// Collect variable definitions from a play
    fn collect_play_definitions(
        &self,
        play: &Play,
        play_idx: usize,
        source_file: &Option<String>,
        definitions: &mut HashMap<String, VariableUsage>,
    ) {
        let play_location = SourceLocation::new().with_play(play_idx, &play.name);
        let play_location = if let Some(f) = source_file {
            play_location.with_file(f.clone())
        } else {
            play_location
        };

        // Play-level variables
        for var_name in play.vars.as_map().keys() {
            definitions.insert(
                var_name.clone(),
                VariableUsage::new(var_name, VariableScope::Play)
                    .with_definition(play_location.clone()),
            );
        }

        // Tasks
        let all_tasks = helpers::get_all_tasks(play);
        for (task_idx, task) in all_tasks.iter().enumerate() {
            let task_location = SourceLocation::new()
                .with_play(play_idx, &play.name)
                .with_task(task_idx, &task.name);
            let task_location = if let Some(f) = source_file {
                task_location.with_file(f.clone())
            } else {
                task_location
            };

            // Task-level vars
            for var_name in task.vars.as_map().keys() {
                definitions.insert(
                    var_name.clone(),
                    VariableUsage::new(var_name, VariableScope::Task)
                        .with_definition(task_location.clone()),
                );
            }

            // Register variable
            if let Some(register_var) = &task.register {
                let mut usage = VariableUsage::new(register_var, VariableScope::Task)
                    .with_definition(task_location.clone());
                usage.is_registered = true;
                definitions.insert(register_var.clone(), usage);
            }

            // set_fact module
            if task.module.name == "set_fact" || task.module.name == "ansible.builtin.set_fact" {
                if let Some(obj) = task.module.args.as_object() {
                    for key in obj.keys() {
                        if key != "cacheable" {
                            let mut usage = VariableUsage::new(key, VariableScope::Task)
                                .with_definition(task_location.clone());
                            usage.is_fact = true;
                            definitions.insert(key.clone(), usage);
                        }
                    }
                }
            }

            // Loop variables
            if task.loop_.is_some() || task.with_items.is_some() {
                let loop_var = task
                    .loop_control
                    .as_ref()
                    .map(|lc| lc.loop_var.clone())
                    .unwrap_or_else(|| "item".to_string());
                definitions.insert(
                    loop_var.clone(),
                    VariableUsage::new(loop_var, VariableScope::Loop)
                        .with_definition(task_location.clone()),
                );

                if let Some(lc) = &task.loop_control {
                    if let Some(index_var) = &lc.index_var {
                        definitions.insert(
                            index_var.clone(),
                            VariableUsage::new(index_var, VariableScope::Loop)
                                .with_definition(task_location.clone()),
                        );
                    }
                }
            }
        }
    }

    /// Collect variable usages from a play
    fn collect_play_usages(
        &self,
        play: &Play,
        play_idx: usize,
        source_file: &Option<String>,
        usages: &mut HashMap<String, Vec<SourceLocation>>,
    ) {
        // Collect from play vars (values might reference other variables)
        for value in play.vars.as_map().values() {
            let vars = helpers::extract_value_variables(value);
            let location = SourceLocation::new().with_play(play_idx, &play.name);
            let location = if let Some(f) = source_file {
                location.with_file(f.clone())
            } else {
                location
            };
            for var in vars {
                usages.entry(var).or_default().push(location.clone());
            }
        }

        // Collect from tasks
        let all_tasks = helpers::get_all_tasks(play);
        for (task_idx, task) in all_tasks.iter().enumerate() {
            let location = SourceLocation::new()
                .with_play(play_idx, &play.name)
                .with_task(task_idx, &task.name);
            let location = if let Some(f) = source_file {
                location.with_file(f.clone())
            } else {
                location
            };

            // From when conditions
            if let Some(when) = &task.when {
                let conditions = when.conditions();
                for condition in conditions {
                    let vars = helpers::extract_when_variables(condition);
                    for var in vars {
                        usages.entry(var).or_default().push(location.clone());
                    }
                }
            }

            // From module args
            let vars = helpers::extract_value_variables(&task.module.args);
            for var in vars {
                usages.entry(var).or_default().push(location.clone());
            }

            // From loop expression
            if let Some(loop_expr) = &task.loop_ {
                let vars = helpers::extract_value_variables(loop_expr);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            // From with_items
            if let Some(with_items) = &task.with_items {
                let vars = helpers::extract_value_variables(with_items);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            // From task vars (values)
            for value in task.vars.as_map().values() {
                let vars = helpers::extract_value_variables(value);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            // From environment
            for value in task.environment.values() {
                let vars = helpers::extract_jinja_variables(value);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            // From changed_when/failed_when
            if let Some(changed_when) = &task.changed_when {
                let vars = helpers::extract_when_variables(changed_when);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            if let Some(failed_when) = &task.failed_when {
                let vars = helpers::extract_when_variables(failed_when);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }

            // From until condition
            if let Some(until) = &task.until {
                let vars = helpers::extract_when_variables(until);
                for var in vars {
                    usages.entry(var).or_default().push(location.clone());
                }
            }
        }
    }

    /// Check for variable shadowing (same name defined in multiple scopes)
    fn check_variable_shadowing(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            let play_vars: HashSet<_> = play.vars.as_map().keys().cloned().collect();

            let all_tasks = helpers::get_all_tasks(play);
            for (task_idx, task) in all_tasks.iter().enumerate() {
                // Check if task vars shadow play vars
                for var_name in task.vars.as_map().keys() {
                    if play_vars.contains(var_name) {
                        let location = SourceLocation::new()
                            .with_play(play_idx, &play.name)
                            .with_task(task_idx, &task.name);
                        let location = if let Some(f) = source_file {
                            location.with_file(f.clone())
                        } else {
                            location
                        };

                        findings.push(
                            AnalysisFinding::new(
                                "VAR003",
                                AnalysisCategory::Variable,
                                Severity::Info,
                                format!("Variable '{}' shadows play-level variable", var_name),
                            )
                            .with_description(
                                "This variable is defined at both play and task level. \
                                 The task-level definition will take precedence.",
                            )
                            .with_location(location)
                            .with_suggestion(
                                "Consider using a different name to avoid confusion, \
                                 or remove the duplicate definition.",
                            ),
                        );
                    }
                }
            }
        }

        Ok(findings)
    }

    /// Check for potential typos (similar variable names)
    fn check_potential_typos(
        &self,
        definitions: &HashMap<String, VariableUsage>,
        usages: &HashMap<String, Vec<SourceLocation>>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        let defined_names: Vec<_> = definitions.keys().collect();

        for (used_name, locations) in usages {
            if !definitions.contains_key(used_name) && !self.builtin_vars.contains(used_name) {
                // Look for similar names
                for defined_name in &defined_names {
                    if self.is_likely_typo(used_name, defined_name) {
                        for location in locations {
                            findings.push(
                                AnalysisFinding::new(
                                    "VAR004",
                                    AnalysisCategory::Variable,
                                    Severity::Warning,
                                    format!(
                                        "Variable '{}' might be a typo of '{}'",
                                        used_name, defined_name
                                    ),
                                )
                                .with_description(
                                    "This undefined variable name is very similar to a defined one."
                                )
                                .with_location(location.clone())
                                .with_suggestion(format!(
                                    "Did you mean '{}'?",
                                    defined_name
                                )),
                            );
                        }
                        break; // Only report one suggestion per undefined variable
                    }
                }
            }
        }

        Ok(findings)
    }

    /// Check if two variable names are likely typos of each other
    fn is_likely_typo(&self, a: &str, b: &str) -> bool {
        if a.len() < 3 || b.len() < 3 {
            return false;
        }

        let distance = Self::levenshtein_distance(a, b);
        let max_len = a.len().max(b.len());

        // Allow 1 edit for short names, 2 for longer
        let threshold = if max_len <= 5 { 1 } else { 2 };

        distance > 0 && distance <= threshold
    }

    /// Calculate Levenshtein distance between two strings
    #[allow(clippy::needless_range_loop)]
    fn levenshtein_distance(a: &str, b: &str) -> usize {
        let a_chars: Vec<_> = a.chars().collect();
        let b_chars: Vec<_> = b.chars().collect();

        let a_len = a_chars.len();
        let b_len = b_chars.len();

        if a_len == 0 {
            return b_len;
        }
        if b_len == 0 {
            return a_len;
        }

        let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

        for i in 0..=a_len {
            matrix[i][0] = i;
        }
        for j in 0..=b_len {
            matrix[0][j] = j;
        }

        for i in 1..=a_len {
            for j in 1..=b_len {
                let cost = if a_chars[i - 1] == b_chars[j - 1] {
                    0
                } else {
                    1
                };
                matrix[i][j] = (matrix[i - 1][j] + 1)
                    .min(matrix[i][j - 1] + 1)
                    .min(matrix[i - 1][j - 1] + cost);
            }
        }

        matrix[a_len][b_len]
    }
}

impl Default for VariableAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(VariableAnalyzer::levenshtein_distance("hello", "hello"), 0);
        assert_eq!(VariableAnalyzer::levenshtein_distance("hello", "hallo"), 1);
        assert_eq!(VariableAnalyzer::levenshtein_distance("hello", "help"), 2);
        assert_eq!(VariableAnalyzer::levenshtein_distance("", "abc"), 3);
        assert_eq!(VariableAnalyzer::levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn test_is_likely_typo() {
        let analyzer = VariableAnalyzer::new();
        assert!(analyzer.is_likely_typo("my_variable", "my_varable")); // 1 char diff
        assert!(analyzer.is_likely_typo("server_name", "sever_name")); // missing 'r'
        assert!(!analyzer.is_likely_typo("abc", "xyz")); // too different
        assert!(!analyzer.is_likely_typo("ab", "ba")); // too short
    }

    #[test]
    fn test_variable_scope_display() {
        assert_eq!(format!("{}", VariableScope::Play), "play");
        assert_eq!(format!("{}", VariableScope::Task), "task");
        assert_eq!(format!("{}", VariableScope::BuiltIn), "built-in");
    }
}
