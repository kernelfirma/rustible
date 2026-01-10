//! Debug module - Print debug messages and variable values
//!
//! This module is used for debugging playbooks. It prints messages or variable
//! values to the console. Unlike most modules, it runs entirely on the control
//! node and does not require an SSH connection.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde_json::Value;
use std::collections::HashMap;

/// Module for printing debug messages and variable values
pub struct DebugModule;

impl DebugModule {
    /// Format a variable value for display
    fn format_value(&self, value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Null => "(undefined)".to_string(),
            _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| format!("{:?}", value)),
        }
    }

    /// Get the value of a variable from context
    fn get_variable_value(&self, var_name: &str, context: &ModuleContext) -> Option<Value> {
        // Try to parse as a JSON path or simple variable name
        // First check vars, then facts
        if let Some(value) = context.vars.get(var_name) {
            return Some(value.clone());
        }

        if let Some(value) = context.facts.get(var_name) {
            return Some(value.clone());
        }

        // Try to handle nested paths like "ansible_facts.hostname"
        if var_name.contains('.') {
            let parts: Vec<&str> = var_name.split('.').collect();

            // Try vars first
            if let Some(root) = context.vars.get(parts[0]) {
                let mut current = root;
                for part in &parts[1..] {
                    if let Value::Object(obj) = current {
                        if let Some(val) = obj.get(*part) {
                            current = val;
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                return Some(current.clone());
            }

            // Try facts
            if let Some(root) = context.facts.get(parts[0]) {
                let mut current = root;
                for part in &parts[1..] {
                    if let Value::Object(obj) = current {
                        if let Some(val) = obj.get(*part) {
                            current = val;
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                return Some(current.clone());
            }
        }

        None
    }

    /// Check if the current verbosity level allows this message to be shown
    fn should_show(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<bool> {
        // Get the verbosity parameter (default to 0)
        let required_verbosity = params.get_i64("verbosity")?.unwrap_or(0);
        let current_verbosity = i64::from(context.verbosity);

        Ok(current_verbosity >= required_verbosity)
    }
}

impl Module for DebugModule {
    fn name(&self) -> &'static str {
        "debug"
    }

    fn description(&self) -> &'static str {
        "Print debug messages or variable values to the console"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs entirely on the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel since it's just printing
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // Neither msg nor var is strictly required, but at least one should be provided
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have either msg or var
        if params.get("msg").is_none() && params.get("var").is_none() {
            return Err(ModuleError::InvalidParameter(
                "Either 'msg' or 'var' must be provided".to_string(),
            ));
        }

        // Cannot have both msg and var
        if params.get("msg").is_some() && params.get("var").is_some() {
            return Err(ModuleError::InvalidParameter(
                "Cannot specify both 'msg' and 'var' parameters".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Check if we should show this message based on verbosity
        if !self.should_show(params, context)? {
            return Ok(ModuleOutput::skipped("Skipped due to verbosity level"));
        }

        let mut output_data: HashMap<String, Value> = HashMap::new();
        let message: String;

        if let Some(msg) = params.get("msg") {
            // Print a message
            message = match msg {
                Value::String(s) => s.clone(),
                _ => self.format_value(msg),
            };

            output_data.insert("msg".to_string(), Value::String(message.clone()));
        } else if let Some(var_param) = params.get("var") {
            // Print a variable value
            let var_name = match var_param {
                Value::String(s) => s,
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "'var' parameter must be a string".to_string(),
                    ))
                }
            };

            match self.get_variable_value(var_name, context) {
                Some(value) => {
                    let formatted = self.format_value(&value);
                    message = format!("{}: {}", var_name, formatted);
                    output_data.insert(var_name.clone(), value);
                }
                None => {
                    // Variable not found
                    message = format!("{}: VARIABLE IS NOT DEFINED!", var_name);
                    output_data.insert(var_name.clone(), Value::Null);
                }
            }
        } else {
            // This shouldn't happen due to validate_params, but handle it anyway
            return Err(ModuleError::InvalidParameter(
                "Either 'msg' or 'var' must be provided".to_string(),
            ));
        }

        // Debug module never changes anything - it's always "ok" status
        let mut output = ModuleOutput::ok(message);
        output.data = output_data;

        Ok(output)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ModuleStatus;

    #[test]
    fn test_debug_with_msg() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "msg".to_string(),
            Value::String("Hello, World!".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.msg, "Hello, World!");
        assert!(result.data.contains_key("msg"));
    }

    #[test]
    fn test_debug_with_var() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("var".to_string(), Value::String("test_var".to_string()));

        let mut vars = HashMap::new();
        vars.insert(
            "test_var".to_string(),
            Value::String("test value".to_string()),
        );
        let context = ModuleContext::default().with_vars(vars);

        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("test_var"));
        assert!(result.msg.contains("test value"));
        assert!(result.data.contains_key("test_var"));
    }

    #[test]
    fn test_debug_with_undefined_var() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "var".to_string(),
            Value::String("undefined_var".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("VARIABLE IS NOT DEFINED"));
        assert!(result.data.contains_key("undefined_var"));
        assert_eq!(result.data.get("undefined_var"), Some(&Value::Null));
    }

    #[test]
    fn test_debug_with_nested_var() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "var".to_string(),
            Value::String("ansible_facts.hostname".to_string()),
        );

        let mut vars = HashMap::new();
        let mut ansible_facts = serde_json::Map::new();
        ansible_facts.insert(
            "hostname".to_string(),
            Value::String("testhost".to_string()),
        );
        vars.insert("ansible_facts".to_string(), Value::Object(ansible_facts));

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("ansible_facts.hostname"));
        assert!(result.msg.contains("testhost"));
    }

    #[test]
    fn test_debug_with_verbosity() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "msg".to_string(),
            Value::String("Debug message".to_string()),
        );
        params.insert("verbosity".to_string(), Value::Number(2.into()));

        // Default context has verbosity 0, so this should be skipped
        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();
        assert_eq!(result.status, ModuleStatus::Skipped);

        // Context with verbosity 2 should show the message
        let context = ModuleContext::default().with_verbosity(2);
        let result = module.execute(&params, &context).unwrap();
        assert_eq!(result.status, ModuleStatus::Ok);
        assert_eq!(result.msg, "Debug message");
    }

    #[test]
    fn test_debug_validation_requires_msg_or_var() {
        let module = DebugModule;
        let params: ModuleParams = HashMap::new();

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Either 'msg' or 'var'"));
    }

    #[test]
    fn test_debug_validation_not_both() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), Value::String("Hello".to_string()));
        params.insert("var".to_string(), Value::String("test".to_string()));

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Cannot specify both"));
    }

    #[test]
    fn test_debug_check_mode_same_as_execute() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), Value::String("Test".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.msg, "Test");
    }

    #[test]
    fn test_debug_with_complex_object() {
        let module = DebugModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("var".to_string(), Value::String("complex_var".to_string()));

        let mut vars = HashMap::new();
        let complex_value = serde_json::json!({
            "key1": "value1",
            "key2": 42,
            "nested": {
                "inner": true
            }
        });
        vars.insert("complex_var".to_string(), complex_value);

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("complex_var"));
        assert!(result.data.contains_key("complex_var"));
    }
}
