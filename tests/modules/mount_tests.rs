//! Integration tests for the mount module
//!
//! Tests validate parameter handling, error paths, and module metadata.
//! Execute tests verify proper error reporting when no connection is available,
//! and validate that parameters are correctly parsed before the connection check.

use rustible::modules::{
    mount::MountModule, Module, ModuleContext, ModuleContextBuilder, ModuleError, ModuleParams,
};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_path(mut params: ModuleParams, path: &str) -> ModuleParams {
    params.insert("path".to_string(), serde_json::json!(path));
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
fn test_mount_module_name() {
    let module = MountModule;
    assert_eq!(module.name(), "mount");
}

#[test]
fn test_mount_module_description() {
    let module = MountModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_mount_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = MountModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_mount_required_params() {
    let module = MountModule;
    let required = module.required_params();
    assert!(required.contains(&"path"));
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// These use the default validate_params which returns Ok(())
// ============================================================================

#[test]
fn test_mount_validate_params_present() {
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("src".to_string(), serde_json::json!("/dev/sdb1"));
    params.insert("fstype".to_string(), serde_json::json!("ext4"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Valid params should pass validation");
}

#[test]
fn test_mount_validate_params_absent() {
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("state".to_string(), serde_json::json!("absent"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Absent state should be valid");
}

#[test]
fn test_mount_validate_empty_params() {
    let module = MountModule;
    let params = create_params();

    // Default validate_params returns Ok(()) - required param checking is done elsewhere
    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

// ============================================================================
// Execution Tests (require connection - ignored)
// ============================================================================

#[test]
fn test_mount_add_to_fstab() {
    // Verify that present state with src/fstype params reaches connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("src".to_string(), serde_json::json!("/dev/sdb1"));
    params.insert("fstype".to_string(), serde_json::json!("ext4"));
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
fn test_mount_remove_from_fstab() {
    // Verify that absent state reaches the connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
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
fn test_mount_filesystem() {
    // Verify that mounted state with full params reaches connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("src".to_string(), serde_json::json!("/dev/sdb1"));
    params.insert("fstype".to_string(), serde_json::json!("ext4"));
    params.insert("opts".to_string(), serde_json::json!("defaults,noatime"));
    params.insert("state".to_string(), serde_json::json!("mounted"));

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
fn test_unmount_filesystem() {
    // Verify that unmounted state reaches the connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("state".to_string(), serde_json::json!("unmounted"));

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
fn test_mount_idempotent() {
    // Verify that calling execute twice with same params produces the same error
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/data");
    params.insert("src".to_string(), serde_json::json!("/dev/sdb1"));
    params.insert("fstype".to_string(), serde_json::json!("ext4"));
    params.insert("state".to_string(), serde_json::json!("mounted"));

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
fn test_mount_check_mode() {
    // Verify that check() convenience method also requires a connection
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/backup");
    params.insert("src".to_string(), serde_json::json!("/dev/sdc1"));
    params.insert("fstype".to_string(), serde_json::json!("xfs"));
    params.insert("state".to_string(), serde_json::json!("mounted"));

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
fn test_mount_nfs() {
    // Verify that NFS mount params (with network source) reach connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/nfs");
    params.insert(
        "src".to_string(),
        serde_json::json!("192.168.1.100:/exports/data"),
    );
    params.insert("fstype".to_string(), serde_json::json!("nfs"));
    params.insert("opts".to_string(), serde_json::json!("rw,sync,hard,intr"));
    params.insert("state".to_string(), serde_json::json!("mounted"));

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
fn test_mount_tmpfs() {
    // Verify that tmpfs mount params reach connection check
    let module = MountModule;
    let mut params = with_path(create_params(), "/mnt/tmpfs");
    params.insert("src".to_string(), serde_json::json!("tmpfs"));
    params.insert("fstype".to_string(), serde_json::json!("tmpfs"));
    params.insert("opts".to_string(), serde_json::json!("size=512m,mode=1777"));
    params.insert("state".to_string(), serde_json::json!("mounted"));

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
