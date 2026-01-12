//! Comprehensive tests for diff mode output functionality in Rustible.
//!
//! These tests verify the --diff mode output for modules that support it:
//! - Diff output format (before/after/details)
//! - Diff combined with check mode
//! - Diff formatting and edge cases
//!
//! Note: The Module::diff() method was removed in favor of modules handling
//! diff internally via context.diff_mode in their execute() implementation.

use rustible::modules::{
    copy::CopyModule, file::FileModule, template::TemplateModule, Diff, Module, ModuleContext,
    ModuleOutput, ModuleParams,
};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Diff Structure Tests
// ============================================================================

#[test]
fn test_diff_new_basic() {
    let diff = Diff::new("old content", "new content");

    assert_eq!(diff.before, "old content");
    assert_eq!(diff.after, "new content");
    assert!(diff.details.is_none());
}

#[test]
fn test_diff_with_details() {
    let diff = Diff::new("before", "after").with_details("--- a/file\n+++ b/file\n-old\n+new");

    assert_eq!(diff.before, "before");
    assert_eq!(diff.after, "after");
    assert_eq!(
        diff.details,
        Some("--- a/file\n+++ b/file\n-old\n+new".to_string())
    );
}

#[test]
fn test_diff_empty_before() {
    let diff = Diff::new("", "new content");

    assert_eq!(diff.before, "");
    assert_eq!(diff.after, "new content");
}

#[test]
fn test_diff_empty_after() {
    let diff = Diff::new("old content", "");

    assert_eq!(diff.before, "old content");
    assert_eq!(diff.after, "");
}

#[test]
fn test_diff_both_empty() {
    let diff = Diff::new("", "");

    assert_eq!(diff.before, "");
    assert_eq!(diff.after, "");
}

#[test]
fn test_diff_multiline_content() {
    let before = "line1\nline2\nline3";
    let after = "line1\nmodified\nline3\nline4";
    let diff = Diff::new(before, after);

    assert_eq!(diff.before, before);
    assert_eq!(diff.after, after);
}

#[test]
fn test_diff_unicode_content() {
    let before = "Hello \u{1F600}";
    let after = "Hello \u{1F604} World \u{4E2D}\u{6587}";
    let diff = Diff::new(before, after);

    assert_eq!(diff.before, before);
    assert_eq!(diff.after, after);
}

#[test]
fn test_diff_whitespace_only_changes() {
    let before = "line1\nline2";
    let after = "line1  \n  line2";
    let diff = Diff::new(before, after);

    assert_eq!(diff.before, before);
    assert_eq!(diff.after, after);
    assert_ne!(diff.before, diff.after);
}

// ============================================================================
// ModuleOutput with Diff Tests
// ============================================================================

#[test]
fn test_module_output_with_diff() {
    let diff = Diff::new("old", "new");
    let output = ModuleOutput::changed("Content updated").with_diff(diff);

    assert!(output.changed);
    assert!(output.diff.is_some());
    let d = output.diff.unwrap();
    assert_eq!(d.before, "old");
    assert_eq!(d.after, "new");
}

#[test]
fn test_module_output_without_diff() {
    let output = ModuleOutput::ok("No changes");

    assert!(!output.changed);
    assert!(output.diff.is_none());
}

#[test]
fn test_module_output_diff_with_data() {
    let diff = Diff::new("before", "after");
    let output = ModuleOutput::changed("Updated")
        .with_diff(diff)
        .with_data("file", serde_json::json!("/tmp/test.txt"))
        .with_data("size", serde_json::json!(1024));

    assert!(output.diff.is_some());
    assert_eq!(output.data.len(), 2);
}

// ============================================================================
// ModuleContext Diff Mode Tests
// ============================================================================

#[test]
fn test_context_diff_mode_disabled_by_default() {
    let ctx = ModuleContext::default();

    assert!(!ctx.diff_mode);
    assert!(!ctx.check_mode);
}

#[test]
fn test_context_diff_mode_enabled() {
    let ctx = ModuleContext::default().with_diff_mode(true);

    assert!(ctx.diff_mode);
}

#[test]
fn test_context_check_and_diff_mode() {
    let ctx = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    assert!(ctx.check_mode);
    assert!(ctx.diff_mode);
}

// ============================================================================
// Copy Module Check Mode with Diff Tests
// ============================================================================

#[test]
fn test_copy_check_mode_with_diff() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("check_diff.txt");

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
    assert!(result.msg.contains("Would copy"));
    assert!(result.diff.is_some());

    let diff = result.diff.unwrap();
    assert_eq!(diff.before, "old content");
    assert_eq!(diff.after, "new content");

    // Verify file was not modified
    assert_eq!(fs::read_to_string(&dest).unwrap(), "old content");
}

// ============================================================================
// Template Module Check Mode with Diff Tests
// ============================================================================

#[test]
fn test_template_check_mode_with_diff() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Value: {{ value }}").unwrap();
    fs::write(&dest, "Value: old").unwrap();

    let module = TemplateModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("value".to_string(), serde_json::json!("new"));

    let context = ModuleContext::default()
        .with_vars(vars)
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would render"));
    assert!(result.diff.is_some());

    // Verify original file unchanged
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Value: old");
}

// ============================================================================
// File Module Check Mode with Diff Tests
// ============================================================================

#[test]
fn test_file_check_mode_with_diff_create() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("newdir");

    let module = FileModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would create"));
    assert!(!path.exists());
}

#[test]
fn test_file_check_mode_with_diff_remove() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("toremove");
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would remove"));
    assert!(result.diff.is_some());
    // File should still exist
    assert!(path.exists());
}

// ============================================================================
// Diff with Check Mode Integration Tests
// ============================================================================

#[test]
fn test_check_diff_shows_what_would_change() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "original").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("modified"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.diff.is_some());

    let diff = result.diff.unwrap();
    assert_eq!(diff.before, "original");
    assert_eq!(diff.after, "modified");

    // Verify no actual change was made
    assert_eq!(fs::read_to_string(&dest).unwrap(), "original");
}

#[test]
fn test_check_diff_no_change_needed() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "same").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("same"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

// ============================================================================
// Diff Serialization Tests
// ============================================================================

#[test]
fn test_diff_serialization() {
    let diff = Diff::new("before state", "after state");
    let serialized = serde_json::to_string(&diff).unwrap();

    assert!(serialized.contains("before"));
    assert!(serialized.contains("after"));
    assert!(serialized.contains("before state"));
    assert!(serialized.contains("after state"));
}

#[test]
fn test_diff_with_details_serialization() {
    let diff = Diff::new("before", "after").with_details("detailed diff output");
    let serialized = serde_json::to_string(&diff).unwrap();

    assert!(serialized.contains("details"));
    assert!(serialized.contains("detailed diff output"));
}

#[test]
fn test_module_output_with_diff_serialization() {
    let diff = Diff::new("old", "new");
    let output = ModuleOutput::changed("Updated content").with_diff(diff);

    let serialized = serde_json::to_string(&output).unwrap();

    assert!(serialized.contains("diff"));
    assert!(serialized.contains("old"));
    assert!(serialized.contains("new"));
}

// ============================================================================
// Diff Mode Flag Tests
// ============================================================================

#[test]
fn test_diff_mode_disabled_no_diff_in_output() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "old").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("new"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    // Check mode but diff mode disabled
    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(false);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    // When diff_mode is false, check mode output should not include diff
    assert!(result.diff.is_none());
}

#[test]
fn test_diff_mode_enabled_includes_diff() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "old").unwrap();

    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("new"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.diff.is_some());
}

// ============================================================================
// Integration: Full Workflow Tests
// ============================================================================

#[test]
fn test_full_diff_workflow_copy() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("workflow.txt");

    // Step 1: Create new file (check mode with diff)
    let module = CopyModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("initial content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let check_context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let check_result = module.execute(&params, &check_context).unwrap();
    assert!(check_result.changed);
    assert!(check_result.diff.is_some());
    assert!(!dest.exists()); // File not created in check mode

    // Step 2: Actually create the file
    let exec_context = ModuleContext::default();
    let exec_result = module.execute(&params, &exec_context).unwrap();
    assert!(exec_result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "initial content");

    // Step 3: Modify with diff
    params.insert("content".to_string(), serde_json::json!("modified content"));

    let modify_check = module.execute(&params, &check_context).unwrap();
    assert!(modify_check.changed);
    assert!(modify_check.diff.is_some());
    let diff = modify_check.diff.unwrap();
    assert_eq!(diff.before, "initial content");
    assert_eq!(diff.after, "modified content");

    // Step 4: Actually modify
    let modify_result = module.execute(&params, &exec_context).unwrap();
    assert!(modify_result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "modified content");

    // Step 5: No change needed
    let no_change_check = module.execute(&params, &check_context).unwrap();
    assert!(!no_change_check.changed);
}

#[test]
fn test_full_diff_workflow_template() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("config.j2");
    let dest = temp.path().join("config.txt");

    fs::write(&src, "port={{ port }}\nhost={{ host }}").unwrap();

    let module = TemplateModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("port".to_string(), serde_json::json!(8080));
    vars.insert("host".to_string(), serde_json::json!("localhost"));

    let context = ModuleContext::default()
        .with_vars(vars)
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.diff.is_some());
    let diff = result.diff.unwrap();
    assert_eq!(diff.before, "");
    assert_eq!(diff.after, "port=8080\nhost=localhost");
}

#[test]
fn test_full_diff_workflow_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("managed_dir");

    let module = FileModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    // Check mode first
    let check_context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let check_result = module.execute(&params, &check_context).unwrap();
    assert!(check_result.changed);
    assert!(check_result.msg.contains("Would create"));
    assert!(!path.exists());

    // Actually create
    let exec_context = ModuleContext::default();
    let exec_result = module.execute(&params, &exec_context).unwrap();
    assert!(exec_result.changed);
    assert!(path.is_dir());

    // Idempotent check
    let idempotent_result = module.execute(&params, &exec_context).unwrap();
    assert!(!idempotent_result.changed);

    // Remove check
    params.insert("state".to_string(), serde_json::json!("absent"));
    let remove_check = module.execute(&params, &check_context).unwrap();
    assert!(remove_check.changed);
    assert!(remove_check.msg.contains("Would remove"));
    assert!(path.exists()); // Still exists after check mode
}
