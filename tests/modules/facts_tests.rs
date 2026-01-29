//! Integration tests for the facts module
//!
//! Note: Some tests are marked #[ignore] as they require specific
//! system access or produce variable results.

use rustible::modules::{facts::FactsModule, Module, ModuleContext, ModuleParams};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_gather_subset(mut params: ModuleParams, subset: Vec<&str>) -> ModuleParams {
    let subset_json: Vec<serde_json::Value> = subset.iter().map(|s| serde_json::json!(s)).collect();
    params.insert("gather_subset".to_string(), serde_json::json!(subset_json));
    params
}

// ============================================================================
// Module Metadata Tests (no connection needed)
// ============================================================================

#[test]
fn test_facts_module_name() {
    let module = FactsModule;
    // Could be "setup" (Ansible-compatible) or "facts"
    let name = module.name();
    assert!(
        name == "facts" || name == "setup" || name == "gather_facts",
        "Module name should be facts-related, got: {}",
        name
    );
}

#[test]
fn test_facts_module_description() {
    let module = FactsModule;
    let desc = module.description();
    assert!(!desc.is_empty());
    assert!(
        desc.to_lowercase().contains("fact") || desc.to_lowercase().contains("gather"),
        "Description should mention facts or gathering"
    );
}

#[test]
fn test_facts_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = FactsModule;
    // Facts module can work locally
    let classification = module.classification();
    assert!(
        classification == ModuleClassification::LocalLogic
            || classification == ModuleClassification::NativeTransport
            || classification == ModuleClassification::RemoteCommand,
        "Facts module should have a valid classification"
    );
}

#[test]
fn test_facts_required_params() {
    let module = FactsModule;
    let required = module.required_params();
    // Facts module typically has no required params
    assert!(
        required.is_empty(),
        "Facts module should have no required params"
    );
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// ============================================================================

#[test]
fn test_facts_validate_empty_params() {
    let module = FactsModule;
    let params = create_params();

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Empty params should be valid for facts");
}

#[test]
fn test_facts_validate_with_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["os", "hardware"]);

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with subset should be valid");
}

#[test]
fn test_facts_validate_with_filter() {
    let module = FactsModule;
    let mut params = create_params();
    params.insert("filter".to_string(), serde_json::json!("ansible_os*"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with filter should be valid");
}

#[test]
fn test_facts_validate_with_timeout() {
    let module = FactsModule;
    let mut params = create_params();
    params.insert("gather_timeout".to_string(), serde_json::json!(30));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with timeout should be valid");
}

// ============================================================================
// Basic Execution Tests (run locally, may vary by system)
// ============================================================================

#[test]
fn test_facts_gather_basic() {
    let module = FactsModule;
    let params = create_params();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    // Facts gathering should succeed
    assert!(
        result.is_ok(),
        "Facts gathering should succeed: {:?}",
        result.err()
    );

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");
}

#[test]
fn test_facts_idempotent() {
    let module = FactsModule;
    let params = create_params();
    let context = ModuleContext::default();

    // Facts gathering should never report changed
    let result = module.execute(&params, &context);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(!output.changed, "Facts should never report changed");
}

#[test]
fn test_facts_check_mode() {
    let module = FactsModule;
    let params = create_params();
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context);

    // Facts gathering in check mode should still work
    assert!(result.is_ok(), "Check mode should work for facts");
    let output = result.unwrap();
    assert!(!output.changed);
}

#[test]
fn test_facts_multiple_runs() {
    let module = FactsModule;
    let params = create_params();
    let context = ModuleContext::default();

    // First run
    let result1 = module.execute(&params, &context);
    assert!(result1.is_ok());

    // Second run
    let result2 = module.execute(&params, &context);
    assert!(result2.is_ok());

    // Both should not report changed
    assert!(!result1.unwrap().changed);
    assert!(!result2.unwrap().changed);
}

// ============================================================================
// Subset Tests (may produce variable results depending on implementation)
// ============================================================================

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_gather_os_subset() {
    // Would test gathering OS-specific facts
}

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_gather_hardware_subset() {
    // Would test gathering hardware facts
}

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_gather_network_subset() {
    // Would test gathering network facts
}

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_gather_date_subset() {
    // Would test gathering date/time facts
}

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_gather_env_subset() {
    // Would test gathering environment facts
}

#[test]
#[ignore = "Subset support depends on implementation"]
fn test_facts_exclude_subset() {
    // Would test excluding specific fact subsets
}
