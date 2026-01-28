//! Integration tests for the blockinfile module
//!
//! Tests cover:
//! - Inserting blocks into files
//! - Updating existing blocks
//! - Removing blocks
//! - Custom markers
//! - Insert position (before/after)
//! - Create file if absent
//! - Idempotency
//! - Check mode

use rustible::modules::{blockinfile::BlockinfileModule, Module, ModuleContext, ModuleParams};
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

fn with_block(mut params: ModuleParams, block: &str) -> ModuleParams {
    params.insert("block".to_string(), serde_json::json!(block));
    params
}

fn with_state(mut params: ModuleParams, state: &str) -> ModuleParams {
    params.insert("state".to_string(), serde_json::json!(state));
    params
}

fn with_marker(mut params: ModuleParams, marker: &str) -> ModuleParams {
    params.insert("marker".to_string(), serde_json::json!(marker));
    params
}

// ============================================================================
// Basic Block Insertion Tests
// ============================================================================

#[test]
fn test_blockinfile_insert_to_empty() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "line1\nline2\nline3");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when inserting block");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("line1"), "Block content should be present");
    assert!(content.contains("line2"));
    assert!(content.contains("line3"));
    assert!(
        content.contains("BEGIN ANSIBLE MANAGED BLOCK"),
        "Begin marker should be present"
    );
    assert!(
        content.contains("END ANSIBLE MANAGED BLOCK"),
        "End marker should be present"
    );
}

#[test]
fn test_blockinfile_insert_to_existing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "existing content\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new block content");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("existing content"),
        "Existing content should remain"
    );
    assert!(
        content.contains("new block content"),
        "Block should be inserted"
    );
}

#[test]
fn test_blockinfile_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    // File already has the managed block
    fs::write(
        &path,
        "# BEGIN ANSIBLE MANAGED BLOCK\nblock content\n# END ANSIBLE MANAGED BLOCK\n",
    )
    .unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "block content");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not change when block already exists with same content"
    );
}

#[test]
fn test_blockinfile_multiple_runs() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "block content");
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
}

// ============================================================================
// Block Update Tests
// ============================================================================

#[test]
fn test_blockinfile_update_existing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(
        &path,
        "before\n# BEGIN ANSIBLE MANAGED BLOCK\nold content\n# END ANSIBLE MANAGED BLOCK\nafter\n",
    )
    .unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new content");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when updating block");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new content"), "New content should be present");
    assert!(
        !content.contains("old content"),
        "Old content should be replaced"
    );
    assert!(
        content.contains("before"),
        "Content before block should remain"
    );
    assert!(
        content.contains("after"),
        "Content after block should remain"
    );
}

#[test]
fn test_blockinfile_update_multiline() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(
        &path,
        "# BEGIN ANSIBLE MANAGED BLOCK\nold1\nold2\n# END ANSIBLE MANAGED BLOCK\n",
    )
    .unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new1\nnew2\nnew3");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new1"));
    assert!(content.contains("new2"));
    assert!(content.contains("new3"));
    assert!(!content.contains("old1"));
    assert!(!content.contains("old2"));
}

// ============================================================================
// Block Removal Tests
// ============================================================================

#[test]
fn test_blockinfile_remove() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(
        &path,
        "before\n# BEGIN ANSIBLE MANAGED BLOCK\nblock content\n# END ANSIBLE MANAGED BLOCK\nafter\n",
    )
    .unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when removing block");
    let content = fs::read_to_string(&path).unwrap();
    assert!(!content.contains("block content"), "Block should be removed");
    assert!(
        !content.contains("BEGIN ANSIBLE MANAGED BLOCK"),
        "Markers should be removed"
    );
    assert!(
        !content.contains("END ANSIBLE MANAGED BLOCK"),
        "Markers should be removed"
    );
    assert!(content.contains("before"), "Other content should remain");
    assert!(content.contains("after"), "Other content should remain");
}

#[test]
fn test_blockinfile_remove_nonexistent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "no block here\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_state(params, "absent");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not change when block doesn't exist"
    );
}

// ============================================================================
// Custom Marker Tests
// ============================================================================

#[test]
fn test_blockinfile_custom_marker() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "content");
    params = with_marker(params, "# {mark} MY BLOCK");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("# BEGIN MY BLOCK"),
        "Custom begin marker should be used"
    );
    assert!(
        content.contains("# END MY BLOCK"),
        "Custom end marker should be used"
    );
}

#[test]
fn test_blockinfile_update_with_custom_marker() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "# BEGIN CUSTOM\nold\n# END CUSTOM\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new");
    params = with_marker(params, "# {mark} CUSTOM");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new"));
    assert!(!content.contains("old"));
}

// ============================================================================
// Insert Position Tests
// ============================================================================

#[test]
fn test_blockinfile_insertafter() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nmarker\nline3\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "inserted block");
    params.insert("insertafter".to_string(), serde_json::json!("marker"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    // The block should appear after "marker"
    let marker_pos = content.find("marker").unwrap();
    let block_pos = content.find("BEGIN ANSIBLE MANAGED BLOCK").unwrap();
    assert!(block_pos > marker_pos, "Block should be after marker");
}

#[test]
fn test_blockinfile_insertbefore() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "line1\nmarker\nline3\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "inserted block");
    params.insert("insertbefore".to_string(), serde_json::json!("marker"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    // The block should appear before "marker"
    let marker_pos = content.find("marker").unwrap();
    let block_pos = content.find("BEGIN ANSIBLE MANAGED BLOCK").unwrap();
    assert!(block_pos < marker_pos, "Block should be before marker");
}

// ============================================================================
// Create File Tests
// ============================================================================

#[test]
fn test_blockinfile_create() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new_file.txt");

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new block");
    params.insert("create".to_string(), serde_json::json!(true));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.exists(), "File should be created");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("new block"));
}

#[test]
fn test_blockinfile_no_create() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent.txt");

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "block");
    // create defaults to false
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should fail when file doesn't exist and create=false");
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_blockinfile_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "existing\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "new block");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Check mode should report would change");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        !content.contains("new block"),
        "File should not be modified in check mode"
    );
    assert!(
        !content.contains("BEGIN"),
        "Markers should not be added in check mode"
    );
}

#[test]
fn test_blockinfile_check_mode_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(
        &path,
        "# BEGIN ANSIBLE MANAGED BLOCK\nblock\n# END ANSIBLE MANAGED BLOCK\n",
    )
    .unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "block");
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Check mode should report no change for existing block"
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_blockinfile_empty_block() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "content\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "");
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    // Empty block still has markers
    assert!(content.contains("BEGIN ANSIBLE MANAGED BLOCK"));
    assert!(content.contains("END ANSIBLE MANAGED BLOCK"));
}

#[test]
fn test_blockinfile_preserves_file_structure() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.conf");
    fs::write(&path, "[section1]\nkey1=val1\n\n[section2]\nkey2=val2\n").unwrap();

    let module = BlockinfileModule;
    let mut params = with_path(create_params(), path.to_str().unwrap());
    params = with_block(params, "added=content");
    params.insert("insertafter".to_string(), serde_json::json!(r"\[section1\]"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("[section1]"));
    assert!(content.contains("[section2]"));
    assert!(content.contains("key1=val1"));
    assert!(content.contains("key2=val2"));
    assert!(content.contains("added=content"));
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_blockinfile_module_name() {
    let module = BlockinfileModule;
    assert_eq!(module.name(), "blockinfile");
}

#[test]
fn test_blockinfile_required_params() {
    let module = BlockinfileModule;
    let required = module.required_params();
    assert!(required.contains(&"path"));
}
