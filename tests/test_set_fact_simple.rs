// Simple test to verify set_fact module functionality
// This can be run with: cargo test --test test_set_fact_simple

#[cfg(test)]
mod tests {
    use rustible::executor::runtime::RuntimeContext;
    use rustible::modules::set_fact::SetFactModule;
    use rustible::modules::{Module, ModuleContext, ModuleParams};
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_set_fact_basic() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("test_var".to_string(), Value::String("test_value".to_string()));
        params.insert("test_num".to_string(), Value::Number(42.into()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Set 2 facts"));
        assert_eq!(result.data.get("test_var"), Some(&Value::String("test_value".to_string())));
        assert_eq!(result.data.get("test_num"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn test_set_fact_runtime_integration() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("testhost".to_string(), None);

        // Simulate set_fact behavior
        runtime.set_host_fact("testhost", "my_fact".to_string(), Value::String("my_value".to_string()));

        // Verify the fact is set
        let merged = runtime.get_merged_vars("testhost");
        assert_eq!(merged.get("my_fact"), Some(&Value::String("my_value".to_string())));
    }

    #[test]
    fn test_set_fact_persistence() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("testhost".to_string(), None);

        // Set multiple facts
        runtime.set_host_fact("testhost", "fact1".to_string(), Value::String("value1".to_string()));
        runtime.set_host_fact("testhost", "fact2".to_string(), Value::Number(100.into()));
        runtime.set_host_fact("testhost", "fact3".to_string(), Value::Bool(true));

        // Verify all facts persist
        let merged = runtime.get_merged_vars("testhost");
        assert_eq!(merged.get("fact1"), Some(&Value::String("value1".to_string())));
        assert_eq!(merged.get("fact2"), Some(&Value::Number(100.into())));
        assert_eq!(merged.get("fact3"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_set_fact_complex_types() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();

        // Dictionary
        let dict = serde_json::json!({
            "key1": "value1",
            "key2": 42
        });
        params.insert("my_dict".to_string(), dict.clone());

        // Array
        let array = serde_json::json!(["item1", "item2"]);
        params.insert("my_array".to_string(), array.clone());

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("my_dict"));
        assert!(result.data.contains_key("my_array"));
    }

    #[test]
    fn test_set_fact_cacheable() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cached_var".to_string(), Value::String("cached".to_string()));
        params.insert("cacheable".to_string(), Value::Bool(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("cached_var"));
        assert_eq!(result.data.get("cacheable"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_set_fact_validation_requires_facts() {
        let module = SetFactModule;
        let params: ModuleParams = HashMap::new();

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_set_fact_validation_cacheable_only_fails() {
        let module = SetFactModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cacheable".to_string(), Value::Bool(true));

        assert!(module.validate_params(&params).is_err());
    }
}
