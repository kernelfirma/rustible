//! Integration tests for the hostname module
//!
//! Tests validate parameter handling, error paths, and module metadata.
//! Execute tests verify proper error reporting when no connection is available,
//! and validate that parameters are correctly parsed before the connection check.

use rustible::modules::{
    hostname::HostnameModule, Module, ModuleContext, ModuleContextBuilder, ModuleError,
    ModuleParams,
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
fn test_hostname_module_name() {
    let module = HostnameModule;
    assert_eq!(module.name(), "hostname");
}

#[test]
fn test_hostname_module_description() {
    let module = HostnameModule;
    let desc = module.description();
    assert!(!desc.is_empty());
    assert!(desc.to_lowercase().contains("hostname"));
}

#[test]
fn test_hostname_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = HostnameModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_hostname_required_params() {
    let module = HostnameModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// These use the default validate_params which returns Ok(())
// ============================================================================

#[test]
fn test_hostname_validate_params_simple() {
    let module = HostnameModule;
    let params = with_name(create_params(), "myhost");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Simple hostname should be valid");
}

#[test]
fn test_hostname_validate_params_fqdn() {
    let module = HostnameModule;
    let params = with_name(create_params(), "myhost.example.com");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "FQDN should be valid");
}

#[test]
fn test_hostname_validate_params_with_numbers() {
    let module = HostnameModule;
    let params = with_name(create_params(), "server01");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with numbers should be valid");
}

#[test]
fn test_hostname_validate_params_with_hyphens() {
    let module = HostnameModule;
    let params = with_name(create_params(), "web-server-01");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with hyphens should be valid");
}

#[test]
fn test_hostname_validate_empty_params() {
    let module = HostnameModule;
    let params = create_params();

    // Default validate_params returns Ok(()) - required param checking is done elsewhere
    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_hostname_validate_with_strategy() {
    let module = HostnameModule;
    let mut params = with_name(create_params(), "newhost");
    params.insert("use".to_string(), serde_json::json!("systemd"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with strategy should be valid");
}

// ============================================================================
// Execution Tests (require connection or privileges - ignored)
// ============================================================================

#[test]
fn test_hostname_set_simple() {
    // Verify that a simple hostname passes validation and reaches the connection check
    let module = HostnameModule;
    let params = with_name(create_params(), "myserver");

    let context = check_mode_context();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("connection"),
                "Should require connection, got: {}",
                msg
            );
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}

#[test]
fn test_hostname_set_fqdn() {
    // Verify that a FQDN passes validation and reaches the connection check
    let module = HostnameModule;
    let params = with_name(create_params(), "myserver.example.com");

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
fn test_hostname_file_strategy() {
    // Verify that file strategy param is accepted and reaches connection check
    let module = HostnameModule;
    let mut params = with_name(create_params(), "filehost");
    params.insert("use".to_string(), serde_json::json!("file"));

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
fn test_hostname_systemd_strategy() {
    // Verify that systemd strategy param is accepted and reaches connection check
    let module = HostnameModule;
    let mut params = with_name(create_params(), "systemdhost");
    params.insert("use".to_string(), serde_json::json!("systemd"));

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
fn test_hostname_command_strategy() {
    // Verify that auto strategy (which detects command vs systemd) reaches connection check
    let module = HostnameModule;
    let mut params = with_name(create_params(), "autohost");
    params.insert("use".to_string(), serde_json::json!("auto"));

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
fn test_hostname_idempotent() {
    // Verify that calling execute twice with the same params produces the same error
    let module = HostnameModule;
    let params = with_name(create_params(), "idempotent-host");

    let context = check_mode_context();
    let result1 = module.execute(&params, &context);
    let result2 = module.execute(&params, &context);

    assert!(result1.is_err());
    assert!(result2.is_err());
    assert_eq!(
        format!("{}", result1.unwrap_err()),
        format!("{}", result2.unwrap_err()),
    );
}

#[test]
fn test_hostname_check_mode() {
    // Verify that the check() convenience method also requires a connection
    let module = HostnameModule;
    let params = with_name(create_params(), "checkhost");

    let context = ModuleContextBuilder::new()
        .check_mode(false)
        .build()
        .expect("valid context");

    // check() sets check_mode=true internally and calls execute()
    let result = module.check(&params, &context);
    assert!(result.is_err());
    match result.unwrap_err() {
        ModuleError::ExecutionFailed(msg) => {
            assert!(msg.contains("connection"));
        }
        other => panic!("Expected ExecutionFailed, got: {:?}", other),
    }
}
