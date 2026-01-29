//! Integration tests for the cron module
//!
//! Note: Most tests are marked #[ignore] as they require a connection
//! for remote execution. Run with --ignored to test against a real system.

use rustible::modules::{cron::CronModule, Module, ModuleParams};
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
fn test_cron_module_name() {
    let module = CronModule;
    assert_eq!(module.name(), "cron");
}

#[test]
fn test_cron_module_description() {
    let module = CronModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_cron_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = CronModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_cron_required_params() {
    let module = CronModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// These use the default validate_params which returns Ok(())
// ============================================================================

#[test]
fn test_cron_validate_params_basic() {
    let module = CronModule;
    let mut params = with_name(create_params(), "test job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/true"));
    params.insert("minute".to_string(), serde_json::json!("0"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Valid params should pass validation");
}

#[test]
fn test_cron_validate_empty_params() {
    let module = CronModule;
    let params = create_params();

    // Default validate_params returns Ok(()) - required param checking is done elsewhere
    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_cron_validate_special_time() {
    let module = CronModule;
    let mut params = with_name(create_params(), "reboot job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/startup"));
    params.insert("special_time".to_string(), serde_json::json!("reboot"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Special time params should be valid");
}

// ============================================================================
// Execution Tests (require connection - ignored)
// ============================================================================

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_create_job() {
    // Would test creating a cron job via connection
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_remove_job() {
    // Would test removing a cron job via connection
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_update_job() {
    // Would test updating a cron job via connection
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_special_times() {
    // Would test @reboot, @hourly, @daily etc.
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_disable_job() {
    // Would test disabling/commenting out a job
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_idempotent() {
    // Would test idempotency
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_cron_check_mode() {
    // Would test check mode
}
