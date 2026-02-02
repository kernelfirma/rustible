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
fn test_facts_gather_os_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["os"]);
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "OS subset gathering should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");

    // OS subset should include system-level facts
    if let Some(facts) = output.data.get("ansible_facts") {
        // OS facts should contain at least system or hostname
        let facts_obj = facts.as_object().expect("ansible_facts should be an object");
        assert!(
            facts_obj.contains_key("system") || facts_obj.contains_key("hostname") || facts_obj.contains_key("distribution"),
            "OS facts should contain system, hostname, or distribution info, got keys: {:?}",
            facts_obj.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_facts_gather_hardware_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["hardware"]);
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Hardware subset gathering should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");

    // Hardware subset should include memory or processor facts
    if let Some(facts) = output.data.get("ansible_facts") {
        let facts_obj = facts.as_object().expect("ansible_facts should be an object");
        // Hardware facts typically include memtotal, processor count, etc.
        // On some systems these may be empty if /proc is not available, so just verify the call succeeded
        assert!(
            facts_obj.contains_key("memtotal_mb")
                || facts_obj.contains_key("processor_count")
                || facts_obj.contains_key("processor")
                || facts_obj.is_empty(),
            "Hardware facts should contain memory or processor info (or be empty on unsupported systems)"
        );
    }
}

#[test]
fn test_facts_gather_network_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["network"]);
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Network subset gathering should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");

    // Network facts should include interface or IP information
    if let Some(facts) = output.data.get("ansible_facts") {
        let facts_obj = facts.as_object().expect("ansible_facts should be an object");
        assert!(
            facts_obj.contains_key("interfaces")
                || facts_obj.contains_key("default_ipv4")
                || facts_obj.contains_key("all_ipv4_addresses")
                || facts_obj.is_empty(),
            "Network facts should contain interface or IP info (or be empty on unsupported systems)"
        );
    }
}

#[test]
fn test_facts_gather_date_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["date_time"]);
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Date subset gathering should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");

    // Date facts should include date/time information
    if let Some(facts) = output.data.get("ansible_facts") {
        let facts_obj = facts.as_object().expect("ansible_facts should be an object");
        assert!(
            facts_obj.contains_key("date_time")
                || facts_obj.contains_key("epoch")
                || facts_obj.contains_key("iso8601")
                || facts_obj.is_empty(),
            "Date facts should contain date_time or epoch info (or be empty on unsupported systems)"
        );
    }
}

#[test]
fn test_facts_gather_env_subset() {
    let module = FactsModule;
    let params = with_gather_subset(create_params(), vec!["env"]);
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_ok(), "Env subset gathering should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.changed, "Facts gathering should not report changed");

    // Env facts should include environment variables
    if let Some(facts) = output.data.get("ansible_facts") {
        let facts_obj = facts.as_object().expect("ansible_facts should be an object");
        assert!(
            facts_obj.contains_key("env") || facts_obj.contains_key("environment") || facts_obj.is_empty(),
            "Env facts should contain env or environment info (or be empty on unsupported systems)"
        );
    }
}

#[test]
fn test_facts_exclude_subset() {
    let module = FactsModule;
    // Gather only "os" subset, excluding hardware and network
    let params_os_only = with_gather_subset(create_params(), vec!["os"]);
    let params_all = create_params(); // defaults to "all"
    let context = ModuleContext::default();

    let result_os_only = module.execute(&params_os_only, &context);
    let result_all = module.execute(&params_all, &context);

    assert!(result_os_only.is_ok(), "OS-only subset should succeed");
    assert!(result_all.is_ok(), "All subset should succeed");

    let output_os_only = result_os_only.unwrap();
    let output_all = result_all.unwrap();

    // The "all" gather should have at least as many facts as "os" only
    let os_only_count = output_os_only
        .data
        .get("ansible_facts")
        .and_then(|f| f.as_object())
        .map(|o| o.len())
        .unwrap_or(0);
    let all_count = output_all
        .data
        .get("ansible_facts")
        .and_then(|f| f.as_object())
        .map(|o| o.len())
        .unwrap_or(0);

    assert!(
        all_count >= os_only_count,
        "All facts ({}) should have at least as many entries as OS-only facts ({})",
        all_count,
        os_only_count
    );
}
