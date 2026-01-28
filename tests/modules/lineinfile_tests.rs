//! Integration tests for the lineinfile module
//!
//! Tests cover:
//! - Adding lines to files
//! - Removing lines from files
//! - Regexp-based line matching and replacement
//! - Insert position (before/after)
//! - Backreferences
//! - Create file if absent
//! - Idempotency
//! - Check mode

use rustible::modules::{lineinfile::LineinfileModule, Module, ModuleContext, ModuleParams};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_path(mut params: ModuleParams, path: &str) -> ModuleParams {
    params.insert("path".to_string(), serde_json::json!(path));
    params
}

fn with_line(mut params: ModuleParams, line: &str) -> ModuleParams {
    params.insert("line".to_string(), serde_json::json!(line));
    params
}

fn with_state(mut params: ModuleParams, state: &str) -> ModuleParams {
    params.insert("state".to_string(), serde_json::json!(state));
    params
}

fn with_regexp(mut params: ModuleParams, regexp: &str) -> ModuleParams {
    params.insert("regexp".to_string(), serde_json::json!(regexp));
    params
}

// ============================================================================
// Basic Line Addition Tests
// ============================================================================

#[test]
fn test_lineinfile_add_line_to_empty_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "new line");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when adding line");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new line"), "File should contain new line");
}

#[test]
fn test_lineinfile_add_line_to_existing_content() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "new line");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new line"));
    assert!(content.contains("line1"));
}

#[test]
fn test_lineinfile_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "existing line\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "existing line");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not change when line already exists"
    );
}

#[test]
fn test_lineinfile_multiple_runs() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "test line");
    let context = ModuleContext::default();

    // First run - should change
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run - should be idempotent
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);

    // Third run - still idempotent
    let result3 = module.execute(&params, &context).unwrap();
    assert!(!result3.changed);

    // Content should have line only once
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content.matches("test line").count(), 1);
}

// ============================================================================
// Line Removal Tests
// ============================================================================

#[test]
fn test_lineinfile_remove_line() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "keep this\nremove me\nkeep this too\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "remove me");
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when removing line");
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("remove me"), "Line should be removed");
    assert!(content.contains("keep this"), "Other lines should remain");
    assert!(
        content.contains("keep this too"),
        "Other lines should remain"
    );
}

#[test]
fn test_lineinfile_remove_nonexistent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nline2\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "nonexistent line");
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed, "Should not change when line doesn't exist");
}

#[test]
fn test_lineinfile_remove_all_matching() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "dup\nother\ndup\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "dup");
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("dup"), "All matching lines should be removed");
    assert!(content.contains("other"), "Non-matching lines should remain");
}

// ============================================================================
// Regexp Tests
// ============================================================================

#[test]
fn test_lineinfile_regexp_replace() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(&path, "setting=old_value\nother=123\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_regexp(params, "^setting=.*");
    params = with_line(params, "setting=new_value");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("setting=new_value"),
        "Line should be replaced"
    );
    assert!(
        !content.contains("setting=old_value"),
        "Old value should be gone"
    );
    assert!(content.contains("other=123"), "Other lines should remain");
}

#[test]
fn test_lineinfile_regexp_remove() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(&path, "# comment\nkeep=value\nremove=this\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_regexp(params, "^remove=.*");
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("remove="), "Matched line should be removed");
    assert!(content.contains("keep=value"), "Other lines should remain");
    assert!(content.contains("# comment"), "Comments should remain");
}

#[test]
fn test_lineinfile_regexp_no_match_adds_line() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(&path, "other=123\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_regexp(params, "^setting=.*");
    params = with_line(params, "setting=default");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("setting=default"),
        "New line should be added when no match"
    );
}

// ============================================================================
// Insert Position Tests
// ============================================================================

#[test]
fn test_lineinfile_insertafter() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nmarker\nline3\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "inserted");
    params.insert("insertafter".to_string(), serde_json::json!("marker"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let marker_idx = lines.iter().position(|l| *l == "marker").unwrap();
    let inserted_idx = lines.iter().position(|l| *l == "inserted").unwrap();
    assert_eq!(
        inserted_idx,
        marker_idx + 1,
        "Line should be inserted after marker"
    );
}

#[test]
fn test_lineinfile_insertbefore() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nmarker\nline3\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "inserted");
    params.insert("insertbefore".to_string(), serde_json::json!("marker"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let marker_idx = lines.iter().position(|l| *l == "marker").unwrap();
    let inserted_idx = lines.iter().position(|l| *l == "inserted").unwrap();
    assert_eq!(
        inserted_idx + 1,
        marker_idx,
        "Line should be inserted before marker"
    );
}

// ============================================================================
// Create File Tests
// ============================================================================

#[test]
fn test_lineinfile_create_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new_file.txt");

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "new line");
    params.insert("create".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.exists(), "File should be created");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new line"));
}

#[test]
fn test_lineinfile_no_create_missing_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent.txt");

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "new line");
    // create defaults to false
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should fail when file doesn't exist and create=false");
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_lineinfile_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "existing\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "new line");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Check mode should report would change");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        !content.contains("new line"),
        "File should not be modified in check mode"
    );
}

#[test]
fn test_lineinfile_check_mode_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "existing line\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_line(params, "existing line");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Check mode should report no change for existing line"
    );
}

// ============================================================================
// Backreference Tests
// ============================================================================

#[test]
fn test_lineinfile_backrefs() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(&path, "setting=value123\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_regexp(params, r"^setting=(\w+)");
    params = with_line(params, r"setting=modified_\1");
    params.insert("backrefs".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("setting=modified_value123"),
        "Backreference should be applied"
    );
}

// ============================================================================
// First Match Tests
// ============================================================================

#[test]
fn test_lineinfile_firstmatch() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "match\nother\nmatch\n").unwrap();

    let module = LineinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_regexp(params, "^match$");
    params = with_line(params, "replaced");
    params.insert("firstmatch".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    // Only the first match should be replaced
    assert!(content.contains("replaced"));
    assert!(content.contains("match"), "Second match should remain");
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_lineinfile_module_name() {
    let module = LineinfileModule;
    assert_eq!(module.name(), "lineinfile");
}

#[test]
fn test_lineinfile_required_params() {
    let module = LineinfileModule;
    let required = module.required_params();
    assert!(required.contains(&"path"));
}
