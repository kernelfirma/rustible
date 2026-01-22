//! Lexical scoping system for Rustible variables.
//!
//! This module provides proper variable scoping with:
//! - Explicit scope declarations (global, play, task, role)
//! - Variable isolation for roles
//! - Scope visualization for debugging
//! - Backward compatibility with Ansible scoping
//!
//! # Scope Levels
//!
//! Variables exist in a hierarchy of scopes:
//!
//! ```text
//! Global Scope (inventory, extra_vars)
//!   └── Playbook Scope (playbook vars_files)
//!         └── Play Scope (play vars)
//!               └── Role Scope (role defaults, vars)
//!                     └── Block Scope (block vars)
//!                           └── Task Scope (task vars, loop vars)
//! ```
//!
//! Each scope inherits from its parent but can shadow variables.
//! Role scopes are isolated by default - they don't leak variables.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{VarPrecedence, VarStore, Variable};

/// Counter for generating unique scope IDs
static SCOPE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a new unique scope ID
fn next_scope_id() -> u64 {
    SCOPE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Scope level in the execution hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScopeLevel {
    /// Global scope (inventory, extra_vars) - accessible everywhere
    Global,
    /// Playbook scope - vars defined at playbook level
    Playbook,
    /// Play scope - vars defined in a play
    Play,
    /// Role scope - isolated scope for role execution
    Role,
    /// Block scope - vars defined in a block
    Block,
    /// Task scope - vars for a single task execution
    Task,
    /// Loop scope - loop iteration variables (item, loop.index, etc.)
    Loop,
}

impl ScopeLevel {
    /// Get the priority level (lower is more global)
    pub fn priority(&self) -> u8 {
        match self {
            ScopeLevel::Global => 0,
            ScopeLevel::Playbook => 1,
            ScopeLevel::Play => 2,
            ScopeLevel::Role => 3,
            ScopeLevel::Block => 4,
            ScopeLevel::Task => 5,
            ScopeLevel::Loop => 6,
        }
    }

    /// Check if this scope should inherit from another
    pub fn inherits_from(&self, parent: &ScopeLevel) -> bool {
        self.priority() > parent.priority()
    }
}

impl fmt::Display for ScopeLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ScopeLevel::Global => "global",
            ScopeLevel::Playbook => "playbook",
            ScopeLevel::Play => "play",
            ScopeLevel::Role => "role",
            ScopeLevel::Block => "block",
            ScopeLevel::Task => "task",
            ScopeLevel::Loop => "loop",
        };
        write!(f, "{}", name)
    }
}

/// Scope declaration for controlling variable visibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScopeDeclaration {
    /// Implicit scope - follows precedence rules
    #[default]
    Implicit,
    /// Explicit global scope - visible everywhere
    Global,
    /// Play scope - visible only in current play
    PlayLocal,
    /// Task scope - visible only in current task
    TaskLocal,
    /// Role scope - visible only in current role (isolated)
    RoleLocal,
}

impl fmt::Display for ScopeDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ScopeDeclaration::Implicit => "implicit",
            ScopeDeclaration::Global => "global",
            ScopeDeclaration::PlayLocal => "play_local",
            ScopeDeclaration::TaskLocal => "task_local",
            ScopeDeclaration::RoleLocal => "role_local",
        };
        write!(f, "{}", name)
    }
}

/// A scoped variable with additional metadata for scoping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedVariable {
    /// The underlying variable
    pub variable: Variable,
    /// Scope declaration (explicit scope)
    pub scope_declaration: ScopeDeclaration,
    /// Scope ID where this variable was defined
    pub defined_in_scope: u64,
    /// Role name if defined in a role (for isolation)
    pub role_name: Option<String>,
    /// Whether this variable is exported from a role
    pub exported: bool,
}

impl ScopedVariable {
    /// Create a new scoped variable
    pub fn new(variable: Variable) -> Self {
        Self {
            variable,
            scope_declaration: ScopeDeclaration::Implicit,
            defined_in_scope: 0,
            role_name: None,
            exported: false,
        }
    }

    /// Create with explicit scope declaration
    pub fn with_declaration(variable: Variable, declaration: ScopeDeclaration) -> Self {
        Self {
            variable,
            scope_declaration: declaration,
            defined_in_scope: 0,
            role_name: None,
            exported: false,
        }
    }

    /// Set the scope ID where this variable was defined
    pub fn in_scope(mut self, scope_id: u64) -> Self {
        self.defined_in_scope = scope_id;
        self
    }

    /// Set the role name for role isolation
    pub fn in_role(mut self, role_name: impl Into<String>) -> Self {
        self.role_name = Some(role_name.into());
        self
    }

    /// Mark this variable as exported from a role
    pub fn exported(mut self) -> Self {
        self.exported = true;
        self
    }
}

/// Configuration for a scope
#[derive(Debug, Clone)]
pub struct ScopeConfig {
    /// Scope level
    pub level: ScopeLevel,
    /// Optional name for the scope (e.g., role name, play name)
    pub name: Option<String>,
    /// Whether this scope isolates variables (roles do by default)
    pub isolated: bool,
    /// Parent scope ID (None for global)
    pub parent_id: Option<u64>,
}

impl ScopeConfig {
    /// Create a new scope config
    pub fn new(level: ScopeLevel) -> Self {
        Self {
            level,
            name: None,
            isolated: matches!(level, ScopeLevel::Role),
            parent_id: None,
        }
    }

    /// Set the scope name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the parent scope ID
    pub fn with_parent(mut self, parent_id: u64) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Set isolation mode
    pub fn isolated(mut self, is_isolated: bool) -> Self {
        self.isolated = is_isolated;
        self
    }
}

/// A lexical scope for variable management
#[derive(Debug, Clone)]
pub struct LexicalScope {
    /// Unique ID for this scope
    pub id: u64,
    /// Scope configuration
    pub config: ScopeConfig,
    /// Variables defined in this scope
    variables: IndexMap<String, ScopedVariable>,
    /// Child scope IDs
    children: Vec<u64>,
    /// Creation timestamp (for debugging)
    created_at: std::time::Instant,
}

impl LexicalScope {
    /// Create a new lexical scope
    pub fn new(config: ScopeConfig) -> Self {
        Self {
            id: next_scope_id(),
            config,
            variables: IndexMap::new(),
            children: Vec::new(),
            created_at: std::time::Instant::now(),
        }
    }

    /// Set a variable in this scope
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
    ) {
        let key = key.into();
        let variable = Variable::new(value, precedence);
        let scoped = ScopedVariable::new(variable).in_scope(self.id);
        self.variables.insert(key, scoped);
    }

    /// Set a variable with explicit scope declaration
    pub fn set_with_declaration(
        &mut self,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
        declaration: ScopeDeclaration,
    ) {
        let key = key.into();
        let variable = Variable::new(value, precedence);
        let scoped = ScopedVariable::with_declaration(variable, declaration).in_scope(self.id);
        self.variables.insert(key, scoped);
    }

    /// Set a scoped variable
    pub fn set_scoped(&mut self, key: impl Into<String>, scoped_var: ScopedVariable) {
        self.variables.insert(key.into(), scoped_var);
    }

    /// Get a variable from this scope only (no parent lookup)
    pub fn get_local(&self, key: &str) -> Option<&ScopedVariable> {
        self.variables.get(key)
    }

    /// Check if a variable exists in this scope only
    pub fn contains_local(&self, key: &str) -> bool {
        self.variables.contains_key(key)
    }

    /// Remove a variable from this scope
    pub fn remove(&mut self, key: &str) -> Option<ScopedVariable> {
        self.variables.swap_remove(key)
    }

    /// Get all variables defined in this scope
    pub fn all_local(&self) -> &IndexMap<String, ScopedVariable> {
        &self.variables
    }

    /// Get variable count in this scope
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Check if this scope is empty
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    /// Add a child scope ID
    pub fn add_child(&mut self, child_id: u64) {
        self.children.push(child_id);
    }

    /// Get child scope IDs
    pub fn children(&self) -> &[u64] {
        &self.children
    }

    /// Get scope age for debugging
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }
}

/// A scoped variable store that maintains a hierarchy of scopes
#[derive(Debug)]
pub struct ScopedVarStore {
    /// All scopes indexed by ID
    scopes: HashMap<u64, LexicalScope>,
    /// Current scope ID (the active scope)
    current_scope_id: u64,
    /// Scope stack for entering/exiting scopes
    scope_stack: Vec<u64>,
    /// Underlying VarStore for precedence resolution
    var_store: VarStore,
    /// Role isolation - variables defined per role
    role_variables: HashMap<String, IndexMap<String, ScopedVariable>>,
}

impl Default for ScopedVarStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopedVarStore {
    /// Create a new scoped variable store with global scope
    pub fn new() -> Self {
        let mut store = Self {
            scopes: HashMap::new(),
            current_scope_id: 0,
            scope_stack: Vec::new(),
            var_store: VarStore::new(),
            role_variables: HashMap::new(),
        };

        // Create the global scope
        let global_scope = LexicalScope::new(ScopeConfig::new(ScopeLevel::Global));
        let global_id = global_scope.id;
        store.scopes.insert(global_id, global_scope);
        store.current_scope_id = global_id;
        store.scope_stack.push(global_id);

        store
    }

    /// Create from an existing VarStore (for backward compatibility)
    pub fn from_var_store(var_store: VarStore) -> Self {
        let mut store = Self::new();
        store.var_store = var_store;
        store
    }

    /// Get the underlying VarStore (for backward compatibility)
    pub fn var_store(&self) -> &VarStore {
        &self.var_store
    }

    /// Get mutable access to the underlying VarStore
    pub fn var_store_mut(&mut self) -> &mut VarStore {
        &mut self.var_store
    }

    /// Get the current scope
    pub fn current_scope(&self) -> &LexicalScope {
        self.scopes
            .get(&self.current_scope_id)
            .expect("Current scope must exist")
    }

    /// Get mutable access to the current scope
    fn current_scope_mut(&mut self) -> &mut LexicalScope {
        self.scopes
            .get_mut(&self.current_scope_id)
            .expect("Current scope must exist")
    }

    /// Enter a new scope
    pub fn enter_scope(&mut self, config: ScopeConfig) -> u64 {
        let parent_id = self.current_scope_id;
        let scope = LexicalScope::new(config.with_parent(parent_id));
        let scope_id = scope.id;

        // Update parent's children
        if let Some(parent) = self.scopes.get_mut(&parent_id) {
            parent.add_child(scope_id);
        }

        self.scopes.insert(scope_id, scope);
        self.scope_stack.push(scope_id);
        self.current_scope_id = scope_id;

        scope_id
    }

    /// Exit the current scope and return to parent
    pub fn exit_scope(&mut self) -> Option<u64> {
        if self.scope_stack.len() <= 1 {
            // Can't exit global scope
            return None;
        }

        let exited_scope_id = self.scope_stack.pop()?;
        self.current_scope_id = *self.scope_stack.last().unwrap_or(&1);
        Some(exited_scope_id)
    }

    /// Enter a play scope
    pub fn enter_play(&mut self, name: impl Into<String>) -> u64 {
        self.enter_scope(ScopeConfig::new(ScopeLevel::Play).with_name(name))
    }

    /// Enter a role scope (isolated by default)
    pub fn enter_role(&mut self, role_name: impl Into<String>) -> u64 {
        let name = role_name.into();
        self.enter_scope(
            ScopeConfig::new(ScopeLevel::Role)
                .with_name(name)
                .isolated(true),
        )
    }

    /// Enter a block scope
    pub fn enter_block(&mut self, name: Option<String>) -> u64 {
        let mut config = ScopeConfig::new(ScopeLevel::Block);
        if let Some(n) = name {
            config = config.with_name(n);
        }
        self.enter_scope(config)
    }

    /// Enter a task scope
    pub fn enter_task(&mut self, name: impl Into<String>) -> u64 {
        self.enter_scope(ScopeConfig::new(ScopeLevel::Task).with_name(name))
    }

    /// Enter a loop scope
    pub fn enter_loop(&mut self) -> u64 {
        self.enter_scope(ScopeConfig::new(ScopeLevel::Loop))
    }

    /// Set a variable in the current scope
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
    ) {
        let key = key.into();

        // Also set in the underlying VarStore for precedence resolution
        self.var_store.set(&key, value.clone(), precedence);

        // Set in current scope
        self.current_scope_mut().set(&key, value, precedence);
    }

    /// Set a variable with explicit scope declaration
    pub fn set_with_scope(
        &mut self,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
        declaration: ScopeDeclaration,
    ) {
        let key = key.into();

        match declaration {
            ScopeDeclaration::Global => {
                // Set in global scope and VarStore
                self.var_store.set(&key, value.clone(), precedence);
                if let Some(global) = self.scopes.get_mut(&1) {
                    global.set_with_declaration(&key, value, precedence, declaration);
                }
            }
            ScopeDeclaration::TaskLocal
            | ScopeDeclaration::RoleLocal
            | ScopeDeclaration::PlayLocal => {
                // Only set in current scope, not in VarStore
                self.current_scope_mut()
                    .set_with_declaration(&key, value, precedence, declaration);
            }
            ScopeDeclaration::Implicit => {
                // Default behavior
                self.set(&key, value, precedence);
            }
        }
    }

    /// Set a variable for a specific role (for role isolation)
    pub fn set_role_var(
        &mut self,
        role_name: impl Into<String>,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
    ) {
        let role_name = role_name.into();
        let key = key.into();

        let variable = Variable::new(value, precedence);
        let scoped = ScopedVariable::new(variable).in_role(&role_name);

        self.role_variables
            .entry(role_name)
            .or_default()
            .insert(key, scoped);
    }

    /// Export a role variable to parent scope
    pub fn export_role_var(&mut self, role_name: &str, key: &str) -> bool {
        if let Some(role_vars) = self.role_variables.get_mut(role_name) {
            if let Some(var) = role_vars.get_mut(key) {
                var.exported = true;
                // Copy to VarStore for global access
                self.var_store
                    .set(key, var.variable.value.clone(), var.variable.precedence);
                return true;
            }
        }
        false
    }

    /// Get a variable (respecting scope hierarchy and isolation)
    pub fn get(&mut self, key: &str) -> Option<&serde_yaml::Value> {
        // First check current scope and walk up the scope chain
        let mut scope_id = Some(self.current_scope_id);

        while let Some(sid) = scope_id {
            if let Some(scope) = self.scopes.get(&sid) {
                // Check if scope is isolated (like role scope)
                if scope.config.isolated {
                    // In isolated scope, only check local and exported variables
                    if let Some(var) = scope.get_local(key) {
                        return Some(&var.variable.value);
                    }
                    // Check if we're in a role and have role-specific variables
                    if let Some(ref role_name) = scope.config.name {
                        if let Some(role_vars) = self.role_variables.get(role_name) {
                            if let Some(var) = role_vars.get(key) {
                                return Some(&var.variable.value);
                            }
                        }
                    }
                    // For isolated scopes, only continue to parent if var not found locally
                } else {
                    // Non-isolated scope - check local first
                    if let Some(var) = scope.get_local(key) {
                        // Check scope declaration
                        match var.scope_declaration {
                            ScopeDeclaration::TaskLocal => {
                                // Only visible if we're in the same task
                                if sid == self.current_scope_id {
                                    return Some(&var.variable.value);
                                }
                            }
                            ScopeDeclaration::RoleLocal => {
                                // Only visible within the role
                                // (handled by isolation)
                                return Some(&var.variable.value);
                            }
                            _ => {
                                return Some(&var.variable.value);
                            }
                        }
                    }
                }

                scope_id = scope.config.parent_id;
            } else {
                break;
            }
        }

        // Fall back to VarStore for precedence-based resolution
        self.var_store.get(key)
    }

    /// Check if a variable exists in any accessible scope
    pub fn contains(&mut self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Get all variables visible from current scope
    pub fn all(&mut self) -> IndexMap<String, serde_yaml::Value> {
        // Start with VarStore's merged variables
        let mut result = self.var_store.all().clone();

        // Walk up scope chain and overlay variables
        let mut scope_ids: Vec<u64> = Vec::new();
        let mut scope_id = Some(self.current_scope_id);

        while let Some(sid) = scope_id {
            scope_ids.push(sid);
            if let Some(scope) = self.scopes.get(&sid) {
                scope_id = scope.config.parent_id;
            } else {
                break;
            }
        }

        // Reverse to apply from root to current (so current scope wins)
        scope_ids.reverse();

        for sid in scope_ids {
            if let Some(scope) = self.scopes.get(&sid) {
                for (key, var) in scope.all_local() {
                    // Respect scope declarations
                    match var.scope_declaration {
                        ScopeDeclaration::TaskLocal if sid != self.current_scope_id => continue,
                        ScopeDeclaration::RoleLocal if scope.config.level != ScopeLevel::Role => {
                            continue
                        }
                        _ => {}
                    }
                    result.insert(key.clone(), var.variable.value.clone());
                }
            }
        }

        result
    }

    /// Get the scope stack depth
    pub fn depth(&self) -> usize {
        self.scope_stack.len()
    }

    /// Get all scope IDs in the current stack
    pub fn scope_stack(&self) -> &[u64] {
        &self.scope_stack
    }
}

/// Scope visualization for debugging
#[derive(Debug)]
pub struct ScopeVisualization {
    lines: Vec<String>,
}

impl ScopeVisualization {
    /// Create a new visualization from a ScopedVarStore
    pub fn new(store: &ScopedVarStore) -> Self {
        let mut lines = Vec::new();
        Self::visualize_scope(store, store.scope_stack[0], 0, &mut lines);
        Self { lines }
    }

    fn visualize_scope(
        store: &ScopedVarStore,
        scope_id: u64,
        indent: usize,
        lines: &mut Vec<String>,
    ) {
        let prefix = "  ".repeat(indent);

        if let Some(scope) = store.scopes.get(&scope_id) {
            // Scope header
            let scope_name = scope.config.name.as_deref().unwrap_or("anonymous");
            let marker = if scope_id == store.current_scope_id {
                ">>> "
            } else {
                ""
            };
            let isolated = if scope.config.isolated {
                " [isolated]"
            } else {
                ""
            };

            lines.push(format!(
                "{}{}{} scope '{}' (id={}){}",
                prefix, marker, scope.config.level, scope_name, scope_id, isolated
            ));

            // Variables in this scope
            let var_prefix = format!("{}  ", prefix);
            for (key, var) in scope.all_local() {
                let scope_decl = match var.scope_declaration {
                    ScopeDeclaration::Implicit => "",
                    _ => &format!(" [{:?}]", var.scope_declaration),
                };
                let value_preview = Self::value_preview(&var.variable.value);
                lines.push(format!(
                    "{}- {}: {} ({}){}",
                    var_prefix, key, value_preview, var.variable.precedence, scope_decl
                ));
            }

            // Recurse into children
            for &child_id in &scope.children {
                Self::visualize_scope(store, child_id, indent + 1, lines);
            }
        }
    }

    fn value_preview(value: &serde_yaml::Value) -> String {
        match value {
            serde_yaml::Value::Null => "null".to_string(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::String(s) => {
                if s.len() > 30 {
                    format!("\"{}...\"", &s[..27])
                } else {
                    format!("\"{}\"", s)
                }
            }
            serde_yaml::Value::Sequence(seq) => format!("[{} items]", seq.len()),
            serde_yaml::Value::Mapping(map) => format!("{{}} {} keys}}", map.len()),
            serde_yaml::Value::Tagged(t) => format!("!{} ...", t.tag),
        }
    }
}

impl fmt::Display for ScopeVisualization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in &self.lines {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

/// Helper trait for scope visualization
pub trait ScopeVisualize {
    /// Generate a visual representation of the scope hierarchy
    fn visualize(&self) -> ScopeVisualization;
}

impl ScopeVisualize for ScopedVarStore {
    fn visualize(&self) -> ScopeVisualization {
        ScopeVisualization::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_levels() {
        assert!(ScopeLevel::Task.priority() > ScopeLevel::Play.priority());
        assert!(ScopeLevel::Global.priority() < ScopeLevel::Role.priority());
        assert!(ScopeLevel::Task.inherits_from(&ScopeLevel::Play));
        assert!(!ScopeLevel::Global.inherits_from(&ScopeLevel::Task));
    }

    #[test]
    fn test_basic_scoping() {
        let mut store = ScopedVarStore::new();

        // Set global variable
        store.set(
            "global_var",
            serde_yaml::Value::String("global".into()),
            VarPrecedence::ExtraVars,
        );

        // Enter play scope
        store.enter_play("my_play");
        store.set(
            "play_var",
            serde_yaml::Value::String("play".into()),
            VarPrecedence::PlayVars,
        );

        // Should see both
        assert_eq!(
            store.get("global_var").map(|v| v.as_str()),
            Some(Some("global"))
        );
        assert_eq!(
            store.get("play_var").map(|v| v.as_str()),
            Some(Some("play"))
        );

        // Exit play scope
        store.exit_scope();

        // Should still see global but not play
        assert!(store.contains("global_var"));
    }

    #[test]
    fn test_role_isolation() {
        let mut store = ScopedVarStore::new();

        // Set a play variable
        store.enter_play("my_play");
        store.set(
            "outer_var",
            serde_yaml::Value::String("outer".into()),
            VarPrecedence::PlayVars,
        );

        // Enter isolated role scope
        store.enter_role("my_role");
        store.set(
            "role_var",
            serde_yaml::Value::String("role".into()),
            VarPrecedence::RoleVars,
        );

        // Role can see its own var
        assert!(store.contains("role_var"));

        // Exit role
        store.exit_scope();

        // Play should NOT see role_var (isolation)
        assert!(!store.current_scope().contains_local("role_var"));
    }

    #[test]
    fn test_explicit_scope_declarations() {
        let mut store = ScopedVarStore::new();

        // Set a global variable explicitly
        store.set_with_scope(
            "explicit_global",
            serde_yaml::Value::String("global_val".into()),
            VarPrecedence::PlayVars,
            ScopeDeclaration::Global,
        );

        // Enter task scope and set task-local variable
        store.enter_task("my_task");
        store.set_with_scope(
            "task_local",
            serde_yaml::Value::String("task_val".into()),
            VarPrecedence::TaskVars,
            ScopeDeclaration::TaskLocal,
        );

        // Global should be visible
        assert!(store.contains("explicit_global"));

        // Task-local should be visible in task
        assert!(store.contains("task_local"));
    }

    #[test]
    fn test_role_variable_export() {
        let mut store = ScopedVarStore::new();

        // Set role variable
        store.set_role_var(
            "webserver",
            "nginx_port",
            serde_yaml::Value::Number(80.into()),
            VarPrecedence::RoleVars,
        );

        // Not visible by default in global scope
        // (would need to enter role scope or export)

        // Export it
        store.export_role_var("webserver", "nginx_port");

        // Now it should be in VarStore
        assert!(store.var_store_mut().contains("nginx_port"));
    }

    #[test]
    fn test_scope_visualization() {
        let mut store = ScopedVarStore::new();

        store.set(
            "global_var",
            serde_yaml::Value::String("val".into()),
            VarPrecedence::ExtraVars,
        );
        store.enter_play("test_play");
        store.set(
            "play_var",
            serde_yaml::Value::Number(42.into()),
            VarPrecedence::PlayVars,
        );
        store.enter_task("task1");

        let viz = store.visualize();
        let output = viz.to_string();

        assert!(output.contains("global scope"));
        assert!(output.contains("play scope"));
        assert!(output.contains("task scope"));
        assert!(output.contains("global_var"));
        assert!(output.contains("play_var"));
    }

    #[test]
    fn test_scope_stack() {
        let mut store = ScopedVarStore::new();

        assert_eq!(store.depth(), 1); // Global

        store.enter_play("play1");
        assert_eq!(store.depth(), 2);

        store.enter_role("role1");
        assert_eq!(store.depth(), 3);

        store.enter_task("task1");
        assert_eq!(store.depth(), 4);

        store.exit_scope();
        assert_eq!(store.depth(), 3);

        store.exit_scope();
        store.exit_scope();
        assert_eq!(store.depth(), 1);

        // Can't exit global
        assert!(store.exit_scope().is_none());
        assert_eq!(store.depth(), 1);
    }

    #[test]
    fn test_all_variables() {
        let mut store = ScopedVarStore::new();

        store.set(
            "a",
            serde_yaml::Value::Number(1.into()),
            VarPrecedence::ExtraVars,
        );
        store.enter_play("play");
        store.set(
            "b",
            serde_yaml::Value::Number(2.into()),
            VarPrecedence::PlayVars,
        );
        store.enter_task("task");
        store.set(
            "c",
            serde_yaml::Value::Number(3.into()),
            VarPrecedence::TaskVars,
        );

        let all = store.all();

        assert!(all.contains_key("a"));
        assert!(all.contains_key("b"));
        assert!(all.contains_key("c"));
    }
}
