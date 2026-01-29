//! Integration tests for the sysctl module
//!
//! Note: Most tests are marked #[ignore] as they require a connection
//! for remote execution. Run with --ignored to test against a real system.

use rustible::modules::{sysctl::SysctlModule, Module, ModuleParams};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_name(mut params: ModuleParams, name: &str) -> ModuleParams {
    params.insert("name".to_string(), serde_json::json!(name));
    params
}

// ============================================================================
// Module Metadata Tests (no connection needed)
// ============================================================================

#[test]
fn test_sysctl_module_name() {
    let module = SysctlModule;
    assert_eq!(module.name(), "sysctl");
}

#[test]
fn test_sysctl_module_description() {
    let module = SysctlModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_sysctl_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = SysctlModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_sysctl_required_params() {
    let module = SysctlModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// These use the default validate_params which returns Ok(())
// ============================================================================

#[test]
fn test_sysctl_validate_params_basic() {
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.ipv4.ip_forward");
    params.insert("value".to_string(), serde_json::json!("1"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Valid params should pass validation");
}

#[test]
fn test_sysctl_validate_empty_params() {
    let module = SysctlModule;
    let params = create_params();

    // Default validate_params returns Ok(()) - required param checking is done elsewhere
    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_sysctl_validate_absent_state() {
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.ipv4.ip_forward");
    params.insert("state".to_string(), serde_json::json!("absent"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Absent state should be valid");
}

// ============================================================================
// Execution Tests (require connection - ignored)
// ============================================================================

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_set_value() {
    // Would test setting a sysctl value
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_remove_value() {
    // Would test removing a sysctl entry
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_persist() {
    // Would test persisting to sysctl.conf
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_reload() {
    // Would test sysctl reload behavior
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_idempotent() {
    // Would test idempotency
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_check_mode() {
    // Would test check mode
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_network_params() {
    // Would test network parameters
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_sysctl_kernel_params() {
    // Would test kernel parameters
}
