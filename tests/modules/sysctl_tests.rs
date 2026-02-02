//! Integration tests for the sysctl module
//!
//! Tests validate parameter handling, error paths, and module metadata.
//! Execute tests verify proper error reporting when no connection is available,
//! and validate that parameters are correctly parsed before the connection check.

use rustible::modules::{
    sysctl::SysctlModule, Module, ModuleContext, ModuleContextBuilder, ModuleError, ModuleParams,
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
fn test_sysctl_set_value() {
    // Verify that setting a sysctl value with valid params reaches connection check
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.ipv4.ip_forward");
    params.insert("value".to_string(), serde_json::json!("1"));
    params.insert("state".to_string(), serde_json::json!("present"));

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
fn test_sysctl_remove_value() {
    // Verify that absent state reaches the connection check
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.ipv4.ip_forward");
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
fn test_sysctl_persist() {
    // Verify that sysctl_file param is accepted and module reaches connection check
    let module = SysctlModule;
    let mut params = with_name(create_params(), "vm.swappiness");
    params.insert("value".to_string(), serde_json::json!("10"));
    params.insert(
        "sysctl_file".to_string(),
        serde_json::json!("/etc/sysctl.d/99-custom.conf"),
    );

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
fn test_sysctl_reload() {
    // Verify that reload param is accepted and module reaches connection check
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.core.somaxconn");
    params.insert("value".to_string(), serde_json::json!("4096"));
    params.insert("reload".to_string(), serde_json::json!(true));

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
fn test_sysctl_idempotent() {
    // Verify that calling execute twice with same params produces the same error
    let module = SysctlModule;
    let mut params = with_name(create_params(), "net.ipv4.ip_forward");
    params.insert("value".to_string(), serde_json::json!("1"));

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
fn test_sysctl_check_mode() {
    // Verify that check() convenience method also requires a connection
    let module = SysctlModule;
    let mut params = with_name(create_params(), "kernel.pid_max");
    params.insert("value".to_string(), serde_json::json!("65535"));

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

#[test]
fn test_sysctl_network_params() {
    // Verify various network-related sysctl params are accepted
    let module = SysctlModule;
    let network_params = [
        ("net.ipv4.tcp_syncookies", "1"),
        ("net.ipv4.conf.all.rp_filter", "1"),
        ("net.ipv4.tcp_max_syn_backlog", "8192"),
        ("net.core.rmem_max", "16777216"),
    ];

    let context = check_mode_context();
    for (param_name, param_value) in &network_params {
        let mut params = with_name(create_params(), param_name);
        params.insert("value".to_string(), serde_json::json!(param_value));

        let result = module.execute(&params, &context);
        assert!(result.is_err());
        match result.unwrap_err() {
            ModuleError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("connection"),
                    "Param '{}' should pass validation, got: {}",
                    param_name,
                    msg
                );
            }
            other => panic!(
                "Expected ExecutionFailed for '{}', got: {:?}",
                param_name, other
            ),
        }
    }
}

#[test]
fn test_sysctl_kernel_params() {
    // Verify various kernel-related sysctl params are accepted
    let module = SysctlModule;
    let kernel_params = [
        ("kernel.pid_max", "65535"),
        ("kernel.shmmax", "68719476736"),
        ("kernel.msgmax", "65536"),
        ("kernel.core_pattern", "core"),
    ];

    let context = check_mode_context();
    for (param_name, param_value) in &kernel_params {
        let mut params = with_name(create_params(), param_name);
        params.insert("value".to_string(), serde_json::json!(param_value));

        let result = module.execute(&params, &context);
        assert!(result.is_err());
        match result.unwrap_err() {
            ModuleError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("connection"),
                    "Param '{}' should pass validation, got: {}",
                    param_name,
                    msg
                );
            }
            other => panic!(
                "Expected ExecutionFailed for '{}', got: {:?}",
                param_name, other
            ),
        }
    }
}
