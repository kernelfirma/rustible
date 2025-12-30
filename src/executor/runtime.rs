//! Runtime context for Rustible execution
//!
//! This module provides:
//! - Variable scoping (global, play, task, host)
//! - Fact storage
//! - Register system for task results

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::connection::Connection;

/// Scope levels for variable resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VarScope {
    /// Built-in variables (lowest precedence)
    Builtin,
    /// Inventory group variables
    GroupVars,
    /// Inventory host variables
    HostVars,
    /// Playbook variables
    PlaybookVars,
    /// Play-level variables
    PlayVars,
    /// Block variables
    BlockVars,
    /// Task variables
    TaskVars,
    /// Registered variables
    Registered,
    /// Set_fact / include_vars
    SetFact,
    /// Extra vars from command line (highest precedence)
    ExtraVars,
}

/// Container for host-specific variables
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostVars {
    /// Variables specific to this host
    vars: IndexMap<String, JsonValue>,
    /// Facts gathered from this host
    facts: IndexMap<String, JsonValue>,
    /// Registered task results
    registered: IndexMap<String, RegisteredResult>,
}

impl HostVars {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a variable for this host
    pub fn set_var(&mut self, name: String, value: JsonValue) {
        self.vars.insert(name, value);
    }

    /// Get a variable for this host
    pub fn get_var(&self, name: &str) -> Option<&JsonValue> {
        self.vars.get(name)
    }

    /// Set a fact for this host
    pub fn set_fact(&mut self, name: String, value: JsonValue) {
        self.facts.insert(name, value);
    }

    /// Get a fact for this host
    pub fn get_fact(&self, name: &str) -> Option<&JsonValue> {
        self.facts.get(name)
    }

    /// Get all facts for this host
    pub fn get_all_facts(&self) -> &IndexMap<String, JsonValue> {
        &self.facts
    }

    /// Register a task result
    pub fn register(&mut self, name: String, result: RegisteredResult) {
        self.registered.insert(name, result);
    }

    /// Get a registered result
    pub fn get_registered(&self, name: &str) -> Option<&RegisteredResult> {
        self.registered.get(name)
    }

    /// Merge another HostVars into this one
    pub fn merge(&mut self, other: &HostVars) {
        for (k, v) in &other.vars {
            self.vars.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.facts {
            self.facts.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.registered {
            self.registered.insert(k.clone(), v.clone());
        }
    }
}

/// Result of a task that can be registered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredResult {
    /// Whether the task changed something
    pub changed: bool,
    /// Whether the task failed
    pub failed: bool,
    /// Whether the task was skipped
    pub skipped: bool,
    /// Return code (for command/shell modules)
    pub rc: Option<i32>,
    /// Standard output
    pub stdout: Option<String>,
    /// Standard output as lines
    pub stdout_lines: Option<Vec<String>>,
    /// Standard error
    pub stderr: Option<String>,
    /// Standard error as lines
    pub stderr_lines: Option<Vec<String>>,
    /// Message from the task
    pub msg: Option<String>,
    /// Results for loop tasks
    pub results: Option<Vec<RegisteredResult>>,
    /// Module-specific data
    #[serde(flatten)]
    pub data: IndexMap<String, JsonValue>,
}

impl Default for RegisteredResult {
    fn default() -> Self {
        Self {
            changed: false,
            failed: false,
            skipped: false,
            rc: None,
            stdout: None,
            stdout_lines: None,
            stderr: None,
            stderr_lines: None,
            msg: None,
            results: None,
            data: IndexMap::new(),
        }
    }
}

impl RegisteredResult {
    /// Create a new registered result
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a successful result
    pub fn ok(changed: bool) -> Self {
        Self {
            changed,
            ..Default::default()
        }
    }

    /// Create a failed result
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            failed: true,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create a skipped result
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            skipped: true,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Convert to JSON value
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or(JsonValue::Null)
    }
}

/// Group definition in inventory
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryGroup {
    /// Hosts in this group
    pub hosts: Vec<String>,
    /// Variables for this group
    pub vars: IndexMap<String, JsonValue>,
    /// Child groups
    pub children: Vec<String>,
}

/// Execution context passed to tasks
#[derive(Clone)]
pub struct ExecutionContext {
    /// Current host being executed on
    pub host: String,
    /// Whether we're in check mode (dry-run)
    pub check_mode: bool,
    /// Whether to show diffs
    pub diff_mode: bool,
    /// Verbosity level (0-4)
    pub verbosity: u8,
    /// Optional connection for remote execution
    pub connection: Option<Arc<dyn Connection>>,
    /// Python interpreter path on remote host
    pub python_interpreter: String,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("host", &self.host)
            .field("check_mode", &self.check_mode)
            .field("diff_mode", &self.diff_mode)
            .field("verbosity", &self.verbosity)
            .field(
                "connection",
                &self.connection.as_ref().map(|c| c.identifier()),
            )
            .field("python_interpreter", &self.python_interpreter)
            .finish()
    }
}

impl ExecutionContext {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            connection: None,
            python_interpreter: "/usr/bin/python3".to_string(),
        }
    }

    pub fn with_check_mode(mut self, check: bool) -> Self {
        self.check_mode = check;
        self
    }

    pub fn with_diff_mode(mut self, diff: bool) -> Self {
        self.diff_mode = diff;
        self
    }

    pub fn with_verbosity(mut self, verbosity: u8) -> Self {
        self.verbosity = verbosity;
        self
    }

    /// Set the connection for remote execution
    pub fn with_connection(mut self, conn: Arc<dyn Connection>) -> Self {
        self.connection = Some(conn);
        self
    }

    /// Set the Python interpreter path
    pub fn with_python_interpreter(mut self, path: impl Into<String>) -> Self {
        self.python_interpreter = path.into();
        self
    }
}

/// The main runtime context holding all state during execution
#[derive(Debug, Default)]
pub struct RuntimeContext {
    /// Global variables (from inventory, playbook vars_files, etc.)
    global_vars: IndexMap<String, JsonValue>,

    /// Play-level variables
    play_vars: IndexMap<String, JsonValue>,

    /// Task-level variables
    task_vars: IndexMap<String, JsonValue>,

    /// Extra variables (highest precedence)
    extra_vars: IndexMap<String, JsonValue>,

    /// Per-host variables and facts
    host_data: IndexMap<String, HostVars>,

    /// Inventory groups
    groups: IndexMap<String, InventoryGroup>,

    /// Special "all" group containing all hosts
    all_hosts: Vec<String>,

    /// Magic variables
    magic_vars: IndexMap<String, JsonValue>,

    /// Role defaults (lowest precedence)
    role_defaults: IndexMap<String, JsonValue>,

    /// Block-level variables
    block_vars: IndexMap<String, JsonValue>,

    /// Include vars (from include_vars module)
    include_vars: IndexMap<String, JsonValue>,

    /// Role params (when including roles)
    role_params: IndexMap<String, JsonValue>,

    /// Include params
    include_params: IndexMap<String, JsonValue>,
}

impl RuntimeContext {
    /// Create a new runtime context
    pub fn new() -> Self {
        let mut ctx = Self::default();
        ctx.init_magic_vars();
        ctx
    }

    /// Create a runtime context from an inventory
    pub fn from_inventory(inventory: &crate::inventory::Inventory) -> Self {
        let mut ctx = Self::new();

        // Add hosts from inventory
        for host in inventory.hosts() {
            // Convert host variables from serde_yaml to serde_json
            for (key, value) in &host.vars {
                if let Ok(json_value) = serde_json::to_value(value) {
                    ctx.set_host_var(host.name(), key.clone(), json_value);
                }
            }
        }

        ctx
    }

    /// Initialize magic variables
    fn init_magic_vars(&mut self) {
        self.magic_vars.insert(
            "ansible_version".to_string(),
            serde_json::json!({
                "full": env!("CARGO_PKG_VERSION"),
                "major": 2,
                "minor": 16,
                "revision": 0,
                "string": format!("rustible {}", env!("CARGO_PKG_VERSION"))
            }),
        );

        self.magic_vars.insert(
            "rustible_version".to_string(),
            serde_json::json!(env!("CARGO_PKG_VERSION")),
        );

        // Playbook directory will be set when playbook is loaded
        self.magic_vars
            .insert("playbook_dir".to_string(), JsonValue::Null);

        self.magic_vars
            .insert("inventory_dir".to_string(), JsonValue::Null);
    }

    /// Set a global variable
    pub fn set_global_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting global var: {} = {:?}", name, value);
        self.global_vars.insert(name, value);
    }

    /// Set a play-level variable
    pub fn set_play_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting play var: {} = {:?}", name, value);
        self.play_vars.insert(name, value);
    }

    /// Set a task-level variable
    pub fn set_task_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting task var: {} = {:?}", name, value);
        self.task_vars.insert(name, value);
    }

    /// Set an extra variable (highest precedence)
    pub fn set_extra_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting extra var: {} = {:?}", name, value);
        self.extra_vars.insert(name, value);
    }

    /// Clear task-level variables (called between tasks)
    pub fn clear_task_vars(&mut self) {
        self.task_vars.clear();
    }

    /// Remove specific task-level variables by name
    /// Used for cleaning up loop variables without clearing all task vars
    pub fn remove_task_vars(&mut self, names: &[&str]) {
        for name in names {
            self.task_vars.swap_remove(*name);
        }
    }

    /// Clear play-level variables (called between plays)
    pub fn clear_play_vars(&mut self) {
        self.play_vars.clear();
        self.task_vars.clear();
    }

    /// Get a variable by name, respecting precedence
    ///
    /// # Performance
    /// Hot path function - inline hint for better optimization.
    #[inline]
    pub fn get_var(&self, name: &str, host: Option<&str>) -> Option<JsonValue> {
        // Check in order of precedence (highest first)

        // Extra vars (highest)
        if let Some(v) = self.extra_vars.get(name) {
            return Some(v.clone());
        }

        // Registered variables and set_fact (check host data)
        if let Some(host_name) = host {
            if let Some(host_data) = self.host_data.get(host_name) {
                if let Some(reg) = host_data.get_registered(name) {
                    return Some(reg.to_json());
                }
            }
        }

        // Task variables
        if let Some(v) = self.task_vars.get(name) {
            return Some(v.clone());
        }

        // Play variables
        if let Some(v) = self.play_vars.get(name) {
            return Some(v.clone());
        }

        // Global variables
        if let Some(v) = self.global_vars.get(name) {
            return Some(v.clone());
        }

        // Host variables
        if let Some(host_name) = host {
            if let Some(host_data) = self.host_data.get(host_name) {
                if let Some(v) = host_data.get_var(name) {
                    return Some(v.clone());
                }
            }
        }

        // Magic variables
        if let Some(v) = self.magic_vars.get(name) {
            return Some(v.clone());
        }

        None
    }

    /// Get all variables merged for a specific host
    ///
    /// # Performance
    /// This is a hot path function. Optimizations applied:
    /// - Pre-sized capacity for IndexMap to reduce reallocations
    /// - Cached host string comparison
    /// - Inline hint for better optimization
    #[inline]
    pub fn get_merged_vars(&self, host: &str) -> IndexMap<String, JsonValue> {
        // OPTIMIZATION: Pre-allocate with estimated capacity to reduce reallocations
        let host_facts_count = self
            .host_data
            .get(host)
            .map(|hd| hd.facts.len())
            .unwrap_or(0);
        let estimated_size = self.magic_vars.len()
            + self.global_vars.len()
            + self.play_vars.len()
            + self.task_vars.len()
            + self.extra_vars.len()
            + host_facts_count // For top-level ansible_* fact variables
            + 10; // Buffer for special vars
        let mut merged = IndexMap::with_capacity(estimated_size);

        // OPTIMIZATION: Cache host string for comparisons
        let host_string = host.to_string();

        // Start with magic vars (lowest)
        for (k, v) in &self.magic_vars {
            merged.insert(k.clone(), v.clone());
        }

        // Global vars
        for (k, v) in &self.global_vars {
            merged.insert(k.clone(), v.clone());
        }

        // Group vars for groups this host is in
        for (_group_name, group) in &self.groups {
            if group.hosts.contains(&host_string) {
                for (k, v) in &group.vars {
                    merged.insert(k.clone(), v.clone());
                }
            }
        }

        // Host-specific vars
        if let Some(host_data) = self.host_data.get(host) {
            for (k, v) in &host_data.vars {
                merged.insert(k.clone(), v.clone());
            }
        }

        // Play vars
        for (k, v) in &self.play_vars {
            merged.insert(k.clone(), v.clone());
        }

        // Task vars
        for (k, v) in &self.task_vars {
            merged.insert(k.clone(), v.clone());
        }

        // Host facts (under 'ansible_facts' namespace and as top-level ansible_* variables)
        if let Some(host_data) = self.host_data.get(host) {
            if !host_data.facts.is_empty() {
                // Store facts under ansible_facts for backwards compatibility
                merged.insert(
                    "ansible_facts".to_string(),
                    serde_json::to_value(host_data.get_all_facts()).unwrap_or(JsonValue::Null),
                );

                // Also expose each fact as a top-level ansible_* variable
                // Facts like {"hostname": "server1"} become {"ansible_hostname": "server1"}
                for (fact_name, fact_value) in host_data.get_all_facts() {
                    let prefixed_name = if fact_name.starts_with("ansible_") {
                        fact_name.clone()
                    } else {
                        format!("ansible_{}", fact_name)
                    };
                    merged.insert(prefixed_name, fact_value.clone());
                }
            }

            // Registered vars
            for (k, v) in &host_data.registered {
                merged.insert(k.clone(), v.to_json());
            }
        }

        // Extra vars (highest)
        for (k, v) in &self.extra_vars {
            merged.insert(k.clone(), v.clone());
        }

        // Add special vars
        merged.insert(
            "inventory_hostname".to_string(),
            JsonValue::String(host_string.clone()),
        );
        merged.insert(
            "inventory_hostname_short".to_string(),
            JsonValue::String(host.split('.').next().unwrap_or(host).to_string()),
        );

        // Add group names this host belongs to
        let group_names: Vec<String> = self
            .groups
            .iter()
            .filter(|(_, g)| g.hosts.contains(&host_string))
            .map(|(name, _)| name.clone())
            .collect();
        merged.insert(
            "group_names".to_string(),
            serde_json::to_value(&group_names).unwrap_or(JsonValue::Array(vec![])),
        );

        merged
    }

    /// Add a host to the inventory
    pub fn add_host(&mut self, host: String, group: Option<&str>) {
        debug!("Adding host: {} to group: {:?}", host, group);

        if !self.all_hosts.contains(&host) {
            self.all_hosts.push(host.clone());
        }

        self.host_data
            .entry(host.clone())
            .or_insert_with(HostVars::new);

        if let Some(group_name) = group {
            let group = self
                .groups
                .entry(group_name.to_string())
                .or_insert_with(InventoryGroup::default);

            if !group.hosts.contains(&host) {
                group.hosts.push(host);
            }
        }
    }

    /// Add a group to the inventory
    pub fn add_group(&mut self, name: String, group: InventoryGroup) {
        debug!("Adding group: {}", name);
        self.groups.insert(name, group);
    }

    /// Get all hosts (returns a reference to avoid cloning)
    pub fn get_all_hosts(&self) -> Vec<String> {
        // Note: Cloning is necessary here as callers may modify the list
        // or need ownership. Consider using Cow for future optimization.
        self.all_hosts.clone()
    }

    /// Get a reference to all hosts (avoids cloning when only reading)
    #[inline]
    pub fn hosts(&self) -> &[String] {
        &self.all_hosts
    }

    /// Get hosts in a group
    pub fn get_group_hosts(&self, group: &str) -> Option<Vec<String>> {
        self.groups.get(group).map(|g| {
            let mut hosts = g.hosts.clone();

            // Include hosts from child groups
            for child in &g.children {
                if let Some(child_hosts) = self.get_group_hosts(child) {
                    for h in child_hosts {
                        if !hosts.contains(&h) {
                            hosts.push(h);
                        }
                    }
                }
            }

            hosts
        })
    }

    /// Set a fact for a host
    pub fn set_host_fact(&mut self, host: &str, name: String, value: JsonValue) {
        let host_data = self
            .host_data
            .entry(host.to_string())
            .or_insert_with(HostVars::new);
        host_data.set_fact(name, value);
    }

    /// Get a fact for a host
    pub fn get_host_fact(&self, host: &str, name: &str) -> Option<JsonValue> {
        self.host_data
            .get(host)
            .and_then(|hd| hd.get_fact(name).cloned())
    }

    /// Set all facts for a host
    pub fn set_host_facts(&mut self, host: &str, facts: IndexMap<String, JsonValue>) {
        let host_data = self
            .host_data
            .entry(host.to_string())
            .or_insert_with(HostVars::new);
        for (k, v) in facts {
            host_data.set_fact(k, v);
        }
    }

    /// Register a task result for a host
    pub fn register_result(&mut self, host: &str, name: String, result: RegisteredResult) {
        debug!("Registering result '{}' for host '{}'", name, host);
        let host_data = self
            .host_data
            .entry(host.to_string())
            .or_insert_with(HostVars::new);
        host_data.register(name, result);
    }

    /// Get a registered result for a host
    pub fn get_registered(&self, host: &str, name: &str) -> Option<&RegisteredResult> {
        self.host_data
            .get(host)
            .and_then(|hd| hd.get_registered(name))
    }

    /// Set a host variable
    pub fn set_host_var(&mut self, host: &str, name: String, value: JsonValue) {
        let host_data = self
            .host_data
            .entry(host.to_string())
            .or_insert_with(HostVars::new);
        host_data.set_var(name, value);
    }

    /// Get a host variable
    pub fn get_host_var(&self, host: &str, name: &str) -> Option<JsonValue> {
        self.host_data
            .get(host)
            .and_then(|hd| hd.get_var(name).cloned())
    }

    /// Set a magic variable
    pub fn set_magic_var(&mut self, name: String, value: JsonValue) {
        self.magic_vars.insert(name, value);
    }

    /// Check if a host exists in the inventory
    pub fn has_host(&self, host: &str) -> bool {
        self.all_hosts.contains(&host.to_string())
    }

    /// Check if a group exists
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.contains_key(group)
    }

    /// Get all group names
    pub fn get_all_groups(&self) -> Vec<String> {
        self.groups.keys().cloned().collect()
    }

    // =========================================================================
    // Variable Precedence Methods (following Ansible precedence order)
    // =========================================================================

    /// Set a role default variable (lowest precedence after command line values)
    pub fn set_role_default(&mut self, name: String, value: JsonValue) {
        trace!("Setting role default: {} = {:?}", name, value);
        self.role_defaults.insert(name, value);
    }

    /// Set a block-level variable
    pub fn set_block_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting block var: {} = {:?}", name, value);
        self.block_vars.insert(name, value);
    }

    /// Clear block-level variables (called when exiting a block)
    pub fn clear_block_vars(&mut self) {
        self.block_vars.clear();
    }

    /// Set an include_vars variable
    pub fn set_include_var(&mut self, name: String, value: JsonValue) {
        trace!("Setting include var: {} = {:?}", name, value);
        self.include_vars.insert(name, value);
    }

    /// Set role params (when using include_role with parameters)
    pub fn set_role_param(&mut self, name: String, value: JsonValue) {
        trace!("Setting role param: {} = {:?}", name, value);
        self.role_params.insert(name, value);
    }

    /// Clear role params (after role execution)
    pub fn clear_role_params(&mut self) {
        self.role_params.clear();
    }

    /// Set include params
    pub fn set_include_param(&mut self, name: String, value: JsonValue) {
        trace!("Setting include param: {} = {:?}", name, value);
        self.include_params.insert(name, value);
    }

    /// Clear include params
    pub fn clear_include_params(&mut self) {
        self.include_params.clear();
    }

    // =========================================================================
    // Special Variables: hostvars, groups, inventory_hostname
    // =========================================================================

    /// Get hostvars for accessing variables from other hosts
    /// Usage: hostvars["other_host"]["some_var"]
    pub fn get_hostvars(&self) -> IndexMap<String, IndexMap<String, JsonValue>> {
        let mut hostvars = IndexMap::new();

        for host_name in &self.all_hosts {
            hostvars.insert(host_name.clone(), self.get_merged_vars(host_name));
        }

        hostvars
    }

    /// Get hostvars for a specific host
    pub fn get_hostvars_for_host(&self, host: &str) -> Option<IndexMap<String, JsonValue>> {
        if self.all_hosts.contains(&host.to_string()) {
            Some(self.get_merged_vars(host))
        } else {
            None
        }
    }

    /// Get groups dictionary mapping group names to hosts
    pub fn get_groups_dict(&self) -> IndexMap<String, Vec<String>> {
        let mut groups_dict = IndexMap::new();

        for (group_name, group) in &self.groups {
            groups_dict.insert(group_name.clone(), group.hosts.clone());
        }

        // Ensure 'all' group contains all hosts
        groups_dict.insert("all".to_string(), self.all_hosts.clone());

        groups_dict
    }

    /// Get group_names for a specific host (list of groups the host belongs to)
    pub fn get_group_names_for_host(&self, host: &str) -> Vec<String> {
        let mut group_names = Vec::new();

        for (group_name, group) in &self.groups {
            if group.hosts.contains(&host.to_string()) {
                group_names.push(group_name.clone());
            }
        }

        // Always include 'all'
        if !group_names.contains(&"all".to_string()) {
            group_names.push("all".to_string());
        }

        group_names
    }

    /// Get inventory_hostname (the current host being targeted)
    /// This is set per-task context
    pub fn get_inventory_hostname(&self, current_host: &str) -> String {
        current_host.to_string()
    }

    /// Get inventory_hostname_short (hostname without domain)
    pub fn get_inventory_hostname_short(&self, current_host: &str) -> String {
        current_host
            .split('.')
            .next()
            .unwrap_or(current_host)
            .to_string()
    }

    /// Get ansible_play_hosts (all hosts in current play)
    pub fn get_play_hosts(&self) -> Vec<String> {
        self.all_hosts.clone()
    }

    /// Get ansible_play_batch (current batch when using serial)
    pub fn get_play_batch(&self) -> Vec<String> {
        // For now, return all hosts. This would be updated during serial execution
        self.all_hosts.clone()
    }

    /// Set a group variable
    pub fn set_group_var(&mut self, group: &str, name: String, value: JsonValue) {
        let group_data = self
            .groups
            .entry(group.to_string())
            .or_insert_with(InventoryGroup::default);
        group_data.vars.insert(name, value);
    }

    /// Get a group variable
    pub fn get_group_var(&self, group: &str, name: &str) -> Option<&JsonValue> {
        self.groups.get(group).and_then(|g| g.vars.get(name))
    }

    /// Get all group variables for a group
    pub fn get_all_group_vars(&self, group: &str) -> Option<&IndexMap<String, JsonValue>> {
        self.groups.get(group).map(|g| &g.vars)
    }

    // =========================================================================
    // Variable Resolution with Full Precedence
    // =========================================================================

    /// Get a variable following full Ansible precedence order
    /// Precedence (lowest to highest):
    /// 1. Role defaults
    /// 2. Inventory group vars
    /// 3. Inventory host vars
    /// 4. Playbook vars (global_vars)
    /// 5. Play vars (play_vars)
    /// 6. Block vars
    /// 7. Task vars
    /// 8. Include vars
    /// 9. Set facts / registered vars
    /// 10. Role params
    /// 11. Include params
    /// 12. Extra vars (highest precedence)
    pub fn get_var_with_full_precedence(
        &self,
        name: &str,
        host: Option<&str>,
    ) -> Option<JsonValue> {
        // Check in order of precedence (highest first)

        // 12. Extra vars (highest)
        if let Some(v) = self.extra_vars.get(name) {
            return Some(v.clone());
        }

        // 11. Include params
        if let Some(v) = self.include_params.get(name) {
            return Some(v.clone());
        }

        // 10. Role params
        if let Some(v) = self.role_params.get(name) {
            return Some(v.clone());
        }

        // 9. Registered variables and set_fact (check host data)
        if let Some(host_name) = host {
            if let Some(host_data) = self.host_data.get(host_name) {
                if let Some(reg) = host_data.get_registered(name) {
                    return Some(reg.to_json());
                }
            }
        }

        // 8. Include vars
        if let Some(v) = self.include_vars.get(name) {
            return Some(v.clone());
        }

        // 7. Task variables
        if let Some(v) = self.task_vars.get(name) {
            return Some(v.clone());
        }

        // 6. Block vars
        if let Some(v) = self.block_vars.get(name) {
            return Some(v.clone());
        }

        // 5. Play variables
        if let Some(v) = self.play_vars.get(name) {
            return Some(v.clone());
        }

        // 4. Global (playbook) variables
        if let Some(v) = self.global_vars.get(name) {
            return Some(v.clone());
        }

        // 3. Host variables from inventory
        if let Some(host_name) = host {
            if let Some(host_data) = self.host_data.get(host_name) {
                if let Some(v) = host_data.get_var(name) {
                    return Some(v.clone());
                }
            }
        }

        // 2. Group variables (in order of group hierarchy, more specific last)
        if let Some(host_name) = host {
            // Get groups for this host and check their vars
            for (_group_name, group) in &self.groups {
                if group.hosts.contains(&host_name.to_string()) {
                    if let Some(v) = group.vars.get(name) {
                        return Some(v.clone());
                    }
                }
            }
        }

        // 1. Role defaults (lowest)
        if let Some(v) = self.role_defaults.get(name) {
            return Some(v.clone());
        }

        // Magic variables (built-in)
        if let Some(v) = self.magic_vars.get(name) {
            return Some(v.clone());
        }

        None
    }

    /// Build complete context for template rendering
    /// Returns all variables merged according to precedence with special vars
    pub fn build_template_context(&self, host: &str) -> IndexMap<String, JsonValue> {
        let mut context = self.get_merged_vars(host);

        // Add special variables
        context.insert(
            "inventory_hostname".to_string(),
            JsonValue::String(host.to_string()),
        );
        context.insert(
            "inventory_hostname_short".to_string(),
            JsonValue::String(self.get_inventory_hostname_short(host)),
        );

        // Add group_names
        let group_names = self.get_group_names_for_host(host);
        context.insert(
            "group_names".to_string(),
            serde_json::to_value(&group_names).unwrap_or(JsonValue::Array(vec![])),
        );

        // Add groups dict
        let groups_dict = self.get_groups_dict();
        context.insert(
            "groups".to_string(),
            serde_json::to_value(&groups_dict).unwrap_or(JsonValue::Object(serde_json::Map::new())),
        );

        // Add hostvars (lazy - would be accessed via hostvars[hostname])
        // For template context, we include it as a nested structure
        let hostvars = self.get_hostvars();
        context.insert(
            "hostvars".to_string(),
            serde_json::to_value(&hostvars).unwrap_or(JsonValue::Object(serde_json::Map::new())),
        );

        // Add play_hosts
        context.insert(
            "ansible_play_hosts".to_string(),
            serde_json::to_value(&self.all_hosts).unwrap_or(JsonValue::Array(vec![])),
        );
        context.insert(
            "ansible_play_hosts_all".to_string(),
            serde_json::to_value(&self.all_hosts).unwrap_or(JsonValue::Array(vec![])),
        );

        // Add ansible_host (connection address)
        // If set as host var, use it; otherwise use inventory_hostname
        if !context.contains_key("ansible_host") {
            context.insert(
                "ansible_host".to_string(),
                JsonValue::String(host.to_string()),
            );
        }

        context
    }
}

/// Thread-safe wrapper for RuntimeContext
pub struct SharedRuntime {
    inner: Arc<RwLock<RuntimeContext>>,
}

impl SharedRuntime {
    pub fn new(ctx: RuntimeContext) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ctx)),
        }
    }

    pub fn inner(&self) -> Arc<RwLock<RuntimeContext>> {
        Arc::clone(&self.inner)
    }

    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, RuntimeContext> {
        self.inner.read().await
    }

    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, RuntimeContext> {
        self.inner.write().await
    }
}

impl Clone for SharedRuntime {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_precedence() {
        let mut ctx = RuntimeContext::new();

        // Set variables at different levels
        ctx.set_global_var("var1".to_string(), serde_json::json!("global"));
        ctx.set_play_var("var1".to_string(), serde_json::json!("play"));

        // Play should override global
        assert_eq!(ctx.get_var("var1", None), Some(serde_json::json!("play")));

        // Task should override play
        ctx.set_task_var("var1".to_string(), serde_json::json!("task"));
        assert_eq!(ctx.get_var("var1", None), Some(serde_json::json!("task")));

        // Extra should override all
        ctx.set_extra_var("var1".to_string(), serde_json::json!("extra"));
        assert_eq!(ctx.get_var("var1", None), Some(serde_json::json!("extra")));
    }

    #[test]
    fn test_host_vars() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), Some("webservers"));

        ctx.set_host_var("server1", "http_port".to_string(), serde_json::json!(80));

        assert_eq!(
            ctx.get_host_var("server1", "http_port"),
            Some(serde_json::json!(80))
        );
    }

    #[test]
    fn test_host_facts() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), None);

        ctx.set_host_fact(
            "server1",
            "os_family".to_string(),
            serde_json::json!("Debian"),
        );

        assert_eq!(
            ctx.get_host_fact("server1", "os_family"),
            Some(serde_json::json!("Debian"))
        );
    }

    #[test]
    fn test_registered_result() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), None);

        let result = RegisteredResult {
            changed: true,
            stdout: Some("hello world".to_string()),
            stdout_lines: Some(vec!["hello world".to_string()]),
            ..Default::default()
        };

        ctx.register_result("server1", "my_result".to_string(), result);

        let registered = ctx.get_registered("server1", "my_result").unwrap();
        assert!(registered.changed);
        assert_eq!(registered.stdout, Some("hello world".to_string()));
    }

    #[test]
    fn test_group_hosts() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("web1".to_string(), Some("webservers"));
        ctx.add_host("web2".to_string(), Some("webservers"));
        ctx.add_host("db1".to_string(), Some("databases"));

        let web_hosts = ctx.get_group_hosts("webservers").unwrap();
        assert_eq!(web_hosts.len(), 2);
        assert!(web_hosts.contains(&"web1".to_string()));
        assert!(web_hosts.contains(&"web2".to_string()));
    }

    #[test]
    fn test_merged_vars() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), Some("webservers"));

        ctx.set_global_var("env".to_string(), serde_json::json!("production"));
        ctx.set_host_var("server1", "port".to_string(), serde_json::json!(8080));

        let merged = ctx.get_merged_vars("server1");

        assert_eq!(merged.get("env"), Some(&serde_json::json!("production")));
        assert_eq!(merged.get("port"), Some(&serde_json::json!(8080)));
        assert_eq!(
            merged.get("inventory_hostname"),
            Some(&serde_json::json!("server1"))
        );
    }

    #[test]
    fn test_hostvars_access() {
        let mut ctx = RuntimeContext::new();

        // Setup hosts
        ctx.add_host("web1".to_string(), Some("webservers"));
        ctx.add_host("web2".to_string(), Some("webservers"));
        ctx.add_host("db1".to_string(), Some("databases"));

        // Set some host-specific variables
        ctx.set_host_var("web1", "http_port".to_string(), serde_json::json!(80));
        ctx.set_host_var("web2", "http_port".to_string(), serde_json::json!(8080));
        ctx.set_host_var("db1", "db_port".to_string(), serde_json::json!(5432));

        // Test hostvars access
        let hostvars = ctx.get_hostvars();

        assert!(hostvars.contains_key("web1"));
        assert!(hostvars.contains_key("web2"));
        assert!(hostvars.contains_key("db1"));

        // Check specific host vars
        let web1_vars = hostvars.get("web1").unwrap();
        assert_eq!(web1_vars.get("http_port"), Some(&serde_json::json!(80)));

        let db1_vars = hostvars.get("db1").unwrap();
        assert_eq!(db1_vars.get("db_port"), Some(&serde_json::json!(5432)));
    }

    #[test]
    fn test_hostvars_for_specific_host() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("server1".to_string(), Some("webservers"));
        ctx.set_host_var(
            "server1",
            "custom_var".to_string(),
            serde_json::json!("value1"),
        );

        let hostvars = ctx.get_hostvars_for_host("server1");
        assert!(hostvars.is_some());
        let vars = hostvars.unwrap();
        assert_eq!(vars.get("custom_var"), Some(&serde_json::json!("value1")));

        // Non-existent host
        let missing = ctx.get_hostvars_for_host("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_groups_dict() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("web1".to_string(), Some("webservers"));
        ctx.add_host("web2".to_string(), Some("webservers"));
        ctx.add_host("db1".to_string(), Some("databases"));

        let groups = ctx.get_groups_dict();

        // Check webservers group
        assert!(groups.contains_key("webservers"));
        let web_hosts = groups.get("webservers").unwrap();
        assert!(web_hosts.contains(&"web1".to_string()));
        assert!(web_hosts.contains(&"web2".to_string()));

        // Check databases group
        assert!(groups.contains_key("databases"));
        let db_hosts = groups.get("databases").unwrap();
        assert!(db_hosts.contains(&"db1".to_string()));

        // Check all group
        assert!(groups.contains_key("all"));
        let all_hosts = groups.get("all").unwrap();
        assert_eq!(all_hosts.len(), 3);
    }

    #[test]
    fn test_group_names_for_host() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("web1".to_string(), Some("webservers"));

        let group_names = ctx.get_group_names_for_host("web1");

        assert!(group_names.contains(&"webservers".to_string()));
        assert!(group_names.contains(&"all".to_string()));
    }

    #[test]
    fn test_inventory_hostname_short() {
        let ctx = RuntimeContext::new();

        // Test FQDN
        assert_eq!(
            ctx.get_inventory_hostname_short("server1.example.com"),
            "server1"
        );

        // Test simple hostname
        assert_eq!(ctx.get_inventory_hostname_short("server1"), "server1");

        // Test IP address (should return the whole thing)
        assert_eq!(ctx.get_inventory_hostname_short("192.168.1.1"), "192");
    }

    #[test]
    fn test_role_defaults_precedence() {
        let mut ctx = RuntimeContext::new();

        // Role defaults are lowest precedence
        ctx.set_role_default("my_var".to_string(), serde_json::json!("default_value"));
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("default_value"))
        );

        // Play vars should override
        ctx.set_play_var("my_var".to_string(), serde_json::json!("play_value"));
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("play_value"))
        );

        // Extra vars should override all
        ctx.set_extra_var("my_var".to_string(), serde_json::json!("extra_value"));
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("extra_value"))
        );
    }

    #[test]
    fn test_block_vars_scope() {
        let mut ctx = RuntimeContext::new();

        ctx.set_block_var("block_var".to_string(), serde_json::json!("block_value"));
        assert_eq!(
            ctx.get_var_with_full_precedence("block_var", None),
            Some(serde_json::json!("block_value"))
        );

        // Clear block vars (simulating exiting a block)
        ctx.clear_block_vars();
        assert_eq!(ctx.get_var_with_full_precedence("block_var", None), None);
    }

    #[test]
    fn test_include_vars_precedence() {
        let mut ctx = RuntimeContext::new();

        ctx.set_task_var("my_var".to_string(), serde_json::json!("task_value"));
        ctx.set_include_var("my_var".to_string(), serde_json::json!("include_value"));

        // Include vars should override task vars
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("include_value"))
        );
    }

    #[test]
    fn test_role_params_precedence() {
        let mut ctx = RuntimeContext::new();

        ctx.set_include_var("my_var".to_string(), serde_json::json!("include_value"));
        ctx.set_role_param("my_var".to_string(), serde_json::json!("role_param_value"));

        // Role params should override include vars
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("role_param_value"))
        );

        // Clear role params
        ctx.clear_role_params();
        assert_eq!(
            ctx.get_var_with_full_precedence("my_var", None),
            Some(serde_json::json!("include_value"))
        );
    }

    #[test]
    fn test_group_vars() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("web1".to_string(), Some("webservers"));
        ctx.set_group_var("webservers", "http_port".to_string(), serde_json::json!(80));

        // Check group var is accessible
        let group_var = ctx.get_group_var("webservers", "http_port");
        assert_eq!(group_var, Some(&serde_json::json!(80)));

        // Check through precedence chain
        let var = ctx.get_var_with_full_precedence("http_port", Some("web1"));
        assert_eq!(var, Some(serde_json::json!(80)));
    }

    #[test]
    fn test_build_template_context() {
        let mut ctx = RuntimeContext::new();

        ctx.add_host("web1".to_string(), Some("webservers"));
        ctx.add_host("web2".to_string(), Some("webservers"));
        ctx.set_host_var("web1", "custom".to_string(), serde_json::json!("value"));

        let context = ctx.build_template_context("web1");

        // Check special variables are present
        assert_eq!(
            context.get("inventory_hostname"),
            Some(&serde_json::json!("web1"))
        );
        assert!(context.contains_key("group_names"));
        assert!(context.contains_key("groups"));
        assert!(context.contains_key("hostvars"));
        assert!(context.contains_key("ansible_play_hosts"));
        assert_eq!(context.get("custom"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_facts_exposed_as_ansible_variables() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), None);

        // Set facts without ansible_ prefix
        ctx.set_host_fact(
            "server1",
            "hostname".to_string(),
            serde_json::json!("myserver"),
        );
        ctx.set_host_fact(
            "server1",
            "distribution".to_string(),
            serde_json::json!("Ubuntu"),
        );
        ctx.set_host_fact(
            "server1",
            "os_family".to_string(),
            serde_json::json!("Debian"),
        );

        // Set a fact that already has ansible_ prefix
        ctx.set_host_fact(
            "server1",
            "ansible_python_interpreter".to_string(),
            serde_json::json!("/usr/bin/python3"),
        );

        let merged = ctx.get_merged_vars("server1");

        // Facts should be accessible as top-level ansible_* variables
        assert_eq!(
            merged.get("ansible_hostname"),
            Some(&serde_json::json!("myserver"))
        );
        assert_eq!(
            merged.get("ansible_distribution"),
            Some(&serde_json::json!("Ubuntu"))
        );
        assert_eq!(
            merged.get("ansible_os_family"),
            Some(&serde_json::json!("Debian"))
        );

        // Facts that already have ansible_ prefix should not be double-prefixed
        assert_eq!(
            merged.get("ansible_python_interpreter"),
            Some(&serde_json::json!("/usr/bin/python3"))
        );
        assert!(merged.get("ansible_ansible_python_interpreter").is_none());

        // ansible_facts should still contain the nested structure for backwards compatibility
        let ansible_facts = merged.get("ansible_facts").unwrap();
        assert_eq!(
            ansible_facts.get("hostname"),
            Some(&serde_json::json!("myserver"))
        );
        assert_eq!(
            ansible_facts.get("distribution"),
            Some(&serde_json::json!("Ubuntu"))
        );
    }

    #[test]
    fn test_full_precedence_chain() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("server1".to_string(), Some("webservers"));

        // Set variable at every level
        ctx.set_role_default("test_var".to_string(), serde_json::json!("role_default"));
        ctx.set_group_var(
            "webservers",
            "test_var".to_string(),
            serde_json::json!("group_var"),
        );
        ctx.set_host_var(
            "server1",
            "test_var".to_string(),
            serde_json::json!("host_var"),
        );
        ctx.set_global_var("test_var".to_string(), serde_json::json!("global_var"));
        ctx.set_play_var("test_var".to_string(), serde_json::json!("play_var"));
        ctx.set_block_var("test_var".to_string(), serde_json::json!("block_var"));
        ctx.set_task_var("test_var".to_string(), serde_json::json!("task_var"));
        ctx.set_include_var("test_var".to_string(), serde_json::json!("include_var"));
        ctx.set_role_param("test_var".to_string(), serde_json::json!("role_param"));
        ctx.set_include_param("test_var".to_string(), serde_json::json!("include_param"));
        ctx.set_extra_var("test_var".to_string(), serde_json::json!("extra_var"));

        // Extra vars should win
        assert_eq!(
            ctx.get_var_with_full_precedence("test_var", Some("server1")),
            Some(serde_json::json!("extra_var"))
        );

        // Remove extra_var, include_param should win
        ctx.extra_vars.remove("test_var");
        assert_eq!(
            ctx.get_var_with_full_precedence("test_var", Some("server1")),
            Some(serde_json::json!("include_param"))
        );

        // Remove include_param, role_param should win
        ctx.include_params.remove("test_var");
        assert_eq!(
            ctx.get_var_with_full_precedence("test_var", Some("server1")),
            Some(serde_json::json!("role_param"))
        );
    }
}
