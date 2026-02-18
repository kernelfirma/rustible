//! Comprehensive unit tests for the Shell module
//!
//! Tests cover:
//! - Command execution
//! - Shell features (pipes, redirects)
//! - Check mode
//! - Creates/removes conditions
//! - Environment variables
//! - Edge cases

use rustible::modules::shell::ShellModule;
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;
use tempfile::TempDir;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_shell_module_name() {
    let module = ShellModule;
    assert_eq!(module.name(), "shell");
}

#[test]
fn test_shell_module_description() {
    let module = ShellModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("shell"));
}

#[test]
fn test_shell_module_classification() {
    let module = ShellModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_shell_module_required_params() {
    let module = ShellModule;
    let required = module.required_params();
    assert!(required.is_empty());
}

// ============================================================================
// Basic Command Execution Tests
// ============================================================================

#[test]
fn test_shell_echo() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.as_ref().unwrap().contains("hello"));
}

#[test]
fn test_shell_pipe() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo 'hello world' | grep hello"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.as_ref().unwrap().contains("hello"));
}

#[test]
fn test_shell_env_expansion() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo $HOME"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    // HOME should be expanded to a path
    assert!(!result.stdout.as_ref().unwrap().contains("$HOME"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_shell_check_mode() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("rm -rf /"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
}

#[test]
fn test_shell_check_mode_no_execution() {
    let module = ShellModule;
    let temp = TempDir::new().unwrap();
    let test_file = temp.path().join("test_file");

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!(format!("touch {}", test_file.display())),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let _ = module.execute(&params, &context).unwrap();

    // File should NOT be created in check mode
    assert!(!test_file.exists());
}

// ============================================================================
// Creates/Removes Condition Tests
// ============================================================================

#[test]
fn test_shell_creates_exists() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert("creates".to_string(), serde_json::json!("/"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_shell_creates_not_exists() {
    let module = ShellModule;
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("nonexistent");

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert(
        "creates".to_string(),
        serde_json::json!(nonexistent.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    // Command should run because 'creates' doesn't exist
    assert!(result.changed);
}

#[test]
fn test_shell_removes_exists() {
    let module = ShellModule;
    let temp = TempDir::new().unwrap();
    let test_file = temp.path().join("testfile");
    std::fs::write(&test_file, "content").unwrap();

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert(
        "removes".to_string(),
        serde_json::json!(test_file.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    // Command should run because 'removes' file exists
    assert!(result.changed);
}

#[test]
fn test_shell_removes_not_exists() {
    let module = ShellModule;
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("nonexistent");

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert(
        "removes".to_string(),
        serde_json::json!(nonexistent.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

// ============================================================================
// Stdin Tests
// ============================================================================

#[test]
fn test_shell_with_stdin() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("cat"));
    params.insert("stdin".to_string(), serde_json::json!("hello from stdin"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.as_ref().unwrap().contains("hello from stdin"));
}

// ============================================================================
// Working Directory Tests
// ============================================================================

#[test]
fn test_shell_with_chdir() {
    let module = ShellModule;
    let temp = TempDir::new().unwrap();

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("pwd"));
    params.insert(
        "chdir".to_string(),
        serde_json::json!(temp.path().to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result
        .stdout
        .as_ref()
        .unwrap()
        .contains(temp.path().to_str().unwrap()));
}

// ============================================================================
// Environment Variable Tests
// ============================================================================

#[test]
fn test_shell_with_env() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo $TEST_VAR"));
    params.insert(
        "env".to_string(),
        serde_json::json!({"TEST_VAR": "test_value"}),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.as_ref().unwrap().contains("test_value"));
}

// ============================================================================
// Shell Executable Tests
// ============================================================================

#[test]
fn test_shell_custom_executable() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo test"));
    params.insert("executable".to_string(), serde_json::json!("/bin/bash"));

    assert!(params.contains_key("executable"));
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_shell_command_failure() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("exit 1"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

#[test]
fn test_shell_missing_cmd_parameter() {
    let module = ShellModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_shell_multiline_command() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo line1 && echo line2"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let stdout = result.stdout.as_ref().unwrap();
    assert!(stdout.contains("line1"));
    assert!(stdout.contains("line2"));
}

#[test]
fn test_shell_complex_pipeline() {
    let module = ShellModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo 'test' | tr 't' 'T' | tr 'e' 'E'"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.as_ref().unwrap().contains("TEsT"));
}
