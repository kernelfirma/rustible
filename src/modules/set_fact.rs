//! Set_fact module - Set host variables dynamically during playbook execution
//!
//! This module allows you to set variables (facts) on hosts during playbook execution.
//! Unlike gathered facts from the setup module, these are user-defined variables that
//! persist for the duration of the play and can be used in subsequent tasks.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint,
};
use serde_json::Value;
use std::collections::HashMap;

/// Module for setting host facts/variables dynamically
pub struct SetFactModule;

impl Module for SetFactModule {
    fn name(&self) -> &'static str {
        "set_fact"
    }

    fn description(&self) -> &'static str {
        "Set host variables (facts) that persist for the duration of the play"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs entirely on the control node
        // It doesn't need to connect to the remote host, just updates the runtime context
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel since each host gets its own variables
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // No required params - all key=value pairs become facts
        // At least one fact must be provided, but we validate that separately
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have at least one parameter that's not 'cacheable'
        let fact_count = params.keys().filter(|k| k.as_str() != "cacheable").count();

        if fact_count == 0 {
            return Err(ModuleError::InvalidParameter(
                "set_fact requires at least one key=value pair to set".to_string(),
            ));
        }

        // Validate cacheable if present
        if let Some(cacheable) = params.get("cacheable") {
            match cacheable {
                Value::Bool(_) => {}
                Value::String(s) => {
                    let lower = s.to_lowercase();
                    if !["true", "false", "yes", "no", "1", "0"].contains(&lower.as_str()) {
                        return Err(ModuleError::InvalidParameter(
                            "cacheable must be a boolean value".to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "cacheable must be a boolean value".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Note: The actual variable setting happens in the executor (execute_set_fact)
        // because this module needs to modify the RuntimeContext, which isn't
        // directly accessible from the Module trait.
        //
        // This module implementation serves as documentation and validation,
        // and returns a structured result that the executor can use.

        let cacheable = match params.get("cacheable") {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => {
                let lower = s.to_lowercase();
                ["true", "yes", "1"].contains(&lower.as_str())
            }
            _ => false,
        };

        let mut facts_set = Vec::new();
        let mut data = HashMap::new();

        // Collect all facts being set (excluding cacheable)
        for (key, value) in params {
            if key != "cacheable" {
                facts_set.push(key.clone());
                data.insert(key.clone(), value.clone());
            }
        }

        let message = if facts_set.len() == 1 {
            format!("Set fact: {}", facts_set[0])
        } else {
            format!("Set {} facts: {}", facts_set.len(), facts_set.join(", "))
        };

        // set_fact never actually changes the system state, but by Ansible convention
        // it's reported as "ok" not "changed"
        let mut output = ModuleOutput::ok(message);
        output.data = data;

        if cacheable {
            output
                .data
                .insert("cacheable".to_string(), Value::Bool(true));
        }

        Ok(output)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_fact_validation() {
        let module = SetFactModule;

        // Valid: single fact
        let mut params: ModuleParams = HashMap::new();
        params.insert("my_var".to_string(), Value::String("value".to_string()));
        assert!(module.validate_params(&params).is_ok());

        // Valid: multiple facts
        params.insert("another_var".to_string(), Value::Number(42.into()));
        assert!(module.validate_params(&params).is_ok());

        // Valid: with cacheable
        params.insert("cacheable".to_string(), Value::Bool(true));
        assert!(module.validate_params(&params).is_ok());

        // Invalid: no facts
        let empty_params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&empty_params).is_err());

        // Invalid: only cacheable
        let mut cacheable_only: ModuleParams = HashMap::new();
        cacheable_only.insert("cacheable".to_string(), Value::Bool(true));
        assert!(module.validate_params(&cacheable_only).is_err());
    }

    #[test]
    fn test_set_fact_execute() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "my_fact".to_string(),
            Value::String("test_value".to_string()),
        );
        params.insert("number_fact".to_string(), Value::Number(100.into()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed); // set_fact doesn't mark as changed
        assert!(result.msg.contains("Set 2 facts"));
        assert!(result.data.contains_key("my_fact"));
        assert!(result.data.contains_key("number_fact"));
    }

    #[test]
    fn test_set_fact_with_cacheable() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cached_var".to_string(),
            Value::String("cached".to_string()),
        );
        params.insert("cacheable".to_string(), Value::Bool(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("cached_var"));
        assert_eq!(result.data.get("cacheable"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_set_fact_check_mode() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("my_fact".to_string(), Value::String("value".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        // In check mode, set_fact still works
        assert!(!result.changed);
        assert!(result.data.contains_key("my_fact"));
    }

    #[test]
    fn test_set_fact_complex_values() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();

        // Dictionary value
        let dict_value = serde_json::json!({
            "key1": "value1",
            "key2": 42,
            "nested": {
                "inner": true
            }
        });
        params.insert("dict_fact".to_string(), dict_value);

        // List value
        let list_value = serde_json::json!(["item1", "item2", "item3"]);
        params.insert("list_fact".to_string(), list_value);

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("dict_fact"));
        assert!(result.data.contains_key("list_fact"));
    }

    #[test]
    fn test_set_fact_module_classification() {
        let module = SetFactModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::FullyParallel
        );
    }
}
