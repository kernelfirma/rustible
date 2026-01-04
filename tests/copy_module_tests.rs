//! Comprehensive tests for the copy module
//!
//! This test suite validates the copy module functionality including:
//! - Copy with src/dest (file copying)
//! - Copy with content (direct content writing)
//! - Mode/owner/group permissions
//! - Idempotency (no change on second run)
//! - Backup functionality
//! - Check mode behavior
//! - Diff generation
//! - Error handling
//! - Edge cases

use rustible::modules::{copy::CopyModule, Module, ModuleContext, ModuleError, ModuleParams};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

// ============================================================================
// Copy with src/dest tests
// ============================================================================

#[test]
fn test_copy_with_src_dest_basic() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "Source file content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Source file content");
    assert!(result.msg.contains("Copied"));
}

#[test]
fn test_copy_with_src_dest_large_file() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("large.txt");
    let dest = temp.path().join("dest_large.txt");

    // Create a large file
    let large_content = "x".repeat(10_000);
    fs::write(&src, &large_content).unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), large_content);
}

#[test]
fn test_copy_with_src_to_directory() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest_dir = temp.path().join("destdir");

    fs::write(&src, "source content").unwrap();
    fs::create_dir(&dest_dir).unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest_dir.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let expected_dest = dest_dir.join("source.txt");
    assert!(expected_dest.exists());
    assert_eq!(
        fs::read_to_string(&expected_dest).unwrap(),
        "source content"
    );
}

#[test]
fn test_copy_with_nonexistent_src() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("dest.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "src".to_string(),
        serde_json::json!("/nonexistent/source.txt"),
    );
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));
}

#[test]
fn test_copy_creates_parent_directories() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("subdir").join("nested").join("test.txt");

    fs::write(&src, "content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "content");
}

// ============================================================================
// Copy with content tests
// ============================================================================

#[test]
fn test_copy_with_content_basic() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("Hello, World!"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
}

#[test]
fn test_copy_with_content_multiline() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("multiline.txt");

    let content = "Line 1\nLine 2\nLine 3\n";

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!(content));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

#[test]
fn test_copy_with_content_empty_string() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("empty.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!(""));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "");
}

#[test]
fn test_copy_with_content_special_characters() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("special.txt");

    let content = "Special chars: !@#$%^&*()_+-={}[]|\\:\";<>?,./~`";

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!(content));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

// ============================================================================
// Mode/owner/group tests
// ============================================================================

#[test]
fn test_copy_with_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o755));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let meta = fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
}

#[test]
fn test_copy_with_mode_644() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o644));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let meta = fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o644);
}

#[test]
fn test_copy_mode_change_only() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");
    fs::write(&dest, "content").unwrap();

    // Set initial mode to 644
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o755));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("permissions"));
    let meta = fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
}

#[test]
fn test_copy_with_executable_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("script.sh");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "content".to_string(),
        serde_json::json!("#!/bin/bash\necho test"),
    );
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o755));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let meta = fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
    // Verify it's executable
    assert!(meta.permissions().mode() & 0o111 != 0);
}

// ============================================================================
// Idempotency tests (no change on second run)
// ============================================================================

#[test]
fn test_copy_idempotent_with_content() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Same content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("Same content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("already up to date"));
}

#[test]
fn test_copy_idempotent_with_src() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "Same content").unwrap();
    fs::write(&dest, "Same content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_copy_idempotent_with_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    // First copy with mode
    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o644));

    let context = ModuleContext::default();
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second copy with same mode - should be idempotent
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
    assert!(result2.msg.contains("already up to date"));
}

#[test]
fn test_copy_idempotent_run_twice() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o644));

    let context = ModuleContext::default();

    // First run - should change
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run - should be idempotent
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

#[test]
fn test_copy_detects_content_change() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("New content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "New content");
}

// ============================================================================
// Additional functionality tests
// ============================================================================

#[test]
fn test_copy_with_backup() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    // Create existing file
    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("New content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("backup".to_string(), serde_json::json!(true));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.data.contains_key("backup_file"));

    let backup_path = temp.path().join("test.txt~");
    assert!(backup_path.exists());
    assert_eq!(fs::read_to_string(&backup_path).unwrap(), "Old content");
    assert_eq!(fs::read_to_string(&dest).unwrap(), "New content");
}

#[test]
fn test_copy_with_backup_custom_suffix() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("New content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("backup".to_string(), serde_json::json!(true));
    params.insert("backup_suffix".to_string(), serde_json::json!(".bak"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let backup_path = temp.path().join("test.txt.bak");
    assert!(backup_path.exists());
    assert_eq!(fs::read_to_string(&backup_path).unwrap(), "Old content");
}

#[test]
fn test_copy_backup_no_existing_file() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("New content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("backup".to_string(), serde_json::json!(true));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    // No backup should be created if file didn't exist
    let backup_path = temp.path().join("test.txt~");
    assert!(!backup_path.exists());
}

#[test]
fn test_copy_check_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("Hello"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would copy"));
    assert!(!dest.exists()); // File should not be created in check mode
}

#[test]
fn test_copy_check_mode_existing_file() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("New content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would copy"));
    // Original content should remain
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Old content");
}

#[test]
fn test_copy_diff_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("new content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.diff.is_some());
    let diff = result.diff.unwrap();
    assert_eq!(diff.before, "old content");
    assert_eq!(diff.after, "new content");
}

#[test]
fn test_copy_diff_function() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");
    fs::write(&dest, "old content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("new content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
    let d = diff.unwrap();
    assert_eq!(d.before, "old content");
    assert_eq!(d.after, "new content");
}

#[test]
fn test_copy_diff_for_src_file() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "source content").unwrap();
    fs::write(&dest, "old dest content").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
    let d = diff.unwrap();
    assert_eq!(d.before, "old dest content");
    assert_eq!(d.after, "source content");
}

#[test]
fn test_copy_output_contains_metadata() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.data.contains_key("dest"));
    assert!(result.data.contains_key("size"));
    assert!(result.data.contains_key("mode"));
    assert!(result.data.contains_key("uid"));
    assert!(result.data.contains_key("gid"));
}

// ============================================================================
// Error handling tests
// ============================================================================

#[test]
fn test_copy_missing_src_and_content() {
    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

    let result = module.validate_params(&params);
    assert!(result.is_err());
    match result {
        Err(ModuleError::MissingParameter(msg)) => {
            assert!(msg.contains("src") || msg.contains("content"));
        }
        _ => panic!("Expected MissingParameter error"),
    }
}

#[test]
fn test_copy_missing_dest() {
    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));

    let result = module.validate_params(&params);
    assert!(result.is_err());
    match result {
        Err(ModuleError::MissingParameter(msg)) => {
            assert!(msg.contains("dest"));
        }
        _ => panic!("Expected MissingParameter error"),
    }
}

// ============================================================================
// Module trait tests
// ============================================================================

#[test]
fn test_copy_module_name() {
    let module = CopyModule;
    assert_eq!(module.name(), "copy");
}

#[test]
fn test_copy_module_description() {
    let module = CopyModule;
    assert_eq!(module.description(), "Copy files to a destination");
}

#[test]
fn test_copy_module_classification() {
    use rustible::modules::ModuleClassification;

    let module = CopyModule;
    assert_eq!(
        module.classification(),
        ModuleClassification::NativeTransport
    );
}
