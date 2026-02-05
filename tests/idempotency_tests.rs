//! Comprehensive idempotency tests for Rustible modules
//!
//! These tests verify that all modules are properly idempotent - running twice
//! produces no changes on the second run (for modules that should be idempotent).
//!
//! IDEMPOTENCY RULES:
//! - Idempotent modules: Running the same operation twice should result in
//!   `changed: true` on first run and `changed: false` on second run.
//! - Non-idempotent modules (command, shell): Always report `changed: true`
//!   unless `creates` or `removes` conditions are met.
//!
//! MODULE CLASSIFICATION:
//! - file: Idempotent (except state: touch which always changes timestamps)
//! - copy: Idempotent (compares checksums)
//! - template: Idempotent (compares rendered content)
//! - lineinfile: Idempotent (checks if line exists)
//! - package: Idempotent (checks if package is installed) - requires system access
//! - service: Idempotent (checks service state) - requires system access
//! - user: Idempotent (checks if user exists) - requires root access
//! - command: NON-idempotent (always changed unless creates/removes)
//! - shell: NON-idempotent (always changed unless creates/removes)

use rustible::modules::{
    command::CommandModule, copy::CopyModule, file::FileModule, lineinfile::LineinfileModule,
    shell::ShellModule, template::TemplateModule, Module, ModuleContext, ModuleParams,
};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

// ============================================================================
// Helper Functions
// ============================================================================

/// Run a module twice and verify idempotency
/// First run should be changed=true (or unchanged if already in desired state)
/// Second run should be changed=false (idempotent)
fn assert_idempotent(
    module: &dyn Module,
    params: &ModuleParams,
    context: &ModuleContext,
    expect_first_changed: bool,
) {
    // First execution
    let result1 = module
        .execute(params, context)
        .expect("First execution should succeed");
    assert_eq!(
        result1.changed, expect_first_changed,
        "First run: expected changed={}, got changed={}. Message: {}",
        expect_first_changed, result1.changed, result1.msg
    );

    // Second execution - should be idempotent (no change)
    let result2 = module
        .execute(params, context)
        .expect("Second execution should succeed");
    assert!(
        !result2.changed,
        "Second run should be idempotent (changed=false), but got changed=true. Message: {}",
        result2.msg
    );
}

/// Run a module twice and verify it's always changing (non-idempotent)
fn assert_non_idempotent(module: &dyn Module, params: &ModuleParams, context: &ModuleContext) {
    // First execution
    let result1 = module
        .execute(params, context)
        .expect("First execution should succeed");
    assert!(
        result1.changed,
        "Non-idempotent module should always change on first run. Message: {}",
        result1.msg
    );

    // Second execution - should still report changed
    let result2 = module
        .execute(params, context)
        .expect("Second execution should succeed");
    assert!(
        result2.changed,
        "Non-idempotent module should always change on second run. Message: {}",
        result2.msg
    );
}

// ============================================================================
// FILE MODULE IDEMPOTENCY TESTS
// ============================================================================

mod file_module {
    use super::*;

    #[test]
    fn test_file_state_file_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("file"));

        let context = ModuleContext::default();

        // First run - creates the file
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create file");
        assert!(path.exists(), "File should exist after first run");
        assert!(path.is_file(), "Path should be a file");

        // Second run - file already exists, should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_state_directory_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default();

        // First run - creates the directory
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create directory");
        assert!(path.exists(), "Directory should exist after first run");
        assert!(path.is_dir(), "Path should be a directory");

        // Second run - directory already exists, should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_state_directory_recursive_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("parent").join("child").join("grandchild");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));
        params.insert("recurse".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
        assert!(path.is_dir(), "All directories should be created");
    }

    #[test]
    fn test_file_state_absent_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        // Create the file first
        fs::write(&path, "content").unwrap();
        assert!(path.exists(), "File should exist before test");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();

        // First run - removes the file
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should remove file");
        assert!(!path.exists(), "File should not exist after first run");

        // Second run - file already absent, should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_state_absent_directory_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        // Create the directory first
        fs::create_dir(&path).unwrap();
        assert!(path.is_dir(), "Directory should exist before test");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("absent"));
        params.insert("recurse".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
        assert!(!path.exists(), "Directory should be removed");
    }

    #[test]
    fn test_file_mode_change_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        // Create file first
        fs::write(&path, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("file"));
        params.insert("mode".to_string(), serde_json::json!(0o755));

        let context = ModuleContext::default();

        // First run - changes mode
        let _result1 = module.execute(&params, &context).unwrap();
        // May or may not be changed depending on default mode
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755);

        // Second run - mode already correct, should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_symlink_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source");
        let dest = temp.path().join("link");

        // Create source file
        fs::write(&src, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert("state".to_string(), serde_json::json!("link"));

        let context = ModuleContext::default();

        // First run - creates symlink
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create symlink");
        assert!(dest.is_symlink(), "Symlink should exist");
        assert_eq!(fs::read_link(&dest).unwrap(), src);

        // Second run - symlink already points to correct target
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_hardlink_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source");
        let dest = temp.path().join("hardlink");

        // Create source file
        fs::write(&src, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert("state".to_string(), serde_json::json!("hard"));

        let context = ModuleContext::default();

        // First run - creates hard link
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create hard link");
        assert!(dest.exists(), "Hard link should exist");

        // Second run - hard link already exists to same inode
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_file_state_touch_not_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("touchfile");

        // Create the file first
        fs::write(&path, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("touch"));

        let context = ModuleContext::default();

        // Touch always reports changed because it updates timestamps
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "Touch should always report changed");

        // Even second touch reports changed
        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            result2.changed,
            "Touch should always report changed (not idempotent)"
        );
    }

    #[test]
    fn test_file_already_exists_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("existing_file");

        // Pre-create the file
        fs::write(&path, "").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("file"));

        let context = ModuleContext::default();

        // File already exists, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should be unchanged when file already exists"
        );

        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should also be unchanged");
    }

    #[test]
    fn test_directory_already_exists_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("existing_dir");

        // Pre-create the directory
        fs::create_dir(&path).unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default();

        // Directory already exists, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should be unchanged when directory already exists"
        );

        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should also be unchanged");
    }
}

// ============================================================================
// COPY MODULE IDEMPOTENCY TESTS
// ============================================================================

mod copy_module {
    use super::*;

    #[test]
    fn test_copy_file_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source.txt");
        let dest = temp.path().join("dest.txt");

        fs::write(&src, "Hello, World!").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_copy_content_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dest.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello, World!"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_copy_with_mode_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dest.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("mode".to_string(), serde_json::json!(0o644));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let meta = fs::metadata(&dest).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o644);
    }

    #[test]
    fn test_copy_same_content_already_exists_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source.txt");
        let dest = temp.path().join("dest.txt");

        // Create source and dest with same content
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

        // Content already matches, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(!result1.changed, "Should be unchanged when content matches");

        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should also be unchanged");
    }

    #[test]
    fn test_copy_content_same_already_exists_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dest.txt");

        // Pre-create dest with the content we'll be "copying"
        fs::write(&dest, "Existing content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Existing content"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // Content already matches, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(!result1.changed, "Should be unchanged when content matches");
    }

    #[test]
    fn test_copy_different_content_changes() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dest.txt");

        // Pre-create dest with different content
        fs::write(&dest, "Old content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("New content"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // Content differs, should change first time
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "Should change when content differs");
        assert_eq!(fs::read_to_string(&dest).unwrap(), "New content");

        // Now idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_copy_mode_change_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dest.txt");

        // Create file with default mode
        fs::write(&dest, "content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("content"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("mode".to_string(), serde_json::json!(0o755));

        let context = ModuleContext::default();

        // First run may or may not change depending on current mode
        let _result1 = module.execute(&params, &context).unwrap();
        let meta = fs::metadata(&dest).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755);

        // Second run should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }
}

// ============================================================================
// TEMPLATE MODULE IDEMPOTENCY TESTS
// ============================================================================

mod template_module {
    use super::*;

    #[test]
    fn test_template_basic_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Hello, {{ name }}!").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("World"));

        let context = ModuleContext::default().with_vars(vars);

        // First run - creates the file
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create file");
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");

        // Second run - content matches, should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    #[test]
    fn test_template_with_same_vars_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Name: {{ name }}\nAge: {{ age }}").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("John"));
        vars.insert("age".to_string(), serde_json::json!(30));

        let context = ModuleContext::default().with_vars(vars);
        assert_idempotent(&module, &params, &context, true);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Name: John\nAge: 30");
    }

    #[test]
    fn test_template_already_correct_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Hello, {{ name }}!").unwrap();
        fs::write(&dest, "Hello, World!").unwrap(); // Pre-create with correct content

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("World"));

        let context = ModuleContext::default().with_vars(vars);

        // Already correct, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(!result1.changed, "Should be unchanged when content matches");
    }

    #[test]
    fn test_template_with_loop_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "{% for item in items %}{{ item }}\n{% endfor %}").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert(
            "items".to_string(),
            serde_json::json!(["one", "two", "three"]),
        );

        let context = ModuleContext::default().with_vars(vars);
        assert_idempotent(&module, &params, &context, true);
    }

    #[test]
    fn test_template_with_conditional_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "{% if enabled %}ON{% else %}OFF{% endif %}").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("enabled".to_string(), serde_json::json!(true));

        let context = ModuleContext::default().with_vars(vars);
        assert_idempotent(&module, &params, &context, true);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "ON");
    }

    #[test]
    fn test_template_with_mode_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "content").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("mode".to_string(), serde_json::json!(0o600));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let meta = fs::metadata(&dest).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
    }
}

// ============================================================================
// LINEINFILE MODULE IDEMPOTENCY TESTS
// ============================================================================

mod lineinfile_module {
    use super::*;

    #[test]
    fn test_lineinfile_insert_line_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line3"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("line3"));
    }

    #[test]
    fn test_lineinfile_line_already_present_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line2"));

        let context = ModuleContext::default();

        // Line already exists, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should be unchanged when line already exists"
        );

        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed, "Second run should also be unchanged");
    }

    #[test]
    fn test_lineinfile_replace_line_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "key=old_value\nother=stuff\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("regexp".to_string(), serde_json::json!("^key="));
        params.insert("line".to_string(), serde_json::json!("key=new_value"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("key=new_value"));
        assert!(!content.contains("key=old_value"));
    }

    #[test]
    fn test_lineinfile_state_absent_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line2"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("line2"));
    }

    #[test]
    fn test_lineinfile_absent_line_not_present_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line2"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();

        // Line doesn't exist, should be idempotent from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should be unchanged when line not present"
        );
    }

    #[test]
    fn test_lineinfile_insertafter_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("insertafter".to_string(), serde_json::json!("^line1"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let lines: Vec<_> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines[1], "new_line");
    }

    #[test]
    fn test_lineinfile_insertbefore_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("insertbefore".to_string(), serde_json::json!("^line3"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
    }

    #[test]
    fn test_lineinfile_create_file_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("new_file.txt");

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("create".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new_line"));
    }

    #[test]
    fn test_lineinfile_regexp_absent_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        fs::write(&path, "# comment1\nkey=value\n# comment2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("regexp".to_string(), serde_json::json!("^#"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("#"));
        assert!(content.contains("key=value"));
    }
}

// ============================================================================
// COMMAND MODULE NON-IDEMPOTENCY TESTS
// ============================================================================

mod command_module {
    use super::*;

    #[test]
    fn test_command_always_changed() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));
        params.insert("shell_type".to_string(), serde_json::json!("posix"));

        let context = ModuleContext::default();
        assert_non_idempotent(&module, &params, &context);
    }

    #[test]
    fn test_command_with_creates_becomes_idempotent() {
        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker_file");

        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cmd".to_string(),
            serde_json::json!(format!("touch {}", marker.display())),
        );
        params.insert(
            "creates".to_string(),
            serde_json::json!(marker.to_str().unwrap()),
        );
        params.insert("shell_type".to_string(), serde_json::json!("posix"));

        let context = ModuleContext::default();

        // First run - file doesn't exist, command runs
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should execute command");

        // Second run - file exists, command is skipped
        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            !result2.changed,
            "Second run should skip (creates file exists)"
        );
        assert!(result2.msg.contains("Skipped"));
    }

    #[test]
    fn test_command_with_creates_already_exists() {
        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker_file");

        // Create the marker file first
        fs::write(&marker, "").unwrap();

        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo should not run"));
        params.insert(
            "creates".to_string(),
            serde_json::json!(marker.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // File already exists, should skip from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(!result1.changed, "Should skip when creates file exists");
        assert!(result1.msg.contains("Skipped"));
    }

    #[test]
    fn test_command_with_removes_becomes_idempotent() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("target_file");

        // Create the file that will be removed
        fs::write(&target, "content").unwrap();

        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cmd".to_string(),
            serde_json::json!(format!("rm {}", target.display())),
        );
        params.insert(
            "removes".to_string(),
            serde_json::json!(target.to_str().unwrap()),
        );
        params.insert("shell_type".to_string(), serde_json::json!("posix"));

        let context = ModuleContext::default();

        // First run - file exists, command runs
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should execute command");
        assert!(!target.exists(), "File should be removed");

        // Second run - file doesn't exist, command is skipped
        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            !result2.changed,
            "Second run should skip (removes file doesn't exist)"
        );
        assert!(result2.msg.contains("Skipped"));
    }

    #[test]
    fn test_command_with_removes_file_not_present() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("nonexistent_file");

        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo should not run"));
        params.insert(
            "removes".to_string(),
            serde_json::json!(target.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // File doesn't exist, should skip from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should skip when removes file doesn't exist"
        );
        assert!(result1.msg.contains("Skipped"));
    }
}

// ============================================================================
// SHELL MODULE NON-IDEMPOTENCY TESTS
// ============================================================================

mod shell_module {
    use super::*;

    #[test]
    fn test_shell_always_changed() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));

        let context = ModuleContext::default();
        assert_non_idempotent(&module, &params, &context);
    }

    #[test]
    fn test_shell_with_pipe_always_changed() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello | cat"));

        let context = ModuleContext::default();
        assert_non_idempotent(&module, &params, &context);
    }

    #[test]
    fn test_shell_with_creates_becomes_idempotent() {
        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker_file");

        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cmd".to_string(),
            serde_json::json!(format!("touch {}", marker.display())),
        );
        params.insert(
            "creates".to_string(),
            serde_json::json!(marker.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // First run - file doesn't exist, command runs
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should execute command");

        // Second run - file exists, command is skipped
        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            !result2.changed,
            "Second run should skip (creates file exists)"
        );
        assert!(result2.msg.contains("Skipped"));
    }

    #[test]
    fn test_shell_with_creates_already_exists() {
        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker_file");

        // Create the marker file first
        fs::write(&marker, "").unwrap();

        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo should not run"));
        params.insert(
            "creates".to_string(),
            serde_json::json!(marker.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // File already exists, should skip from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(!result1.changed, "Should skip when creates file exists");
        assert!(result1.msg.contains("Skipped"));
    }

    #[test]
    fn test_shell_with_removes_becomes_idempotent() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("target_file");

        // Create the file that will be removed
        fs::write(&target, "content").unwrap();

        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cmd".to_string(),
            serde_json::json!(format!("rm {}", target.display())),
        );
        params.insert(
            "removes".to_string(),
            serde_json::json!(target.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // First run - file exists, command runs
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should execute command");
        assert!(!target.exists(), "File should be removed");

        // Second run - file doesn't exist, command is skipped
        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            !result2.changed,
            "Second run should skip (removes file doesn't exist)"
        );
        assert!(result2.msg.contains("Skipped"));
    }

    #[test]
    fn test_shell_with_removes_file_not_present() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("nonexistent_file");

        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo should not run"));
        params.insert(
            "removes".to_string(),
            serde_json::json!(target.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // File doesn't exist, should skip from the start
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            !result1.changed,
            "Should skip when removes file doesn't exist"
        );
        assert!(result1.msg.contains("Skipped"));
    }
}

// ============================================================================
// IDEMPOTENCY EDGE CASES AND REGRESSION TESTS
// ============================================================================

mod edge_cases {
    use super::*;

    /// Test that running the same operation many times remains idempotent
    #[test]
    fn test_multiple_idempotent_runs() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default();

        // First run creates
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "First run should create directory");

        // Run 5 more times - all should be idempotent
        for i in 2..=6 {
            let result = module.execute(&params, &context).unwrap();
            assert!(!result.changed, "Run {} should be idempotent", i);
        }
    }

    /// Test idempotency with empty files
    #[test]
    fn test_copy_empty_file_idempotent() {
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
        assert_idempotent(&module, &params, &context, true);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "");
    }

    /// Test idempotency with special characters in content
    #[test]
    fn test_copy_special_chars_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("special.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "content".to_string(),
            serde_json::json!("Line1\nLine2\tTab\r\n"),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
    }

    /// Test idempotency with Unicode content
    #[test]
    fn test_copy_unicode_idempotent() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("unicode.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello, World!"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
    }

    /// Test lineinfile with empty file
    #[test]
    fn test_lineinfile_empty_file_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("empty.txt");

        fs::write(&path, "").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("first line"));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
    }

    /// Test that check mode doesn't affect idempotency
    #[test]
    fn test_check_mode_doesnt_affect_idempotency() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let check_context = ModuleContext::default().with_check_mode(true);
        let normal_context = ModuleContext::default();

        // Check mode doesn't create anything
        let check_result = module.execute(&params, &check_context).unwrap();
        assert!(
            check_result.changed,
            "Check mode should report would change"
        );
        assert!(!path.exists(), "Check mode should not create directory");

        // Normal execution creates
        let result1 = module.execute(&params, &normal_context).unwrap();
        assert!(result1.changed, "First run should create directory");
        assert!(path.exists(), "Directory should exist");

        // Second execution is idempotent
        let result2 = module.execute(&params, &normal_context).unwrap();
        assert!(!result2.changed, "Second run should be idempotent");
    }

    /// Test file module with nested directories
    #[test]
    fn test_file_nested_directories_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a").join("b").join("c").join("d");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));
        params.insert("recurse".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        assert_idempotent(&module, &params, &context, true);
        assert!(path.is_dir());
    }

    /// Test that mode changes are idempotent
    #[test]
    fn test_mode_change_sequence_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        fs::write(&path, "content").unwrap();

        let module = FileModule;
        let context = ModuleContext::default();

        // Change to 755
        let mut params1: ModuleParams = HashMap::new();
        params1.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params1.insert("state".to_string(), serde_json::json!("file"));
        params1.insert("mode".to_string(), serde_json::json!(0o755));

        let _result1 = module.execute(&params1, &context).unwrap();
        // May or may not change depending on initial mode

        let result2 = module.execute(&params1, &context).unwrap();
        assert!(
            !result2.changed,
            "Second run with same mode should be idempotent"
        );

        // Change to 644
        let mut params2: ModuleParams = HashMap::new();
        params2.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params2.insert("state".to_string(), serde_json::json!("file"));
        params2.insert("mode".to_string(), serde_json::json!(0o644));

        let result3 = module.execute(&params2, &context).unwrap();
        assert!(result3.changed, "Mode change should be detected");

        let result4 = module.execute(&params2, &context).unwrap();
        assert!(
            !result4.changed,
            "Second run with new mode should be idempotent"
        );
    }
}

// ============================================================================
// DOCUMENTATION: KNOWN IDEMPOTENCY ISSUES
// ============================================================================

/// This module documents known idempotency issues and their workarounds.
/// These tests ensure we don't regress on fixed issues.
mod known_issues {
    use super::*;

    /// DOCUMENTED BEHAVIOR: file state=touch is intentionally non-idempotent
    /// Touch always updates timestamps, so it always reports changed.
    /// This is consistent with Ansible behavior.
    #[test]
    fn test_documented_touch_not_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("touchfile");

        fs::write(&path, "").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("touch"));

        let context = ModuleContext::default();

        // Touch is documented as always changing
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed, "Touch should always report changed");

        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            result2.changed,
            "Touch should always report changed (by design)"
        );
    }

    /// DOCUMENTED BEHAVIOR: command/shell without creates/removes is non-idempotent
    /// Use creates or removes parameters to make them idempotent.
    #[test]
    fn test_documented_command_without_creates_not_idempotent() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("true"));
        params.insert("shell_type".to_string(), serde_json::json!("posix"));

        let context = ModuleContext::default();

        // Without creates/removes, command is always changed
        let result1 = module.execute(&params, &context).unwrap();
        assert!(
            result1.changed,
            "Command should always report changed without creates/removes"
        );

        let result2 = module.execute(&params, &context).unwrap();
        assert!(
            result2.changed,
            "Command should always report changed (by design)"
        );
    }
}
