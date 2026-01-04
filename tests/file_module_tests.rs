//! Comprehensive tests for the File module
//!
//! This test suite covers all file module functionality including:
//! - Directory creation and management
//! - File creation and management
//! - File removal (state=absent)
//! - Touch operation (state=touch)
//! - Permission changes (mode parameter)
//! - Ownership changes (owner/group parameters)
//! - Symbolic and hard links
//! - Idempotency checks for all operations
//! - Check mode behavior
//! - Error handling
//! - Edge cases

use rustible::modules::{file::FileModule, Module, ModuleContext, ModuleError, ModuleParams};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tempfile::TempDir;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a module params HashMap with path
fn params_with_path(path: &str) -> ModuleParams {
    let mut params = HashMap::new();
    params.insert("path".to_string(), serde_json::json!(path));
    params
}

/// Create a module params HashMap with path and state
fn params_with_state(path: &str, state: &str) -> ModuleParams {
    let mut params = params_with_path(path);
    params.insert("state".to_string(), serde_json::json!(state));
    params
}

// ============================================================================
// Directory Creation Tests (state=directory)
// ============================================================================

#[test]
fn test_directory_create_simple() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating directory"
    );
    assert!(path.is_dir(), "Directory should exist on filesystem");
    assert!(
        result.msg.contains("Created directory"),
        "Message should indicate directory creation"
    );
}

#[test]
fn test_directory_create_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not report changed when directory already exists"
    );
    assert!(path.is_dir(), "Directory should still exist");
}

#[test]
fn test_directory_create_nested() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("a").join("b").join("c").join("d");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "directory");
    params.insert("recurse".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating nested directories"
    );
    assert!(path.is_dir(), "Nested directory should exist");
    assert!(
        path.parent().unwrap().is_dir(),
        "Parent directories should be created"
    );
}

#[test]
fn test_directory_create_with_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "directory");
    params.insert("mode".to_string(), serde_json::json!(0o755));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating directory"
    );
    assert!(path.is_dir(), "Directory should exist");

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o7777,
        0o755,
        "Directory should have correct permissions"
    );
}

#[test]
fn test_directory_update_mode_on_existing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "directory");
    params.insert("mode".to_string(), serde_json::json!(0o700));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when updating permissions"
    );

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o7777,
        0o700,
        "Directory should have updated permissions"
    );
}

#[test]
fn test_directory_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Check mode should report changed for new directory"
    );
    assert!(
        result.msg.contains("Would create"),
        "Message should indicate what would happen"
    );
    assert!(
        !path.exists(),
        "Directory should not be created in check mode"
    );
}

#[test]
fn test_directory_check_mode_existing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Check mode should not report changed for existing directory"
    );
}

// ============================================================================
// File Removal Tests (state=absent)
// ============================================================================

#[test]
fn test_absent_remove_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "test content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when removing file");
    assert!(!path.exists(), "File should be removed");
    assert!(
        result.msg.contains("Removed"),
        "Message should indicate removal"
    );
}

#[test]
fn test_absent_remove_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "absent");
    params.insert("recurse".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when removing directory"
    );
    assert!(!path.exists(), "Directory should be removed");
}

#[test]
fn test_absent_remove_directory_with_contents() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir_all(path.join("subdir")).unwrap();
    fs::write(path.join("file1.txt"), "content1").unwrap();
    fs::write(path.join("subdir/file2.txt"), "content2").unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "absent");
    params.insert("recurse".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when removing directory tree"
    );
    assert!(
        !path.exists(),
        "Directory and all contents should be removed"
    );
}

#[test]
fn test_absent_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not report changed when file already absent"
    );
    assert!(
        result.msg.contains("already absent"),
        "Message should indicate already absent"
    );
}

#[test]
fn test_absent_remove_symlink() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    let link_str = link.to_str().unwrap();
    fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let module = FileModule;
    let params = params_with_state(link_str, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when removing symlink"
    );
    assert!(!link.exists(), "Symlink should be removed");
    assert!(target.exists(), "Target should still exist");
}

#[test]
fn test_absent_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "absent");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Check mode should report changed for file to remove"
    );
    assert!(
        result.msg.contains("Would remove"),
        "Message should indicate what would happen"
    );
    assert!(path.exists(), "File should not be removed in check mode");
}

// ============================================================================
// Touch Tests (state=touch)
// ============================================================================

#[test]
fn test_touch_create_new_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("newfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "touch");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating new file"
    );
    assert!(path.exists(), "File should be created");
    assert!(path.is_file(), "Should be a regular file");
}

#[test]
fn test_touch_existing_file_updates_timestamp() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("existing");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "existing content").unwrap();

    // Get initial modification time
    let initial_mtime = fs::metadata(&path).unwrap().modified().unwrap();

    // Sleep to ensure time difference
    std::thread::sleep(std::time::Duration::from_millis(100));

    let module = FileModule;
    let params = params_with_state(path_str, "touch");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when touching existing file"
    );

    let final_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        final_mtime > initial_mtime,
        "Modification time should be updated"
    );

    // Verify content is unchanged
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "existing content", "Content should be unchanged");
}

#[test]
fn test_touch_with_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("touchfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "touch");
    params.insert("mode".to_string(), serde_json::json!(0o644));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when creating file");

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o7777,
        0o644,
        "File should have correct permissions"
    );
}

#[test]
fn test_touch_creates_parent_directories() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("parent").join("child").join("touchfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "touch");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating file with parents"
    );
    assert!(path.exists(), "File should be created");
    assert!(
        path.parent().unwrap().is_dir(),
        "Parent directories should be created"
    );
}

#[test]
fn test_touch_check_mode_new_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("touchfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "touch");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Check mode should report changed for new file"
    );
    assert!(
        result.msg.contains("Would create"),
        "Message should indicate file creation"
    );
    assert!(!path.exists(), "File should not be created in check mode");
}

#[test]
fn test_touch_check_mode_existing_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("existing");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "touch");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Check mode should report changed for existing file"
    );
    assert!(
        result.msg.contains("Would update timestamps"),
        "Message should indicate timestamp update"
    );
}

// ============================================================================
// Permission Change Tests (mode parameter)
// ============================================================================

#[test]
fn test_mode_change_on_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "file");
    params.insert("mode".to_string(), serde_json::json!(0o600));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when updating permissions"
    );

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o7777,
        0o600,
        "File should have updated permissions"
    );
}

#[test]
fn test_mode_change_on_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "directory");
    params.insert("mode".to_string(), serde_json::json!(0o700));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when updating directory permissions"
    );

    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o7777,
        0o700,
        "Directory should have updated permissions"
    );
}

#[test]
fn test_mode_no_change_when_same() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "file");
    params.insert("mode".to_string(), serde_json::json!(0o644));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not report changed when permissions already correct"
    );
}

#[test]
fn test_mode_various_permissions() {
    let temp = TempDir::new().unwrap();

    let test_modes = vec![0o777, 0o755, 0o700, 0o644, 0o600, 0o444, 0o400];

    for mode in test_modes {
        let path = temp.path().join(format!("file_{:o}", mode));
        let path_str = path.to_str().unwrap();

        let module = FileModule;
        let mut params = params_with_state(path_str, "file");
        params.insert("mode".to_string(), serde_json::json!(mode));
        let context = ModuleContext::default();

        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed, "Should create file with mode {:o}", mode);

        let meta = fs::metadata(&path).unwrap();
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            mode,
            "File should have mode {:o}",
            mode
        );
    }
}

// ============================================================================
// Idempotency Tests
// ============================================================================

#[test]
fn test_idempotent_file_creation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "file");
    let context = ModuleContext::default();

    // First execution - should create file
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed, "First execution should report changed");

    // Second execution - should be idempotent
    let result2 = module.execute(&params, &context).unwrap();
    assert!(
        !result2.changed,
        "Second execution should not report changed"
    );

    // Third execution - should still be idempotent
    let result3 = module.execute(&params, &context).unwrap();
    assert!(
        !result3.changed,
        "Third execution should not report changed"
    );
}

#[test]
fn test_idempotent_directory_creation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default();

    // Multiple executions
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);

    let result3 = module.execute(&params, &context).unwrap();
    assert!(!result3.changed);
}

#[test]
fn test_idempotent_absent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "absent");
    let context = ModuleContext::default();

    // First execution - nothing to remove
    let result1 = module.execute(&params, &context).unwrap();
    assert!(!result1.changed);

    // Second execution - still nothing to remove
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

#[test]
fn test_idempotent_mode_changes() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let mut params = params_with_state(path_str, "file");
    params.insert("mode".to_string(), serde_json::json!(0o600));
    let context = ModuleContext::default();

    // First execution - should change mode
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed, "First execution should change mode");

    // Second execution - mode already correct
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed, "Second execution should be idempotent");

    // Third execution - still idempotent
    let result3 = module.execute(&params, &context).unwrap();
    assert!(!result3.changed, "Third execution should be idempotent");
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_error_missing_path_parameter() {
    let module = FileModule;
    let params = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should error when path is missing");
    match result {
        Err(ModuleError::MissingParameter(msg)) => {
            assert!(msg.contains("path"), "Error should mention path parameter");
        }
        _ => panic!("Expected MissingParameter error"),
    }
}

#[test]
fn test_error_invalid_state() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "invalid_state");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should error with invalid state");
    match result {
        Err(ModuleError::InvalidParameter(msg)) => {
            assert!(
                msg.contains("Invalid state"),
                "Error should mention invalid state"
            );
        }
        _ => panic!("Expected InvalidParameter error"),
    }
}

#[test]
fn test_error_symlink_missing_src() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("link");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "link");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(
        result.is_err(),
        "Should error when src is missing for symlink"
    );
    match result {
        Err(ModuleError::MissingParameter(msg)) => {
            assert!(msg.contains("src"), "Error should mention src parameter");
        }
        _ => panic!("Expected MissingParameter error"),
    }
}

#[test]
fn test_error_hardlink_missing_src() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("hardlink");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "hard");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(
        result.is_err(),
        "Should error when src is missing for hard link"
    );
}

#[test]
fn test_error_file_exists_when_creating_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("conflict");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "file content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(
        result.is_err(),
        "Should error when file exists at directory path"
    );
}

#[test]
fn test_error_directory_exists_when_creating_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("conflict");
    let path_str = path.to_str().unwrap();
    fs::create_dir(&path).unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "file");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(
        result.is_err(),
        "Should error when directory exists at file path"
    );
}

// ============================================================================
// Symlink Tests
// ============================================================================

#[test]
fn test_symlink_create() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::write(&target, "target content").unwrap();

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "link");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating symlink"
    );
    assert!(link.is_symlink(), "Link should be a symlink");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        target,
        "Symlink should point to target"
    );
}

#[test]
fn test_symlink_idempotent() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::write(&target, "content").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "link");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not report changed when symlink already correct"
    );
}

#[test]
fn test_symlink_to_nonexistent_target() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("nonexistent");
    let link = temp.path().join("link");

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "link");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    // Should succeed - symlinks can point to nonexistent targets
    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should create symlink to nonexistent target"
    );
    assert!(link.is_symlink(), "Link should be a symlink");
}

// ============================================================================
// Hard Link Tests
// ============================================================================

#[test]
fn test_hardlink_create() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("hardlink");
    fs::write(&target, "target content").unwrap();

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "hard");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should report changed when creating hard link"
    );
    assert!(link.exists(), "Hard link should exist");

    // Verify it's a hard link by checking inodes
    let target_meta = fs::metadata(&target).unwrap();
    let link_meta = fs::metadata(&link).unwrap();
    assert_eq!(
        target_meta.ino(),
        link_meta.ino(),
        "Hard link should have same inode as target"
    );
    assert_eq!(
        target_meta.dev(),
        link_meta.dev(),
        "Hard link should be on same device"
    );
}

#[test]
fn test_hardlink_idempotent() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("hardlink");
    fs::write(&target, "content").unwrap();
    fs::hard_link(&target, &link).unwrap();

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "hard");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not report changed when hard link already exists"
    );
}

#[test]
fn test_hardlink_error_nonexistent_source() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("nonexistent");
    let link = temp.path().join("hardlink");

    let module = FileModule;
    let mut params = params_with_state(link.to_str().unwrap(), "hard");
    params.insert(
        "src".to_string(),
        serde_json::json!(target.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(
        result.is_err(),
        "Should error when hard link source doesn't exist"
    );
}

// ============================================================================
// Diff Tests
// ============================================================================

#[test]
fn test_diff_file_creation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "file");
    let context = ModuleContext::default();

    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some(), "Should provide diff for file creation");
    let d = diff.unwrap();
    assert_eq!(d.before, "absent", "Before should be absent");
    assert_eq!(d.after, "file exists", "After should be file exists");
}

#[test]
fn test_diff_directory_creation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    let path_str = path.to_str().unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "directory");
    let context = ModuleContext::default();

    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some(), "Should provide diff for directory creation");
    let d = diff.unwrap();
    assert_eq!(d.before, "absent");
    assert_eq!(d.after, "directory exists");
}

#[test]
fn test_diff_removal() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "absent");
    let context = ModuleContext::default();

    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some(), "Should provide diff for file removal");
    let d = diff.unwrap();
    assert_eq!(d.before, "file exists");
    assert_eq!(d.after, "absent");
}

#[test]
fn test_diff_no_change() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    let path_str = path.to_str().unwrap();
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = params_with_state(path_str, "file");
    let context = ModuleContext::default();

    let diff = module.diff(&params, &context).unwrap();

    assert!(
        diff.is_none(),
        "Should not provide diff when no change needed"
    );
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_module_name() {
    let module = FileModule;
    assert_eq!(module.name(), "file", "Module name should be 'file'");
}

#[test]
fn test_module_description() {
    let module = FileModule;
    let desc = module.description();
    assert!(
        !desc.is_empty(),
        "Module should have a non-empty description"
    );
    assert!(
        desc.contains("file") || desc.contains("directory"),
        "Description should mention file or directory management"
    );
}

#[test]
fn test_module_required_params() {
    let module = FileModule;
    let required = module.required_params();
    assert_eq!(
        required.len(),
        1,
        "Should have exactly one required parameter"
    );
    assert_eq!(required[0], "path", "Required parameter should be 'path'");
}

#[test]
fn test_module_classification() {
    use rustible::modules::ModuleClassification;

    let module = FileModule;
    let classification = module.classification();
    assert_eq!(
        classification,
        ModuleClassification::NativeTransport,
        "File module should be NativeTransport classification"
    );
}
