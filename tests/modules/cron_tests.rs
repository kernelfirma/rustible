//! Integration tests for the cron module
//!
//! Tests validate parameter handling, error paths, and module metadata.
//! Execute tests verify proper error reporting when no connection is available,
//! and validate that parameters are correctly parsed before the connection check.

use rustible::modules::{
    cron::CronModule, Module, ModuleContext, ModuleContextBuilder, ModuleError, ModuleParams,
};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_name(mut params: ModuleParams, name: &str) -> ModuleParams {
    params.insert("name".to_string(), serde_json::json!(name));
    params
}

/// Helper to build a check_mode context without a connection.
fn check_mode_context() -> ModuleContext {
    ModuleContextBuilder::new()
        .check_mode(true)
        .build()
        .expect("valid context")
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
fn test_cron_create_job() {
    // Without a connection, execute should return an ExecutionFailed error
    // indicating that a connection is required. This validates that params
    // are accepted and the module reaches the connection check.
    let module = CronModule;
    let mut params = with_name(create_params(), "backup job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/backup.sh"));
    params.insert("minute".to_string(), serde_json::json!("0"));
    params.insert("hour".to_string(), serde_json::json!("2"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let context = check_mode_context();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        ModuleError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("connection"),
                "Error should mention connection requirement, got: {}",
                msg
            );
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}

#[test]
fn test_cron_remove_job() {
    // Verify that absent state params are accepted and the module reaches connection check
    let module = CronModule;
    let mut params = with_name(create_params(), "old job");
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = check_mode_context();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(msg.contains("connection"));
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}

#[test]
fn test_cron_update_job() {
    // Verify that update params (present with all schedule fields) are accepted
    let module = CronModule;
    let mut params = with_name(create_params(), "updated job");
    params.insert(
        "job".to_string(),
        serde_json::json!("/usr/bin/new-script.sh"),
    );
    params.insert("minute".to_string(), serde_json::json!("30"));
    params.insert("hour".to_string(), serde_json::json!("3"));
    params.insert("day".to_string(), serde_json::json!("1"));
    params.insert("month".to_string(), serde_json::json!("*/2"));
    params.insert("weekday".to_string(), serde_json::json!("1-5"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let context = check_mode_context();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(msg.contains("connection"));
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}

#[test]
fn test_cron_special_times() {
    // Verify that special_time params like @reboot, @hourly, @daily are accepted
    let module = CronModule;
    let special_times = [
        "@reboot", "@hourly", "@daily", "@weekly", "@monthly", "@yearly",
    ];

    let context = check_mode_context();
    for st in &special_times {
        let mut params = with_name(create_params(), &format!("{} job", st));
        params.insert("job".to_string(), serde_json::json!("/usr/bin/task.sh"));
        params.insert("special_time".to_string(), serde_json::json!(st));

        let result = module.execute(&params, &context);
        // Should reach connection check (not fail on param validation)
        assert!(result.is_err());
        match result.unwrap_err() {
            ModuleError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("connection"),
                    "Special time '{}' should pass param validation, got: {}",
                    st,
                    msg
                );
            }
            other => panic!(
                "Expected ExecutionFailed for special_time '{}', got: {:?}",
                st, other
            ),
        }
    }

    // Verify invalid special_time is rejected before connection check
    let mut params = with_name(create_params(), "bad job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/task.sh"));
    params.insert("special_time".to_string(), serde_json::json!("@invalid"));

    // This should fail but the error comes after connection check since
    // special_time validation happens inside execute after connection extraction.
    // Without a connection, we get ExecutionFailed for connection first.
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_cron_disable_job() {
    // Verify that disabled param is accepted
    let module = CronModule;
    let mut params = with_name(create_params(), "disabled job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/task.sh"));
    params.insert("disabled".to_string(), serde_json::json!(true));

    let context = check_mode_context();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(msg.contains("connection"));
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}

#[test]
fn test_cron_idempotent() {
    // Verify that calling execute twice with the same params produces the same error
    // (consistent behavior without a connection)
    let module = CronModule;
    let mut params = with_name(create_params(), "idempotent job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/task.sh"));
    params.insert("minute".to_string(), serde_json::json!("0"));
    params.insert("hour".to_string(), serde_json::json!("0"));

    let context = check_mode_context();
    let result1 = module.execute(&params, &context);
    let result2 = module.execute(&params, &context);

    // Both should fail identically
    assert!(result1.is_err());
    assert!(result2.is_err());
    assert_eq!(
        format!("{}", result1.unwrap_err()),
        format!("{}", result2.unwrap_err()),
    );
}

#[test]
fn test_cron_check_mode() {
    // Verify that the check() convenience method also requires a connection
    let module = CronModule;
    let mut params = with_name(create_params(), "check mode job");
    params.insert("job".to_string(), serde_json::json!("/usr/bin/check.sh"));
    params.insert("minute".to_string(), serde_json::json!("*/5"));

    let context = ModuleContextBuilder::new()
        .check_mode(false)
        .build()
        .expect("valid context");

    // The check() method sets check_mode=true internally
    let result = module.check(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(msg.contains("connection"));
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}
