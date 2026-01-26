//! Integration tests for the mount module
//!
//! Note: Most tests are marked #[ignore] as they require a connection
//! for remote execution. Run with --ignored to test against a real system.

use rustible::modules::{mount::MountModule, Module, ModuleParams};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_path(mut params: ModuleParams, path: &str) -> ModuleParams {
    params.insert("path".to_string(), serde_json::json!(path));
    params
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
#[ignore = "Requires connection for remote execution"]
fn test_mount_add_to_fstab() {
    // Would test adding entry to fstab
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_remove_from_fstab() {
    // Would test removing entry from fstab
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_filesystem() {
    // Would test mounting filesystem
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_unmount_filesystem() {
    // Would test unmounting filesystem
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_idempotent() {
    // Would test idempotency
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_check_mode() {
    // Would test check mode
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_nfs() {
    // Would test NFS mount
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_mount_tmpfs() {
    // Would test tmpfs mount
}
