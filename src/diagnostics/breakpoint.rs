//! Breakpoint support for debugging.
//!
//! This module provides breakpoint functionality for pausing execution
//! at specific points based on task names, hosts, conditions, or events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::BreakpointContext;

/// Global counter for breakpoint IDs
static BREAKPOINT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Type of breakpoint
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointType {
    /// Break on a specific task by name
    Task,
    /// Break on a specific host
    Host,
    /// Break on a specific module
    Module,
    /// Break on a specific play
    Play,
    /// Break at a specific task number
    TaskNumber,
    /// Break on any failure
    OnFailure,
    /// Break on any change
    OnChange,
    /// Break on a conditional expression
    Conditional,
    /// Break on unreachable host
    OnUnreachable,
    /// Break at start of each play
    PlayStart,
    /// Break at end of each play
    PlayEnd,
    /// Temporary breakpoint (removed after first hit)
    Temporary,
}

/// Condition for conditional breakpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BreakpointCondition {
    /// Variable equals a value
    VarEquals { name: String, value: JsonValue },
    /// Variable is defined
    VarDefined(String),
    /// Variable is undefined
    VarUndefined(String),
    /// Custom expression (Jinja2-like)
    Expression(String),
    /// Combined conditions (all must match)
    All(Vec<BreakpointCondition>),
    /// Combined conditions (any must match)
    Any(Vec<BreakpointCondition>),
    /// Negated condition
    Not(Box<BreakpointCondition>),
    /// Always true
    Always,
}

impl BreakpointCondition {
    /// Evaluate the condition against the current context
    pub fn evaluate(&self, context: &BreakpointContext) -> bool {
        match self {
            BreakpointCondition::VarEquals { name, value } => context
                .variables
                .get(name)
                .map(|v| v == value)
                .unwrap_or(false),
            BreakpointCondition::VarDefined(name) => context.variables.contains_key(name),
            BreakpointCondition::VarUndefined(name) => !context.variables.contains_key(name),
            BreakpointCondition::Expression(expr) => {
                // Simple expression evaluation
                // For complex expressions, would need full template engine
                Self::evaluate_simple_expression(expr, &context.variables)
            }
            BreakpointCondition::All(conditions) => conditions.iter().all(|c| c.evaluate(context)),
            BreakpointCondition::Any(conditions) => conditions.iter().any(|c| c.evaluate(context)),
            BreakpointCondition::Not(condition) => !condition.evaluate(context),
            BreakpointCondition::Always => true,
        }
    }

    /// Evaluate a simple expression
    fn evaluate_simple_expression(expr: &str, vars: &HashMap<String, JsonValue>) -> bool {
        let expr = expr.trim();

        // Handle simple variable checks
        if let Some(var_name) = expr
            .strip_prefix("defined(")
            .and_then(|s| s.strip_suffix(')'))
        {
            return vars.contains_key(var_name.trim());
        }

        // Handle equality: var == value
        if let Some((left, right)) = expr.split_once("==") {
            let var_name = left.trim();
            let expected = right.trim().trim_matches(|c| c == '"' || c == '\'');
            return vars
                .get(var_name)
                .and_then(|v| v.as_str())
                .map(|s| s == expected)
                .unwrap_or(false);
        }

        // Handle inequality: var != value
        if let Some((left, right)) = expr.split_once("!=") {
            let var_name = left.trim();
            let expected = right.trim().trim_matches(|c| c == '"' || c == '\'');
            return vars
                .get(var_name)
                .and_then(|v| v.as_str())
                .map(|s| s != expected)
                .unwrap_or(true);
        }

        // Handle boolean variable
        vars.get(expr).and_then(|v| v.as_bool()).unwrap_or(false)
    }
}

/// A breakpoint definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Unique identifier
    pub id: String,
    /// Type of breakpoint
    pub breakpoint_type: BreakpointType,
    /// Target pattern (task name, host, module, etc.)
    pub pattern: Option<String>,
    /// Task number (for TaskNumber type)
    pub task_number: Option<usize>,
    /// Condition for conditional breakpoints
    pub condition: Option<BreakpointCondition>,
    /// Whether the breakpoint is enabled
    pub enabled: bool,
    /// Number of times this breakpoint has been hit
    pub hit_count: usize,
    /// Description for the user
    pub description: Option<String>,
    /// Whether this is a one-shot breakpoint
    pub temporary: bool,
    /// Ignore count (skip this many hits before breaking)
    pub ignore_count: usize,
}

impl Breakpoint {
    /// Create a new breakpoint
    fn new(breakpoint_type: BreakpointType) -> Self {
        let id = format!("bp_{}", BREAKPOINT_COUNTER.fetch_add(1, Ordering::SeqCst));
        Self {
            id,
            breakpoint_type,
            pattern: None,
            task_number: None,
            condition: None,
            enabled: true,
            hit_count: 0,
            description: None,
            temporary: false,
            ignore_count: 0,
        }
    }

    /// Create a breakpoint on a specific task
    pub fn on_task(name: impl Into<String>) -> Self {
        let mut bp = Self::new(BreakpointType::Task);
        bp.pattern = Some(name.into());
        bp
    }

    /// Create a breakpoint on a specific host
    pub fn on_host(host: impl Into<String>) -> Self {
        let mut bp = Self::new(BreakpointType::Host);
        bp.pattern = Some(host.into());
        bp
    }

    /// Create a breakpoint on a specific module
    pub fn on_module(module: impl Into<String>) -> Self {
        let mut bp = Self::new(BreakpointType::Module);
        bp.pattern = Some(module.into());
        bp
    }

    /// Create a breakpoint on a specific play
    pub fn on_play(play: impl Into<String>) -> Self {
        let mut bp = Self::new(BreakpointType::Play);
        bp.pattern = Some(play.into());
        bp
    }

    /// Create a breakpoint at a task number
    pub fn at_task_number(num: usize) -> Self {
        let mut bp = Self::new(BreakpointType::TaskNumber);
        bp.task_number = Some(num);
        bp
    }

    /// Create a breakpoint on any failure
    pub fn on_failure() -> Self {
        Self::new(BreakpointType::OnFailure)
    }

    /// Create a breakpoint on any change
    pub fn on_change() -> Self {
        Self::new(BreakpointType::OnChange)
    }

    /// Create a breakpoint on unreachable hosts
    pub fn on_unreachable() -> Self {
        Self::new(BreakpointType::OnUnreachable)
    }

    /// Create a breakpoint at play start
    pub fn at_play_start() -> Self {
        Self::new(BreakpointType::PlayStart)
    }

    /// Create a breakpoint at play end
    pub fn at_play_end() -> Self {
        Self::new(BreakpointType::PlayEnd)
    }

    /// Create a conditional breakpoint
    pub fn with_condition(condition: BreakpointCondition) -> Self {
        let mut bp = Self::new(BreakpointType::Conditional);
        bp.condition = Some(condition);
        bp
    }

    /// Create a temporary (one-shot) breakpoint
    pub fn temporary(mut self) -> Self {
        self.temporary = true;
        self.breakpoint_type = BreakpointType::Temporary;
        self
    }

    /// Set a description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set ignore count
    pub fn with_ignore_count(mut self, count: usize) -> Self {
        self.ignore_count = count;
        self
    }

    /// Add a condition to this breakpoint
    pub fn with_additional_condition(mut self, condition: BreakpointCondition) -> Self {
        self.condition = Some(match self.condition {
            Some(existing) => BreakpointCondition::All(vec![existing, condition]),
            None => condition,
        });
        self
    }

    /// Enable this breakpoint
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable this breakpoint
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if this breakpoint matches the current context
    pub fn matches(&self, context: &BreakpointContext) -> bool {
        if !self.enabled {
            return false;
        }

        // Check ignore count
        if self.hit_count < self.ignore_count {
            return false;
        }

        // Check type-specific matching
        let type_matches = match &self.breakpoint_type {
            BreakpointType::Task => self.pattern.as_ref().is_some_and(|p| {
                context
                    .task
                    .as_ref()
                    .is_some_and(|t| Self::pattern_matches(p, t))
            }),
            BreakpointType::Host => self.pattern.as_ref().is_some_and(|p| {
                context
                    .host
                    .as_ref()
                    .is_some_and(|h| Self::pattern_matches(p, h))
            }),
            BreakpointType::Module => self.pattern.as_ref().is_some_and(|p| {
                context
                    .module
                    .as_ref()
                    .is_some_and(|m| Self::pattern_matches(p, m))
            }),
            BreakpointType::Play => self.pattern.as_ref().is_some_and(|p| {
                context
                    .play
                    .as_ref()
                    .is_some_and(|pl| Self::pattern_matches(p, pl))
            }),
            BreakpointType::TaskNumber => self.task_number == Some(context.task_number),
            BreakpointType::OnFailure => context.failed,
            BreakpointType::OnChange => context.changed,
            BreakpointType::OnUnreachable => false, // Would need additional context
            BreakpointType::PlayStart | BreakpointType::PlayEnd => true, // Triggered by events
            BreakpointType::Conditional | BreakpointType::Temporary => true, // Condition-based
        };

        if !type_matches {
            return false;
        }

        // Check condition if present
        self.condition.as_ref().is_none_or(|c| c.evaluate(context))
    }

    /// Check if a pattern matches a value (supports wildcards)
    fn pattern_matches(pattern: &str, value: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.contains('*') {
            // Simple wildcard matching
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                if pattern.starts_with('*') && pattern.ends_with('*') {
                    return value.contains(parts[0]) || value.contains(parts[1]);
                } else if pattern.starts_with('*') {
                    return value.ends_with(parts[1]);
                } else if pattern.ends_with('*') {
                    return value.starts_with(parts[0]);
                }
            }
        }

        pattern == value
    }

    /// Record a hit on this breakpoint
    pub fn record_hit(&mut self) {
        self.hit_count += 1;
    }

    /// Get a display string for this breakpoint
    pub fn display(&self) -> String {
        let status = if self.enabled { "enabled" } else { "disabled" };
        let desc = self.description.as_deref().unwrap_or("");

        match &self.breakpoint_type {
            BreakpointType::Task => {
                format!(
                    "[{}] {} Task: {} {}",
                    self.id,
                    status,
                    self.pattern.as_deref().unwrap_or("*"),
                    desc
                )
            }
            BreakpointType::Host => {
                format!(
                    "[{}] {} Host: {} {}",
                    self.id,
                    status,
                    self.pattern.as_deref().unwrap_or("*"),
                    desc
                )
            }
            BreakpointType::TaskNumber => {
                format!(
                    "[{}] {} Task #: {} {}",
                    self.id,
                    status,
                    self.task_number.unwrap_or(0),
                    desc
                )
            }
            BreakpointType::OnFailure => {
                format!("[{}] {} On failure {}", self.id, status, desc)
            }
            BreakpointType::OnChange => {
                format!("[{}] {} On change {}", self.id, status, desc)
            }
            _ => {
                format!(
                    "[{}] {} {:?} {}",
                    self.id, status, self.breakpoint_type, desc
                )
            }
        }
    }
}

/// Manager for breakpoints
#[derive(Debug, Default)]
pub struct BreakpointManager {
    /// All registered breakpoints
    breakpoints: Vec<Breakpoint>,
}

impl BreakpointManager {
    /// Create a new breakpoint manager
    pub fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
        }
    }

    /// Add a breakpoint
    pub fn add(&mut self, breakpoint: Breakpoint) {
        self.breakpoints.push(breakpoint);
    }

    /// Remove a breakpoint by ID
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.breakpoints.len();
        self.breakpoints.retain(|bp| bp.id != id);
        self.breakpoints.len() < len_before
    }

    /// Clear all breakpoints
    pub fn clear(&mut self) {
        self.breakpoints.clear();
    }

    /// Get a breakpoint by ID
    pub fn get(&self, id: &str) -> Option<&Breakpoint> {
        self.breakpoints.iter().find(|bp| bp.id == id)
    }

    /// Get a mutable breakpoint by ID
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Breakpoint> {
        self.breakpoints.iter_mut().find(|bp| bp.id == id)
    }

    /// List all breakpoints
    pub fn list(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    /// Enable a breakpoint by ID
    pub fn enable(&mut self, id: &str) -> bool {
        if let Some(bp) = self.get_mut(id) {
            bp.enable();
            true
        } else {
            false
        }
    }

    /// Disable a breakpoint by ID
    pub fn disable(&mut self, id: &str) -> bool {
        if let Some(bp) = self.get_mut(id) {
            bp.disable();
            true
        } else {
            false
        }
    }

    /// Check if any breakpoint matches the current context
    pub fn check(&self, context: &BreakpointContext) -> Option<&Breakpoint> {
        self.breakpoints.iter().find(|bp| bp.matches(context))
    }

    /// Check and update hit counts, removing temporary breakpoints
    pub fn check_and_update(&mut self, context: &BreakpointContext) -> Option<String> {
        let mut matched_id = None;

        for bp in &mut self.breakpoints {
            if bp.matches(context) {
                bp.record_hit();
                matched_id = Some(bp.id.clone());
                break;
            }
        }

        // Remove temporary breakpoints that were hit
        if let Some(ref id) = matched_id {
            if let Some(bp) = self.get(id) {
                if bp.temporary && bp.hit_count > 0 {
                    self.remove(id);
                }
            }
        }

        matched_id
    }

    /// Count enabled breakpoints
    pub fn enabled_count(&self) -> usize {
        self.breakpoints.iter().filter(|bp| bp.enabled).count()
    }

    /// Count total breakpoints
    pub fn total_count(&self) -> usize {
        self.breakpoints.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> BreakpointContext {
        BreakpointContext::new()
            .with_playbook("test.yml")
            .with_play("Install software")
            .with_task("Install nginx")
            .with_host("web1")
            .with_module("apt")
            .with_progress(3, 10)
    }

    #[test]
    fn test_breakpoint_on_task() {
        let bp = Breakpoint::on_task("Install nginx");
        let context = test_context();
        assert!(bp.matches(&context));

        let context_other = test_context().with_task("Install apache");
        assert!(!bp.matches(&context_other));
    }

    #[test]
    fn test_breakpoint_on_host() {
        let bp = Breakpoint::on_host("web1");
        assert!(bp.matches(&test_context()));

        let bp = Breakpoint::on_host("web2");
        assert!(!bp.matches(&test_context()));
    }

    #[test]
    fn test_breakpoint_wildcard() {
        let bp = Breakpoint::on_task("Install*");
        assert!(bp.matches(&test_context()));

        let bp = Breakpoint::on_task("*nginx");
        assert!(bp.matches(&test_context()));

        let bp = Breakpoint::on_task("*soft*");
        assert!(!bp.matches(&test_context())); // Task is "Install nginx", not matching
    }

    #[test]
    fn test_breakpoint_on_failure() {
        let bp = Breakpoint::on_failure();

        let context = test_context();
        assert!(!bp.matches(&context));

        let context_failed = test_context().with_failed(true);
        assert!(bp.matches(&context_failed));
    }

    #[test]
    fn test_breakpoint_on_change() {
        let bp = Breakpoint::on_change();

        let context = test_context();
        assert!(!bp.matches(&context));

        let context_changed = test_context().with_changed(true);
        assert!(bp.matches(&context_changed));
    }

    #[test]
    fn test_breakpoint_at_task_number() {
        let bp = Breakpoint::at_task_number(3);
        assert!(bp.matches(&test_context()));

        let bp = Breakpoint::at_task_number(5);
        assert!(!bp.matches(&test_context()));
    }

    #[test]
    fn test_breakpoint_condition() {
        let mut vars = HashMap::new();
        vars.insert(
            "env".to_string(),
            JsonValue::String("production".to_string()),
        );

        let context = BreakpointContext::new()
            .with_task("Deploy")
            .with_variables(vars);

        let bp = Breakpoint::with_condition(BreakpointCondition::VarEquals {
            name: "env".to_string(),
            value: JsonValue::String("production".to_string()),
        });
        assert!(bp.matches(&context));

        let bp = Breakpoint::with_condition(BreakpointCondition::VarEquals {
            name: "env".to_string(),
            value: JsonValue::String("staging".to_string()),
        });
        assert!(!bp.matches(&context));
    }

    #[test]
    fn test_breakpoint_disabled() {
        let mut bp = Breakpoint::on_task("Install nginx");
        bp.disable();
        assert!(!bp.matches(&test_context()));

        bp.enable();
        assert!(bp.matches(&test_context()));
    }

    #[test]
    fn test_breakpoint_ignore_count() {
        let mut bp = Breakpoint::on_task("Install nginx").with_ignore_count(2);
        let context = test_context();

        assert!(!bp.matches(&context)); // hit_count = 0, ignore = 2
        bp.record_hit();
        assert!(!bp.matches(&context)); // hit_count = 1, ignore = 2
        bp.record_hit();
        assert!(bp.matches(&context)); // hit_count = 2, ignore = 2
    }

    #[test]
    fn test_breakpoint_manager() {
        let mut manager = BreakpointManager::new();

        let bp1 = Breakpoint::on_task("Install nginx");
        let bp2 = Breakpoint::on_host("web1");
        let id1 = bp1.id.clone();

        manager.add(bp1);
        manager.add(bp2);

        assert_eq!(manager.total_count(), 2);
        assert_eq!(manager.enabled_count(), 2);

        manager.disable(&id1);
        assert_eq!(manager.enabled_count(), 1);

        manager.remove(&id1);
        assert_eq!(manager.total_count(), 1);

        manager.clear();
        assert_eq!(manager.total_count(), 0);
    }

    #[test]
    fn test_breakpoint_manager_check() {
        let mut manager = BreakpointManager::new();
        manager.add(Breakpoint::on_task("Install nginx"));

        let context = test_context();
        assert!(manager.check(&context).is_some());

        let context_other = test_context().with_task("Other task");
        assert!(manager.check(&context_other).is_none());
    }

    #[test]
    fn test_condition_expression() {
        let mut vars = HashMap::new();
        vars.insert("env".to_string(), JsonValue::String("prod".to_string()));

        let context = BreakpointContext::new().with_variables(vars);

        let cond = BreakpointCondition::Expression("env == 'prod'".to_string());
        assert!(cond.evaluate(&context));

        let cond = BreakpointCondition::Expression("env != 'prod'".to_string());
        assert!(!cond.evaluate(&context));

        let cond = BreakpointCondition::Expression("defined(env)".to_string());
        assert!(cond.evaluate(&context));

        let cond = BreakpointCondition::Expression("defined(undefined_var)".to_string());
        assert!(!cond.evaluate(&context));
    }

    #[test]
    fn test_condition_combinators() {
        let vars: HashMap<String, JsonValue> = [
            ("a".to_string(), serde_json::json!(true)),
            ("b".to_string(), serde_json::json!(false)),
        ]
        .into_iter()
        .collect();

        let context = BreakpointContext::new().with_variables(vars);

        let cond = BreakpointCondition::All(vec![
            BreakpointCondition::VarDefined("a".to_string()),
            BreakpointCondition::VarDefined("b".to_string()),
        ]);
        assert!(cond.evaluate(&context));

        let cond = BreakpointCondition::Any(vec![
            BreakpointCondition::VarDefined("a".to_string()),
            BreakpointCondition::VarDefined("nonexistent".to_string()),
        ]);
        assert!(cond.evaluate(&context));

        let cond = BreakpointCondition::Not(Box::new(BreakpointCondition::VarDefined(
            "nonexistent".to_string(),
        )));
        assert!(cond.evaluate(&context));
    }
}
