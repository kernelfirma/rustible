// Integration tests for the debug module
use rustible::modules::{debug::DebugModule, Module, ModuleContext, ModuleParams};
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn test_debug_with_simple_message() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "msg".to_string(),
        Value::String("Hello from debug!".to_string()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed, "Debug module should never change anything");
    assert_eq!(result.msg, "Hello from debug!");
    assert!(result.data.contains_key("msg"));
}

#[test]
fn test_debug_with_variable() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("var".to_string(), Value::String("my_var".to_string()));

    let mut vars = HashMap::new();
    vars.insert("my_var".to_string(), Value::String("my value".to_string()));
    let context = ModuleContext::default().with_vars(vars);

    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("my_var"));
    assert!(result.msg.contains("my value"));
    assert!(result.data.contains_key("my_var"));
}

#[test]
fn test_debug_with_undefined_variable() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("var".to_string(), Value::String("undefined".to_string()));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("VARIABLE IS NOT DEFINED"));
}

#[test]
fn test_debug_with_nested_variable() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("var".to_string(), Value::String("user.name".to_string()));

    let mut vars = HashMap::new();
    let mut user = serde_json::Map::new();
    user.insert("name".to_string(), Value::String("John Doe".to_string()));
    user.insert("age".to_string(), Value::Number(30.into()));
    vars.insert("user".to_string(), Value::Object(user));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("user.name"));
    assert!(result.msg.contains("John Doe"));
}

#[test]
fn test_debug_with_complex_object() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("var".to_string(), Value::String("config".to_string()));

    let mut vars = HashMap::new();
    let config = serde_json::json!({
        "database": {
            "host": "localhost",
            "port": 5432,
            "name": "mydb"
        },
        "settings": {
            "debug": true,
            "verbose": 2
        }
    });
    vars.insert("config".to_string(), config);

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("config"));
    assert!(result.data.contains_key("config"));
}

#[test]
fn test_debug_with_facts() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "var".to_string(),
        Value::String("ansible_hostname".to_string()),
    );

    let mut facts = HashMap::new();
    facts.insert(
        "ansible_hostname".to_string(),
        Value::String("testhost".to_string()),
    );
    let context = ModuleContext::default().with_facts(facts);

    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("ansible_hostname"));
    assert!(result.msg.contains("testhost"));
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
fn test_debug_validation_cannot_have_both() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("msg".to_string(), Value::String("message".to_string()));
    params.insert("var".to_string(), Value::String("variable".to_string()));

    let result = module.validate_params(&params);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Cannot specify both"));
}

#[test]
fn test_debug_check_mode() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "msg".to_string(),
        Value::String("Check mode test".to_string()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    // Debug module behaves the same in check mode
    assert!(!result.changed);
    assert_eq!(result.msg, "Check mode test");
}

#[test]
fn test_debug_with_verbosity() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "msg".to_string(),
        Value::String("Verbose message".to_string()),
    );
    params.insert("verbosity".to_string(), Value::Number(2.into()));

    let context = ModuleContext::default();

    // Without setting RUSTIBLE_VERBOSITY env var, default is 0
    // So this should be shown (assuming default behavior shows all messages)
    let result = module.execute(&params, &context).unwrap();

    // The exact behavior depends on environment variable
    assert!(!result.changed);
}

#[test]
fn test_debug_with_array_variable() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("var".to_string(), Value::String("items".to_string()));

    let mut vars = HashMap::new();
    let items = serde_json::json!(["item1", "item2", "item3"]);
    vars.insert("items".to_string(), items);

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("items"));
    assert!(result.data.contains_key("items"));
}

#[test]
fn test_debug_classification() {
    let module = DebugModule;

    // Debug module should be LocalLogic since it runs on control node
    use rustible::modules::ModuleClassification;
    assert_eq!(module.classification(), ModuleClassification::LocalLogic);
}

#[test]
fn test_debug_parallelization() {
    let module = DebugModule;

    // Debug module should be fully parallel
    use rustible::modules::ParallelizationHint;
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::FullyParallel
    );
}

#[test]
fn test_debug_with_json_message() {
    let module = DebugModule;
    let mut params: ModuleParams = HashMap::new();

    // Test with a complex JSON value as msg
    let msg_value = serde_json::json!({
        "status": "ok",
        "count": 42
    });
    params.insert("msg".to_string(), msg_value);

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    // Should be formatted as JSON
    assert!(result.msg.contains("status") || result.msg.contains("ok"));
}
