//! Assert module - Fail task if conditions are not met
//!
//! This module allows you to assert that certain conditions are true.
//! If the conditions are not met, the task fails with an optional message.
//! Unlike most modules, it runs entirely on the control node and does not
//! require a connection to remote hosts.
//!
//! # Parameters
//!
//! - `that` (required): A single condition or list of conditions to evaluate.
//!   All conditions must be true for the assertion to pass.
//! - `msg` / `fail_msg`: Message to display when assertion fails.
//! - `success_msg`: Message to display when assertion passes.
//! - `quiet`: When true, suppresses detailed output about evaluated conditions.
//!
//! # Examples
//!
//! ```yaml
//! - name: Assert that version is valid
//!   assert:
//!     that:
//!       - version is defined
//!       - version >= "1.0.0"
//!     fail_msg: "Version must be 1.0.0 or higher"
//!     success_msg: "Version check passed"
//!
//! - name: Simple assertion with quiet mode
//!   assert:
//!     that: enabled == true
//!     quiet: true
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::template::TemplateEngine;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

/// Global template engine instance for assertions
/// This avoids recreating the engine for every task, improving performance
static TEMPLATE_ENGINE: Lazy<TemplateEngine> = Lazy::new(TemplateEngine::new);

/// Module for asserting conditions are true
pub struct AssertModule;

impl AssertModule {
    /// Evaluate a single condition string using the template engine
    fn evaluate_condition(
        &self,
        condition: &str,
        vars: &HashMap<String, Value>,
    ) -> ModuleResult<bool> {
        // Wrap the condition in a template expression that evaluates to a boolean
        let template = format!("{{{{ {} }}}}", condition);

        let result = TEMPLATE_ENGINE.render(&template, vars).map_err(|e| {
            ModuleError::TemplateError(format!(
                "Failed to evaluate condition '{}': {}",
                condition, e
            ))
        })?;

        // Parse the result as a boolean
        // In Jinja2, empty strings, "False", "false", "0" are considered false
        let result = result.trim();
        let is_true = !result.is_empty()
            && result != "False"
            && result != "false"
            && result != "0"
            && result != "None"
            && result != "none";

        Ok(is_true)
    }

    /// Evaluate all conditions and return the list of failures
    fn evaluate_all_conditions(
        &self,
        conditions: &[String],
        vars: &HashMap<String, Value>,
    ) -> ModuleResult<Vec<String>> {
        let mut failed_conditions = Vec::new();

        for condition in conditions {
            match self.evaluate_condition(condition, vars) {
                Ok(true) => {
                    // Condition passed
                }
                Ok(false) => {
                    // Condition failed
                    failed_conditions.push(condition.clone());
                }
                Err(e) => {
                    // Error evaluating condition - treat as failure
                    failed_conditions.push(format!("{} (evaluation error: {})", condition, e));
                }
            }
        }

        Ok(failed_conditions)
    }
}

impl Module for AssertModule {
    fn name(&self) -> &'static str {
        "assert"
    }

    fn description(&self) -> &'static str {
        "Assert that given expressions are true"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs entirely on the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel since it's just evaluating conditions
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["that"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // 'that' parameter is required
        if params.get("that").is_none() {
            return Err(ModuleError::MissingParameter(
                "'that' parameter is required".to_string(),
            ));
        }

        // Validate that 'that' is either a string or array
        if let Some(that_param) = params.get("that") {
            match that_param {
                Value::String(_) | Value::Array(_) => {}
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "'that' parameter must be a string or list of strings".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Get the conditions to assert
        let conditions: Vec<String> = match params.get("that") {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(arr)) => {
                let mut conds = Vec::new();
                for item in arr {
                    match item {
                        Value::String(s) => conds.push(s.clone()),
                        _ => conds.push(item.to_string()),
                    }
                }
                conds
            }
            _ => {
                return Err(ModuleError::MissingParameter(
                    "'that' parameter is required".to_string(),
                ));
            }
        };

        // Get optional messages
        let fail_msg = params.get_string("msg")?;
        let success_msg = params.get_string("success_msg")?;
        let quiet = params.get_bool_or("quiet", false);

        // Evaluate all conditions
        let failed_conditions = self.evaluate_all_conditions(&conditions, &context.vars)?;

        // Check if all conditions passed
        if failed_conditions.is_empty() {
            // All assertions passed
            let message = if let Some(msg) = success_msg {
                msg
            } else if quiet {
                "All assertions passed".to_string()
            } else {
                format!(
                    "All assertions passed (evaluated {} condition{})",
                    conditions.len(),
                    if conditions.len() == 1 { "" } else { "s" }
                )
            };

            let mut output = ModuleOutput::ok(message);

            // Add evaluated conditions to output data if not quiet
            if !quiet {
                output.data.insert(
                    "evaluated_to".to_string(),
                    Value::Array(
                        conditions
                            .iter()
                            .map(|c| Value::String(c.clone()))
                            .collect(),
                    ),
                );
            }

            Ok(output)
        } else {
            // Some assertions failed
            let message = if let Some(msg) = fail_msg {
                msg
            } else {
                format!(
                    "Assertion failed: {} of {} condition{} failed",
                    failed_conditions.len(),
                    conditions.len(),
                    if conditions.len() == 1 { "" } else { "s" }
                )
            };

            let mut output = ModuleOutput::failed(message);

            // Add details about failed conditions
            output.data.insert(
                "assertion".to_string(),
                Value::Array(
                    conditions
                        .iter()
                        .map(|c| Value::String(c.clone()))
                        .collect(),
                ),
            );
            output.data.insert(
                "failed_conditions".to_string(),
                Value::Array(
                    failed_conditions
                        .iter()
                        .map(|c| Value::String(c.clone()))
                        .collect(),
                ),
            );

            Ok(output)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_single_condition_pass() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("true".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert!(result.msg.contains("All assertions passed"));
    }

    #[test]
    fn test_assert_single_condition_fail() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("false".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Failed);
        assert!(result.msg.contains("Assertion failed"));
    }

    #[test]
    fn test_assert_multiple_conditions_all_pass() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::Array(vec![
                Value::String("true".to_string()),
                Value::String("1 == 1".to_string()),
                Value::String("'hello' == 'hello'".to_string()),
            ]),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert!(result.msg.contains("All assertions passed"));
        assert!(result.msg.contains("3 conditions"));
    }

    #[test]
    fn test_assert_multiple_conditions_some_fail() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::Array(vec![
                Value::String("true".to_string()),
                Value::String("false".to_string()),
                Value::String("1 == 2".to_string()),
            ]),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Failed);
        assert!(result.msg.contains("2 of 3 conditions failed"));
        assert!(result.data.contains_key("failed_conditions"));
    }

    #[test]
    fn test_assert_with_variables() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::Array(vec![
                Value::String("foo == 'bar'".to_string()),
                Value::String("count > 5".to_string()),
            ]),
        );

        let mut vars = HashMap::new();
        vars.insert("foo".to_string(), Value::String("bar".to_string()));
        vars.insert("count".to_string(), Value::Number(10.into()));
        let context = ModuleContext::default().with_vars(vars);

        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
    }

    #[test]
    fn test_assert_with_custom_fail_msg() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("false".to_string()));
        params.insert(
            "msg".to_string(),
            Value::String("Custom error message".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Failed);
        assert_eq!(result.msg, "Custom error message");
    }

    #[test]
    fn test_assert_with_success_msg() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("true".to_string()));
        params.insert(
            "success_msg".to_string(),
            Value::String("Everything is awesome!".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert_eq!(result.msg, "Everything is awesome!");
    }

    #[test]
    fn test_assert_validation_requires_that() {
        let module = AssertModule;
        let params: ModuleParams = HashMap::new();

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'that'"));
    }

    #[test]
    fn test_assert_check_mode_same_as_execute() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("true".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
    }

    #[test]
    fn test_assert_with_complex_expressions() {
        // Test complex expressions that ARE supported by Tera template engine
        // Note: Jinja2's `is version()` test is NOT supported - use comparison operators instead
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::Array(vec![
                Value::String("ansible_os_family == 'Debian'".to_string()),
                // Use simple string comparison instead of version() test
                Value::String("ansible_distribution_version >= '20.04'".to_string()),
            ]),
        );

        let mut vars = HashMap::new();
        vars.insert(
            "ansible_os_family".to_string(),
            Value::String("Debian".to_string()),
        );
        vars.insert(
            "ansible_distribution_version".to_string(),
            Value::String("22.04".to_string()),
        );
        let context = ModuleContext::default().with_vars(vars);

        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
    }

    #[test]
    fn test_assert_quiet_mode() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("that".to_string(), Value::String("true".to_string()));
        params.insert("quiet".to_string(), Value::Bool(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert_eq!(result.msg, "All assertions passed");
        assert!(!result.data.contains_key("evaluated_to"));
    }

    #[test]
    fn test_assert_with_undefined_variable() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::String("undefined_var == 'test'".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        // Should fail because the condition can't be evaluated or evaluates to false
        assert_eq!(result.status, super::super::ModuleStatus::Failed);
    }

    #[test]
    fn test_assert_with_logical_operators() {
        let module = AssertModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "that".to_string(),
            Value::Array(vec![
                Value::String("(foo == 'bar') and (count > 5)".to_string()),
                Value::String("status == 'ok' or status == 'success'".to_string()),
            ]),
        );

        let mut vars = HashMap::new();
        vars.insert("foo".to_string(), Value::String("bar".to_string()));
        vars.insert("count".to_string(), Value::Number(10.into()));
        vars.insert("status".to_string(), Value::String("ok".to_string()));
        let context = ModuleContext::default().with_vars(vars);

        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
    }
}
