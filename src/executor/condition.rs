//! Condition evaluation for changed_when/failed_when patterns.
//!
//! This module provides condition evaluation capabilities for determining
//! whether tasks have changed state or failed based on their output.

use crate::template::TEMPLATE_ENGINE;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;

/// A condition that can be evaluated against execution context.
///
/// Conditions are used for `when`, `changed_when`, and `failed_when` clauses
/// in task definitions.
#[derive(Debug, Clone)]
#[derive(Default)]
pub enum Condition {
    /// Always evaluates to true
    #[default]
    Always,
    /// Always evaluates to false
    Never,
    /// A boolean literal
    Boolean(bool),
    /// A Jinja2-like expression to evaluate
    Expression(String),
}

impl Condition {
    /// Create a condition from a string expression
    pub fn from_expression(expr: impl Into<String>) -> Self {
        Condition::Expression(expr.into())
    }

    /// Create an always-true condition
    pub fn always() -> Self {
        Condition::Always
    }

    /// Create an always-false condition
    pub fn never() -> Self {
        Condition::Never
    }

    /// Create a boolean condition
    pub fn boolean(value: bool) -> Self {
        Condition::Boolean(value)
    }
}


/// Context for condition evaluation.
///
/// Provides access to variables and task results needed to evaluate conditions.
#[derive(Debug, Clone, Default)]
pub struct ConditionContext {
    /// Variables available during evaluation
    pub variables: IndexMap<String, JsonValue>,
    /// The result of the current task (if available)
    pub task_result: Option<TaskResultContext>,
}

/// Task result context for condition evaluation
#[derive(Debug, Clone, Default)]
pub struct TaskResultContext {
    /// Return code of the command (if applicable)
    pub rc: Option<i32>,
    /// Standard output
    pub stdout: Option<String>,
    /// Standard error
    pub stderr: Option<String>,
    /// Whether the task reported a change
    pub changed: bool,
    /// Whether the task failed
    pub failed: bool,
}

impl ConditionContext {
    /// Create a new empty condition context
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a context with variables
    pub fn with_variables(variables: IndexMap<String, JsonValue>) -> Self {
        Self {
            variables,
            task_result: None,
        }
    }

    /// Set task result context
    pub fn with_task_result(mut self, result: TaskResultContext) -> Self {
        self.task_result = Some(result);
        self
    }

    /// Get a variable by name
    pub fn get_variable(&self, name: &str) -> Option<&JsonValue> {
        self.variables.get(name)
    }

    /// Check if a variable is defined
    pub fn is_defined(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    /// Build variables map for the template engine
    ///
    /// Merges context variables with task result fields (rc, stdout, stderr, etc.)
    /// so they can be accessed in condition expressions.
    pub fn build_template_vars(&self) -> IndexMap<String, JsonValue> {
        let mut vars = self.variables.clone();

        // Add task result fields if available
        if let Some(result) = &self.task_result {
            if let Some(rc) = result.rc {
                vars.insert("rc".to_string(), JsonValue::Number(rc.into()));
            }
            if let Some(stdout) = &result.stdout {
                vars.insert("stdout".to_string(), JsonValue::String(stdout.clone()));
            }
            if let Some(stderr) = &result.stderr {
                vars.insert("stderr".to_string(), JsonValue::String(stderr.clone()));
            }
            vars.insert("changed".to_string(), JsonValue::Bool(result.changed));
            vars.insert("failed".to_string(), JsonValue::Bool(result.failed));
        }

        vars
    }
}

/// Evaluator for condition expressions.
///
/// Provides methods to evaluate conditions against a context.
#[derive(Debug, Default)]
pub struct ConditionEvaluator {
    /// Enable strict mode (fail on undefined variables)
    pub strict_mode: bool,
}

impl ConditionEvaluator {
    /// Create a new condition evaluator
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluator with strict mode enabled
    pub fn strict() -> Self {
        Self { strict_mode: true }
    }

    /// Evaluate a condition against the given context
    pub fn evaluate(&self, condition: &Condition, ctx: &ConditionContext) -> Result<bool, String> {
        match condition {
            Condition::Always => Ok(true),
            Condition::Never => Ok(false),
            Condition::Boolean(b) => Ok(*b),
            Condition::Expression(expr) => self.evaluate_expression(expr, ctx),
        }
    }

    /// Evaluate a string expression using the unified template engine
    fn evaluate_expression(&self, expr: &str, ctx: &ConditionContext) -> Result<bool, String> {
        let expr = expr.trim();

        // Handle empty expression
        if expr.is_empty() {
            return Ok(true);
        }

        // Handle simple boolean literals (fast path before engine)
        match expr.to_lowercase().as_str() {
            "true" | "yes" => return Ok(true),
            "false" | "no" => return Ok(false),
            _ => {}
        }

        // Transform Ansible-style defined(var)/undefined(var) to Jinja2-style "var is defined"
        let transformed_expr = transform_defined_syntax(expr);

        // Build variables from context for the template engine
        let vars = ctx.build_template_vars();

        // Use the unified template engine for expression evaluation
        match TEMPLATE_ENGINE.evaluate_condition(&transformed_expr, &vars) {
            Ok(result) => Ok(result),
            Err(e) => {
                if self.strict_mode {
                    Err(format!("Condition evaluation failed: {}", e))
                } else {
                    // Non-strict: treat evaluation errors as false
                    Ok(false)
                }
            }
        }
    }
}

/// Transform Ansible-style defined(var)/undefined(var) to Jinja2-style expressions
fn transform_defined_syntax(expr: &str) -> String {
    let mut result = expr.to_string();

    // Handle defined(var) -> var is defined
    if let Some(start) = result.find("defined(") {
        if let Some(end) = result[start..].find(')') {
            let var_name = &result[start + 8..start + end].trim();
            let replacement = format!("{} is defined", var_name);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 1..]
            );
        }
    }

    // Handle undefined(var) -> var is undefined
    if let Some(start) = result.find("undefined(") {
        if let Some(end) = result[start..].find(')') {
            let var_name = &result[start + 10..start + end].trim();
            let replacement = format!("{} is undefined", var_name);
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + end + 1..]
            );
        }
    }

    result
}

/// Check if a JSON value is truthy
fn is_truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        JsonValue::String(s) => !s.is_empty() && s.to_lowercase() != "false" && s != "0",
        JsonValue::Array(a) => !a.is_empty(),
        JsonValue::Object(o) => !o.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition_always() {
        let eval = ConditionEvaluator::new();
        let ctx = ConditionContext::new();
        assert!(eval.evaluate(&Condition::Always, &ctx).unwrap());
    }

    #[test]
    fn test_condition_never() {
        let eval = ConditionEvaluator::new();
        let ctx = ConditionContext::new();
        assert!(!eval.evaluate(&Condition::Never, &ctx).unwrap());
    }

    #[test]
    fn test_condition_boolean() {
        let eval = ConditionEvaluator::new();
        let ctx = ConditionContext::new();
        assert!(eval.evaluate(&Condition::Boolean(true), &ctx).unwrap());
        assert!(!eval.evaluate(&Condition::Boolean(false), &ctx).unwrap());
    }

    #[test]
    fn test_expression_literals() {
        let eval = ConditionEvaluator::new();
        let ctx = ConditionContext::new();

        assert!(eval
            .evaluate(&Condition::Expression("true".into()), &ctx)
            .unwrap());
        assert!(!eval
            .evaluate(&Condition::Expression("false".into()), &ctx)
            .unwrap());
    }

    #[test]
    fn test_defined_check() {
        let eval = ConditionEvaluator::new();
        let mut vars = IndexMap::new();
        vars.insert("my_var".into(), JsonValue::String("value".into()));
        let ctx = ConditionContext::with_variables(vars);

        assert!(eval
            .evaluate(&Condition::Expression("defined(my_var)".into()), &ctx)
            .unwrap());
        assert!(!eval
            .evaluate(&Condition::Expression("defined(other_var)".into()), &ctx)
            .unwrap());
    }

    #[test]
    fn test_not_expression() {
        let eval = ConditionEvaluator::new();
        let ctx = ConditionContext::new();

        assert!(!eval
            .evaluate(&Condition::Expression("not true".into()), &ctx)
            .unwrap());
        assert!(eval
            .evaluate(&Condition::Expression("not false".into()), &ctx)
            .unwrap());
    }

    #[test]
    fn test_is_truthy() {
        assert!(!is_truthy(&JsonValue::Null));
        assert!(!is_truthy(&JsonValue::Bool(false)));
        assert!(is_truthy(&JsonValue::Bool(true)));
        assert!(!is_truthy(&JsonValue::String("".into())));
        assert!(is_truthy(&JsonValue::String("hello".into())));
        assert!(!is_truthy(&JsonValue::Array(vec![])));
        assert!(is_truthy(&JsonValue::Array(vec![JsonValue::Null])));
    }
}
