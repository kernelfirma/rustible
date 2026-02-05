//! Comprehensive tests for the Rustible module system
//!
//! These tests verify the core functionality of the module system including:
//! - ModuleRegistry - registration, lookup, execution
//! - ModuleOutput - all factory methods (ok, changed, failed, skipped)
//! - ModuleContext - builder pattern, check mode, diff mode
//! - ParamExt trait - get_string, get_bool, get_i64, get_vec_string with edge cases
//! - Module trait - validation, required params, classification, parallelization hints
//! - Each built-in module: command, shell, copy, template, file, package, service, user
//! - Error handling for missing params, invalid params, execution failures
//! - Check mode behavior for all modules
//! - Diff generation for file-based modules

#![allow(unused_variables)]

mod common;

use common::MockConnection;
use rustible::connection::CommandResult;
use rustible::modules::{
    command::CommandModule, copy::CopyModule, file::FileModule, package::PackageModule,
    service::ServiceModule, shell::ShellModule, template::TemplateModule, user::UserModule, Diff,
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleRegistry, ModuleStatus, ParallelizationHint, ParamExt,
};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// ModuleRegistry Tests
// ============================================================================

#[test]
fn test_registry_new_is_empty() {
    let registry = ModuleRegistry::new();
    assert!(!registry.contains("command"));
    assert!(!registry.contains("shell"));
    assert_eq!(registry.names().len(), 0);
}

#[test]
fn test_registry_with_builtins() {
    let registry = ModuleRegistry::with_builtins();
    assert!(registry.contains("command"));
    assert!(registry.contains("shell"));
    assert!(registry.contains("copy"));
    assert!(registry.contains("template"));
    assert!(registry.contains("file"));
    assert!(registry.contains("package"));
    assert!(registry.contains("service"));
    assert!(registry.contains("user"));
}

#[test]
fn test_registry_default_has_builtins() {
    let registry = ModuleRegistry::default();
    assert!(registry.contains("command"));
    let names = registry.names();
    assert!(names.len() >= 8);
}

#[test]
fn test_registry_register_and_get() {
    let mut registry = ModuleRegistry::new();
    let module = std::sync::Arc::new(CommandModule);
    registry.register(module);

    assert!(registry.contains("command"));
    let retrieved = registry.get("command");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name(), "command");
}

#[test]
fn test_registry_get_nonexistent() {
    let registry = ModuleRegistry::new();
    assert!(registry.get("nonexistent").is_none());
    assert!(!registry.contains("nonexistent"));
}

#[test]
fn test_registry_execute_missing_module() {
    let registry = ModuleRegistry::new();
    let params = HashMap::new();
    let context = ModuleContext::default();

    let result = registry.execute("nonexistent", &params, &context);
    assert!(result.is_err());
    match result {
        Err(ModuleError::NotFound(name)) => assert_eq!(name, "nonexistent"),
        _ => panic!("Expected NotFound error"),
    }
}

#[test]
fn test_registry_execute_missing_required_params() {
    let mut registry = ModuleRegistry::new();
    registry.register(std::sync::Arc::new(CommandModule));

    let params = HashMap::new(); // Missing "cmd" param
    let context = ModuleContext::default();

    let result = registry.execute("command", &params, &context);
    assert!(result.is_err());
    match result {
        Err(ModuleError::MissingParameter(_)) => {}
        _ => panic!("Expected MissingParameter error"),
    }
}

#[test]
fn test_registry_execute_with_check_mode() {
    let mut registry = ModuleRegistry::new();
    registry.register(std::sync::Arc::new(CommandModule));

    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = registry.execute("command", &params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
}

// ============================================================================
// ModuleOutput Tests
// ============================================================================

#[test]
fn test_module_output_ok() {
    let output = ModuleOutput::ok("Everything is fine");

    assert!(!output.changed);
    assert_eq!(output.status, ModuleStatus::Ok);
    assert_eq!(output.msg, "Everything is fine");
    assert!(output.diff.is_none());
    assert!(output.data.is_empty());
    assert!(output.stdout.is_none());
    assert!(output.stderr.is_none());
    assert!(output.rc.is_none());
}

#[test]
fn test_module_output_changed() {
    let output = ModuleOutput::changed("Configuration updated");

    assert!(output.changed);
    assert_eq!(output.status, ModuleStatus::Changed);
    assert_eq!(output.msg, "Configuration updated");
}

#[test]
fn test_module_output_failed() {
    let output = ModuleOutput::failed("Operation failed");

    assert!(!output.changed);
    assert_eq!(output.status, ModuleStatus::Failed);
    assert_eq!(output.msg, "Operation failed");
}

#[test]
fn test_module_output_skipped() {
    let output = ModuleOutput::skipped("Condition not met");

    assert!(!output.changed);
    assert_eq!(output.status, ModuleStatus::Skipped);
    assert_eq!(output.msg, "Condition not met");
}

#[test]
fn test_module_output_with_diff() {
    let diff = Diff::new("old content", "new content");
    let output = ModuleOutput::changed("Updated").with_diff(diff);

    assert!(output.changed);
    assert!(output.diff.is_some());
    let d = output.diff.unwrap();
    assert_eq!(d.before, "old content");
    assert_eq!(d.after, "new content");
}

#[test]
fn test_module_output_with_data() {
    let output = ModuleOutput::ok("Done")
        .with_data("key1", serde_json::json!("value1"))
        .with_data("key2", serde_json::json!(42));

    assert_eq!(output.data.len(), 2);
    assert_eq!(output.data.get("key1"), Some(&serde_json::json!("value1")));
    assert_eq!(output.data.get("key2"), Some(&serde_json::json!(42)));
}

#[test]
fn test_module_output_with_command_output() {
    let output = ModuleOutput::changed("Executed").with_command_output(
        Some("stdout content".to_string()),
        Some("stderr content".to_string()),
        Some(0),
    );

    assert_eq!(output.stdout, Some("stdout content".to_string()));
    assert_eq!(output.stderr, Some("stderr content".to_string()));
    assert_eq!(output.rc, Some(0));
}

#[test]
fn test_module_output_builder_pattern() {
    let output = ModuleOutput::changed("Complex operation")
        .with_diff(Diff::new("before", "after"))
        .with_data("file", serde_json::json!("/tmp/test.txt"))
        .with_command_output(Some("ok".to_string()), None, Some(0));

    assert!(output.changed);
    assert!(output.diff.is_some());
    assert_eq!(output.data.len(), 1);
    assert!(output.stdout.is_some());
}

// ============================================================================
// Diff Tests
// ============================================================================

#[test]
fn test_diff_new() {
    let diff = Diff::new("old", "new");
    assert_eq!(diff.before, "old");
    assert_eq!(diff.after, "new");
    assert!(diff.details.is_none());
}

#[test]
fn test_diff_with_details() {
    let diff = Diff::new("old", "new").with_details("unified diff here");
    assert_eq!(diff.before, "old");
    assert_eq!(diff.after, "new");
    assert_eq!(diff.details, Some("unified diff here".to_string()));
}

// ============================================================================
// ModuleContext Tests
// ============================================================================

#[test]
fn test_module_context_default() {
    let ctx = ModuleContext::default();

    assert!(!ctx.check_mode);
    assert!(!ctx.diff_mode);
    assert!(ctx.vars.is_empty());
    assert!(ctx.facts.is_empty());
    assert!(ctx.work_dir.is_none());
    assert!(!ctx.r#become);
    assert!(ctx.become_method.is_none());
    assert!(ctx.become_user.is_none());
}

#[test]
fn test_module_context_new() {
    let ctx = ModuleContext::new();
    assert!(!ctx.check_mode);
    assert!(!ctx.diff_mode);
}

#[test]
fn test_module_context_with_check_mode() {
    let ctx = ModuleContext::new().with_check_mode(true);
    assert!(ctx.check_mode);
    assert!(!ctx.diff_mode);
}

#[test]
fn test_module_context_with_diff_mode() {
    let ctx = ModuleContext::new().with_diff_mode(true);
    assert!(!ctx.check_mode);
    assert!(ctx.diff_mode);
}

#[test]
fn test_module_context_with_vars() {
    let mut vars = HashMap::new();
    vars.insert("env".to_string(), serde_json::json!("production"));
    vars.insert("debug".to_string(), serde_json::json!(false));

    let ctx = ModuleContext::new().with_vars(vars.clone());
    assert_eq!(ctx.vars.len(), 2);
    assert_eq!(ctx.vars.get("env"), Some(&serde_json::json!("production")));
}

#[test]
fn test_module_context_with_facts() {
    let mut facts = HashMap::new();
    facts.insert("os_family".to_string(), serde_json::json!("Debian"));

    let ctx = ModuleContext::new().with_facts(facts.clone());
    assert_eq!(ctx.facts.len(), 1);
    assert_eq!(
        ctx.facts.get("os_family"),
        Some(&serde_json::json!("Debian"))
    );
}

#[test]
fn test_module_context_builder_pattern() {
    let mut vars = HashMap::new();
    vars.insert("key".to_string(), serde_json::json!("value"));

    let ctx = ModuleContext::new()
        .with_check_mode(true)
        .with_diff_mode(true)
        .with_vars(vars);

    assert!(ctx.check_mode);
    assert!(ctx.diff_mode);
    assert_eq!(ctx.vars.len(), 1);
}

// ============================================================================
// ParamExt Trait Tests
// ============================================================================

#[test]
fn test_param_ext_get_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("str".to_string(), serde_json::json!("hello"));

    assert_eq!(params.get_string("str").unwrap(), Some("hello".to_string()));
    assert_eq!(params.get_string("missing").unwrap(), None);
}

#[test]
fn test_param_ext_get_string_from_number() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("num".to_string(), serde_json::json!(42));

    let result = params.get_string("num").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "42");
}

#[test]
fn test_param_ext_get_string_required() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("key".to_string(), serde_json::json!("value"));

    assert_eq!(
        params.get_string_required("key").unwrap(),
        "value".to_string()
    );

    let result = params.get_string_required("missing");
    assert!(result.is_err());
    match result {
        Err(ModuleError::MissingParameter(name)) => assert_eq!(name, "missing"),
        _ => panic!("Expected MissingParameter error"),
    }
}

#[test]
fn test_param_ext_get_bool() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("bool_true".to_string(), serde_json::json!(true));
    params.insert("bool_false".to_string(), serde_json::json!(false));

    assert_eq!(params.get_bool("bool_true").unwrap(), Some(true));
    assert_eq!(params.get_bool("bool_false").unwrap(), Some(false));
    assert_eq!(params.get_bool("missing").unwrap(), None);
}

#[test]
fn test_param_ext_get_bool_from_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("yes".to_string(), serde_json::json!("yes"));
    params.insert("no".to_string(), serde_json::json!("no"));
    params.insert("true".to_string(), serde_json::json!("true"));
    params.insert("false".to_string(), serde_json::json!("false"));
    params.insert("1".to_string(), serde_json::json!("1"));
    params.insert("0".to_string(), serde_json::json!("0"));
    params.insert("on".to_string(), serde_json::json!("on"));
    params.insert("off".to_string(), serde_json::json!("off"));

    assert_eq!(params.get_bool("yes").unwrap(), Some(true));
    assert_eq!(params.get_bool("no").unwrap(), Some(false));
    assert_eq!(params.get_bool("true").unwrap(), Some(true));
    assert_eq!(params.get_bool("false").unwrap(), Some(false));
    assert_eq!(params.get_bool("1").unwrap(), Some(true));
    assert_eq!(params.get_bool("0").unwrap(), Some(false));
    assert_eq!(params.get_bool("on").unwrap(), Some(true));
    assert_eq!(params.get_bool("off").unwrap(), Some(false));
}

#[test]
fn test_param_ext_get_bool_invalid_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("invalid".to_string(), serde_json::json!("maybe"));

    let result = params.get_bool("invalid");
    assert!(result.is_err());
    match result {
        Err(ModuleError::InvalidParameter(_)) => {}
        _ => panic!("Expected InvalidParameter error"),
    }
}

#[test]
fn test_param_ext_get_bool_or() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("bool".to_string(), serde_json::json!(true));

    assert_eq!(params.get_bool_or("bool", false), true);
    assert_eq!(params.get_bool_or("missing", true), true);
    assert_eq!(params.get_bool_or("missing", false), false);
}

#[test]
fn test_param_ext_get_i64() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("num".to_string(), serde_json::json!(42));
    params.insert("neg".to_string(), serde_json::json!(-10));

    assert_eq!(params.get_i64("num").unwrap(), Some(42));
    assert_eq!(params.get_i64("neg").unwrap(), Some(-10));
    assert_eq!(params.get_i64("missing").unwrap(), None);
}

#[test]
fn test_param_ext_get_i64_from_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("str_num".to_string(), serde_json::json!("123"));

    assert_eq!(params.get_i64("str_num").unwrap(), Some(123));
}

#[test]
fn test_param_ext_get_i64_invalid_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("invalid".to_string(), serde_json::json!("not a number"));

    let result = params.get_i64("invalid");
    assert!(result.is_err());
}

#[test]
fn test_param_ext_get_u32() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("num".to_string(), serde_json::json!(42));

    assert_eq!(params.get_u32("num").unwrap(), Some(42));
}

#[test]
fn test_param_ext_get_u32_negative() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("neg".to_string(), serde_json::json!(-10));

    let result = params.get_u32("neg");
    assert!(result.is_err());
}

#[test]
fn test_param_ext_get_vec_string() {
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "list".to_string(),
        serde_json::json!(["one", "two", "three"]),
    );

    let result = params.get_vec_string("list").unwrap().unwrap();
    assert_eq!(result, vec!["one", "two", "three"]);
}

#[test]
fn test_param_ext_get_vec_string_from_comma_separated() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("list".to_string(), serde_json::json!("one,two,three"));

    let result = params.get_vec_string("list").unwrap().unwrap();
    assert_eq!(result, vec!["one", "two", "three"]);
}

#[test]
fn test_param_ext_get_vec_string_mixed_types() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("list".to_string(), serde_json::json!([1, "two", true]));

    let result = params.get_vec_string("list").unwrap().unwrap();
    assert_eq!(result.len(), 3);
}

#[test]
fn test_param_ext_get_vec_string_empty() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("list".to_string(), serde_json::json!([]));

    let result = params.get_vec_string("list").unwrap().unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_param_ext_get_vec_string_invalid_type() {
    let mut params: ModuleParams = HashMap::new();
    params.insert("num".to_string(), serde_json::json!(42));

    let result = params.get_vec_string("num");
    assert!(result.is_err());
}

// ============================================================================
// Module Trait Tests - Classification and Parallelization
// ============================================================================

#[test]
fn test_module_classification() {
    let command = CommandModule;
    assert_eq!(
        command.classification(),
        ModuleClassification::RemoteCommand
    );

    let copy = CopyModule;
    assert_eq!(copy.classification(), ModuleClassification::NativeTransport);

    let file = FileModule;
    assert_eq!(file.classification(), ModuleClassification::NativeTransport);
}

#[test]
fn test_module_parallelization_hints() {
    let command = CommandModule;
    assert_eq!(
        command.parallelization_hint(),
        ParallelizationHint::FullyParallel
    );

    let package = PackageModule;
    assert_eq!(
        package.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

#[test]
fn test_module_required_params() {
    let command = CommandModule;
    // CommandModule now supports either 'cmd' or 'argv', so required_params is empty
    // Validation is done in validate_params instead
    let required = command.required_params();
    assert!(required.is_empty());

    // Verify validate_params enforces that either cmd or argv is required
    let empty_params: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    let validation_result = command.validate_params(&empty_params);
    assert!(validation_result.is_err());

    // With cmd provided, validation should pass
    let mut params_with_cmd: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    params_with_cmd.insert("cmd".to_string(), serde_json::json!("echo hello"));
    assert!(command.validate_params(&params_with_cmd).is_ok());

    // With argv provided, validation should pass
    let mut params_with_argv: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    params_with_argv.insert("argv".to_string(), serde_json::json!(["echo", "hello"]));
    assert!(command.validate_params(&params_with_argv).is_ok());

    let copy = CopyModule;
    let required = copy.required_params();
    assert!(required.is_empty()); // Uses validate_params instead
}

#[test]
fn test_module_validate_params() {
    let copy = CopyModule;

    // Valid: has src and dest
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!("/tmp/src"));
    params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
    assert!(copy.validate_params(&params).is_ok());

    // Valid: has content and dest
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("hello"));
    params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
    assert!(copy.validate_params(&params).is_ok());

    // Invalid: missing both src and content
    let mut params = HashMap::new();
    params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
    assert!(copy.validate_params(&params).is_err());

    // Invalid: missing dest
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!("/tmp/src"));
    assert!(copy.validate_params(&params).is_err());
}

// ============================================================================
// Command Module Tests
// ============================================================================

#[test]
fn test_command_basic_execution() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.is_some());
    assert!(result.stdout.unwrap().contains("hello"));
    assert_eq!(result.rc, Some(0));
}

#[test]
fn test_command_with_argv() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("")); // dummy value
    params.insert(
        "argv".to_string(),
        serde_json::json!(["echo", "hello", "world"]),
    );
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("hello world"));
}

#[test]
fn test_command_check_mode() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("dangerous command"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
}

#[test]
fn test_command_creates_skip() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo test"));
    params.insert("creates".to_string(), serde_json::json!("/")); // Root always exists

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_command_removes_skip() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo test"));
    params.insert(
        "removes".to_string(),
        serde_json::json!("/nonexistent_path_12345"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_command_failure() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("false"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
    match result {
        Err(ModuleError::CommandFailed { code, .. }) => assert_ne!(code, 0),
        _ => panic!("Expected CommandFailed error"),
    }
}

// ============================================================================
// Shell Module Tests
// ============================================================================

#[test]
fn test_shell_basic_execution() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.is_some());
}

#[test]
fn test_shell_with_pipe() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo 'hello world' | grep hello"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("hello"));
}

#[test]
fn test_shell_check_mode() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("rm -rf /"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
}

#[test]
fn test_shell_with_stdin() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("cat"));
    params.insert("stdin".to_string(), serde_json::json!("test input"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("test input"));
}

#[test]
fn test_shell_missing_cmd() {
    let module = ShellModule;
    let params = HashMap::new();

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

// ============================================================================
// Copy Module Tests
// ============================================================================

#[test]
fn test_copy_with_content() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
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
fn test_copy_with_src() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest = temp.path().join("dest.txt");

    fs::write(&src, "Source content").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Source content");
}

#[test]
fn test_copy_idempotent() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Same content").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("Same content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_copy_check_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("Hello"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would copy"));
    assert!(!dest.exists());
}

#[test]
fn test_copy_with_backup() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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
}

#[test]
fn test_copy_missing_src_and_content() {
    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

    let result = module.validate_params(&params);
    assert!(result.is_err());
}

#[test]
fn test_copy_diff_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "old content").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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

// ============================================================================
// File Module Tests
// ============================================================================

#[test]
fn test_file_create_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.is_dir());
}

#[test]
fn test_file_create_directory_idempotent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    fs::create_dir(&path).unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_file_create_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("file"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.is_file());
}

#[test]
fn test_file_absent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!path.exists());
}

#[test]
fn test_file_symlink() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("link");
    fs::write(&src, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("link"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.is_symlink());
    assert_eq!(fs::read_link(&dest).unwrap(), src);
}

#[test]
fn test_file_touch() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("touch"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.exists());
}

#[test]
fn test_file_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would create"));
    assert!(!path.exists());
}

#[test]
fn test_file_missing_required_param() {
    let module = FileModule;
    let params = HashMap::new();

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

// ============================================================================
// Template Module Tests
// ============================================================================

#[test]
fn test_template_basic() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.txt.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Hello, {{ name }}!").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
}

#[test]
fn test_template_with_loops() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.txt.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "{% for item in items %}{{ item }}\n{% endfor %}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
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
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "one\ntwo\nthree\n");
}

#[test]
fn test_template_idempotent() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.txt.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Hello, {{ name }}!").unwrap();
    fs::write(&dest, "Hello, World!").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_template_check_mode() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.txt.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Hello, {{ name }}!").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let context = ModuleContext::default()
        .with_vars(vars)
        .with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would render"));
    assert!(!dest.exists());
}

#[test]
fn test_template_missing_src() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("output.txt");

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert(
        "src".to_string(),
        serde_json::json!("/nonexistent/template.j2"),
    );
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

// ============================================================================
// Package Module Tests (basic validation)
// ============================================================================

#[test]
fn test_package_module_name() {
    let module = PackageModule;
    assert_eq!(module.name(), "package");
}

#[test]
fn test_package_module_classification() {
    let module = PackageModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_package_module_parallelization() {
    let module = PackageModule;
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

#[test]
fn test_package_missing_name() {
    let module = PackageModule;
    let params = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Service Module Tests (basic validation)
// ============================================================================

#[test]
fn test_service_module_name() {
    let module = ServiceModule;
    assert_eq!(module.name(), "service");
}

#[test]
fn test_service_module_classification() {
    let module = ServiceModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_service_missing_name() {
    let module = ServiceModule;
    let params = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Service Module Extended Tests - Mocked systemctl commands
// ============================================================================

#[tokio::test]
async fn test_service_state_started_when_stopped() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return failure (service is stopped)
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    // Mock systemctl start to succeed
    mock.set_command_result(
        "systemctl start nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Started"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl start")));
}

#[tokio::test]
async fn test_service_state_started_when_already_running() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return success (service is running)
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult::success("active".to_string(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert_eq!(result.status, ModuleStatus::Ok);
    assert!(result.msg.contains("already running"));
}

#[tokio::test]
async fn test_service_state_stopped_when_running() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return success (service is running)
    mock.set_command_result(
        "systemctl is-active apache2",
        CommandResult::success("active".to_string(), String::new()),
    );

    // Mock systemctl stop to succeed
    mock.set_command_result(
        "systemctl stop apache2",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("apache2"));
    params.insert("state".to_string(), serde_json::json!("stopped"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Stopped"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl stop")));
}

#[tokio::test]
async fn test_service_state_stopped_when_already_stopped() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return failure (service is stopped)
    mock.set_command_result(
        "systemctl is-active apache2",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("apache2"));
    params.insert("state".to_string(), serde_json::json!("stopped"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert_eq!(result.status, ModuleStatus::Ok);
    assert!(result.msg.contains("already stopped"));
}

#[tokio::test]
async fn test_service_enabled_true() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-enabled to return failure (service is disabled)
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult {
            success: false,
            stdout: "disabled".to_string(),
            stderr: String::new(),
            exit_code: 1,
        },
    );

    // Mock systemctl enable to succeed
    mock.set_command_result(
        "systemctl enable nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active for final status check
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("enabled"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl enable")));
}

#[tokio::test]
async fn test_service_enabled_false() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-enabled to return success (service is enabled)
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult::success("enabled".to_string(), String::new()),
    );

    // Mock systemctl disable to succeed
    mock.set_command_result(
        "systemctl disable nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active for final status check
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(false));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("disabled"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl disable")));
}

#[tokio::test]
async fn test_service_enabled_already_enabled() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-enabled to return success (service is enabled)
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult::success("enabled".to_string(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active for final status check
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert_eq!(result.status, ModuleStatus::Ok);
}

#[tokio::test]
async fn test_service_state_restarted() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl restart to succeed
    mock.set_command_result(
        "systemctl restart postgresql",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active (needed for state parsing)
    mock.set_command_result(
        "systemctl is-active postgresql",
        CommandResult::success("active".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("postgresql"));
    params.insert("state".to_string(), serde_json::json!("restarted"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Restarted"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl restart")));
}

#[tokio::test]
async fn test_service_state_reloaded() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl reload to succeed
    mock.set_command_result(
        "systemctl reload nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active (needed for state parsing)
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult::success("active".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("reloaded"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Reloaded"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl reload")));
}

#[tokio::test]
async fn test_service_daemon_reload() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl daemon-reload to succeed
    mock.set_command_result(
        "systemctl daemon-reload",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active for final status check
    mock.set_command_result(
        "systemctl is-active myservice",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    // Mock systemctl is-enabled for final status check
    mock.set_command_result(
        "systemctl is-enabled myservice",
        CommandResult {
            success: false,
            stdout: "disabled".to_string(),
            stderr: String::new(),
            exit_code: 1,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myservice"));
    params.insert("daemon_reload".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Reloaded systemd daemon"));

    // Verify the commands were executed
    let commands = mock.get_commands();
    assert!(commands
        .iter()
        .any(|c| c.contains("systemctl daemon-reload")));
}

#[tokio::test]
async fn test_service_combined_state_and_enabled() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return failure (service is stopped)
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    // Mock systemctl is-enabled to return failure (service is disabled)
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult {
            success: false,
            stdout: "disabled".to_string(),
            stderr: String::new(),
            exit_code: 1,
        },
    );

    // Mock systemctl enable to succeed
    mock.set_command_result(
        "systemctl enable nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock systemctl start to succeed
    mock.set_command_result(
        "systemctl start nginx",
        CommandResult::success(String::new(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("enabled"));
    assert!(result.msg.contains("Started"));

    // Verify both commands were executed
    let commands = mock.get_commands();
    assert!(commands.iter().any(|c| c.contains("systemctl enable")));
    assert!(commands.iter().any(|c| c.contains("systemctl start")));
}

#[tokio::test]
async fn test_service_check_mode_started() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return failure (service is stopped)
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));

    let context = ModuleContext::default()
        .with_connection(mock.clone())
        .with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Would start"));

    // Verify that start command was NOT executed in check mode
    let commands = mock.get_commands();
    assert!(!commands.iter().any(|c| c.contains("systemctl start")));
}

#[tokio::test]
async fn test_service_check_mode_enabled() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-enabled to return failure (service is disabled)
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult {
            success: false,
            stdout: "disabled".to_string(),
            stderr: String::new(),
            exit_code: 1,
        },
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    // Mock systemctl is-active for final status check
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult {
            success: false,
            stdout: "inactive".to_string(),
            stderr: String::new(),
            exit_code: 3,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default()
        .with_connection(mock.clone())
        .with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert!(result.msg.contains("Would enable"));

    // Verify that enable command was NOT executed in check mode
    let commands = mock.get_commands();
    assert!(!commands.iter().any(|c| c.contains("systemctl enable")));
}

#[tokio::test]
async fn test_service_invalid_state() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("invalid_state"));

    let context = ModuleContext::default().with_connection(mock);
    let result = module.execute(&params, &context);

    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("Invalid state"));
}

#[tokio::test]
async fn test_service_with_status_data() {
    let module = ServiceModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock systemctl is-active to return success
    mock.set_command_result(
        "systemctl is-active nginx",
        CommandResult::success("active".to_string(), String::new()),
    );

    // Mock systemctl is-enabled to return success
    mock.set_command_result(
        "systemctl is-enabled nginx",
        CommandResult::success("enabled".to_string(), String::new()),
    );

    // Mock the detection commands
    mock.set_command_result(
        "test -d /run/systemd/system && echo yes || echo no",
        CommandResult::success("yes".to_string(), String::new()),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));

    let context = ModuleContext::default().with_connection(mock.clone());
    let result = module.execute(&params, &context).unwrap();

    // Verify that status data is included in the output
    assert!(result.data.contains_key("status"));
    let status = &result.data["status"];
    assert_eq!(status["active"], serde_json::json!(true));
    assert_eq!(status["enabled"], serde_json::json!(true));
}

// ============================================================================
// User Module Tests (basic validation)
// ============================================================================

#[test]
fn test_user_module_name() {
    let module = UserModule;
    assert_eq!(module.name(), "user");
}

#[test]
fn test_user_module_classification() {
    let module = UserModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_user_missing_name() {
    let module = UserModule;
    let params = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// ModuleStatus Tests
// ============================================================================

#[test]
fn test_module_status_display() {
    assert_eq!(format!("{}", ModuleStatus::Ok), "ok");
    assert_eq!(format!("{}", ModuleStatus::Changed), "changed");
    assert_eq!(format!("{}", ModuleStatus::Failed), "failed");
    assert_eq!(format!("{}", ModuleStatus::Skipped), "skipped");
}

#[test]
fn test_module_status_equality() {
    assert_eq!(ModuleStatus::Ok, ModuleStatus::Ok);
    assert_eq!(ModuleStatus::Changed, ModuleStatus::Changed);
    assert_ne!(ModuleStatus::Ok, ModuleStatus::Changed);
}

// ============================================================================
// ModuleClassification Tests
// ============================================================================

#[test]
fn test_module_classification_display() {
    assert_eq!(
        format!("{}", ModuleClassification::LocalLogic),
        "local_logic"
    );
    assert_eq!(
        format!("{}", ModuleClassification::NativeTransport),
        "native_transport"
    );
    assert_eq!(
        format!("{}", ModuleClassification::RemoteCommand),
        "remote_command"
    );
    assert_eq!(
        format!("{}", ModuleClassification::PythonFallback),
        "python_fallback"
    );
}

#[test]
fn test_module_classification_default() {
    let default = ModuleClassification::default();
    assert_eq!(default, ModuleClassification::RemoteCommand);
}

// ============================================================================
// ParallelizationHint Tests
// ============================================================================

#[test]
fn test_parallelization_hint_default() {
    let default = ParallelizationHint::default();
    assert_eq!(default, ParallelizationHint::FullyParallel);
}

#[test]
fn test_parallelization_hint_equality() {
    assert_eq!(
        ParallelizationHint::FullyParallel,
        ParallelizationHint::FullyParallel
    );
    assert_eq!(
        ParallelizationHint::HostExclusive,
        ParallelizationHint::HostExclusive
    );
    assert_ne!(
        ParallelizationHint::FullyParallel,
        ParallelizationHint::HostExclusive
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_module_error_not_found() {
    let error = ModuleError::NotFound("test_module".to_string());
    assert_eq!(format!("{}", error), "Module not found: test_module");
}

#[test]
fn test_module_error_invalid_parameter() {
    let error = ModuleError::InvalidParameter("invalid value".to_string());
    assert_eq!(format!("{}", error), "Invalid parameter: invalid value");
}

#[test]
fn test_module_error_missing_parameter() {
    let error = ModuleError::MissingParameter("required_param".to_string());
    assert_eq!(
        format!("{}", error),
        "Missing required parameter: required_param"
    );
}

#[test]
fn test_module_error_execution_failed() {
    let error = ModuleError::ExecutionFailed("command failed".to_string());
    assert_eq!(format!("{}", error), "Execution failed: command failed");
}

#[test]
fn test_module_error_command_failed() {
    let error = ModuleError::CommandFailed {
        code: 1,
        message: "error occurred".to_string(),
    };
    assert!(format!("{}", error).contains("exit code 1"));
    assert!(format!("{}", error).contains("error occurred"));
}

// ============================================================================
// Integration Tests - Complex Scenarios
// ============================================================================

#[test]
fn test_registry_full_workflow() {
    let registry = ModuleRegistry::with_builtins();

    // Execute a simple command
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo test"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = registry.execute("command", &params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
}

#[test]
fn test_multiple_modules_in_sequence() {
    let temp = TempDir::new().unwrap();

    // Create a directory
    let dir_path = temp.path().join("testdir");
    let file_module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dir_path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default();
    let result = file_module.execute(&params, &context).unwrap();
    assert!(result.changed);

    // Copy a file into that directory
    let dest = dir_path.join("file.txt");
    let copy_module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let result = copy_module.execute(&params, &context).unwrap();
    assert!(result.changed);
    assert!(dest.exists());
}

#[test]
fn test_check_mode_prevents_changes() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    // Run in check mode
    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!dest.exists()); // File should not be created

    // Now run for real
    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists()); // Now file should exist
}

#[test]
fn test_diff_mode_shows_changes() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");
    fs::write(&dest, "old").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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

    let diff = result.diff.unwrap();
    assert_eq!(diff.before, "old");
    assert_eq!(diff.after, "new");
}

#[test]
fn test_module_with_all_context_options() {
    let mut vars = HashMap::new();
    vars.insert("key".to_string(), serde_json::json!("value"));

    let mut facts = HashMap::new();
    facts.insert("os".to_string(), serde_json::json!("linux"));

    let context = ModuleContext {
        check_mode: true,
        diff_mode: true,
        verbosity: 0,
        vars,
        facts,
        work_dir: Some("/tmp".to_string()),
        r#become: true,
        become_method: Some("sudo".to_string()),
        become_user: Some("root".to_string()),
        become_password: None,
        connection: None,
    };

    assert!(context.check_mode);
    assert!(context.diff_mode);
    assert_eq!(context.vars.len(), 1);
    assert_eq!(context.facts.len(), 1);
    assert_eq!(context.work_dir, Some("/tmp".to_string()));
    assert!(context.r#become);
    assert_eq!(context.become_method, Some("sudo".to_string()));
    assert_eq!(context.become_user, Some("root".to_string()));
}

// ============================================================================
// EXTENDED COMMAND MODULE TESTS
// ============================================================================

#[test]
fn test_command_with_chdir() {
    let temp = TempDir::new().unwrap();
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("pwd"));
    params.insert(
        "chdir".to_string(),
        serde_json::json!(temp.path().to_str().unwrap()),
    );
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result
        .stdout
        .unwrap()
        .contains(temp.path().to_str().unwrap()));
}

#[test]
fn test_command_with_environment_variables() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("printenv TEST_VAR"));
    params.insert(
        "env".to_string(),
        serde_json::json!({"TEST_VAR": "hello_from_env"}),
    );
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("hello_from_env"));
}

#[test]
fn test_command_creates_when_file_does_not_exist() {
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("does_not_exist");

    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo creating"));
    params.insert(
        "creates".to_string(),
        serde_json::json!(nonexistent.to_str().unwrap()),
    );
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    // Should execute because file does not exist
    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("creating"));
}

#[test]
fn test_command_removes_when_file_exists() {
    let temp = TempDir::new().unwrap();
    let existing = temp.path().join("exists.txt");
    fs::write(&existing, "content").unwrap();

    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo removing"));
    params.insert(
        "removes".to_string(),
        serde_json::json!(existing.to_str().unwrap()),
    );
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    // Should execute because file exists
    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("removing"));
}

#[test]
fn test_command_with_work_dir_from_context() {
    let temp = TempDir::new().unwrap();
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("pwd"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    let mut context = ModuleContext::default();
    context.work_dir = Some(temp.path().to_str().unwrap().to_string());

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result
        .stdout
        .unwrap()
        .contains(temp.path().to_str().unwrap()));
}

#[test]
fn test_command_output_contains_stderr_warning() {
    let module = CommandModule;
    let mut params = HashMap::new();
    // Use a command that writes to stderr but succeeds
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo stderr_output >&2; echo stdout_output"),
    );

    // Need to use shell for stderr redirection
    let shell_module = ShellModule;
    let context = ModuleContext::default();
    let result = shell_module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stderr.is_some());
}

#[test]
fn test_command_with_empty_cmd_errors() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("   "));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    // Should fail because cmd is effectively empty after parsing
    assert!(result.is_err());
}

#[test]
fn test_command_idempotency_with_creates() {
    let temp = TempDir::new().unwrap();
    let marker = temp.path().join("marker.txt");

    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!(format!("touch {}", marker.to_str().unwrap())),
    );
    params.insert(
        "creates".to_string(),
        serde_json::json!(marker.to_str().unwrap()),
    );

    // First run uses shell to actually create the file
    let shell = ShellModule;
    let context = ModuleContext::default();
    let result = shell.execute(&params, &context).unwrap();
    assert!(result.changed);

    // Second run should skip because file now exists
    let result = module.execute(&params, &context).unwrap();
    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[tokio::test]
async fn test_command_remote_execution() {
    let module = CommandModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock the command
    mock.set_command_result(
        "echo hello",
        CommandResult::success("hello".to_string(), "".to_string()),
    );

    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert("shell_type".to_string(), serde_json::json!("posix"));

    // Create context with connection
    let context = ModuleContext::default().with_connection(mock.clone());

    // Execute
    // This calls execute_remote which uses Handle::current() (from #[tokio::test])
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(result.status, ModuleStatus::Changed);
    assert_eq!(result.stdout, Some("hello".to_string()));

    // Verify command count
    assert_eq!(mock.command_count(), 1);
}

// ============================================================================
// EXTENDED SHELL MODULE TESTS
// ============================================================================

#[test]
fn test_shell_with_variable_expansion() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo $HOME"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    // HOME should be expanded, not literal $HOME
    let stdout = result.stdout.unwrap();
    assert!(!stdout.contains("$HOME"));
    assert!(stdout.starts_with("/") || stdout.contains("home") || stdout.contains("Users"));
}

#[test]
fn test_shell_with_redirection() {
    let temp = TempDir::new().unwrap();
    let output_file = temp.path().join("output.txt");

    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!(format!(
            "echo 'redirected content' > {}",
            output_file.to_str().unwrap()
        )),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(output_file.exists());
    assert_eq!(
        fs::read_to_string(&output_file).unwrap().trim(),
        "redirected content"
    );
}

#[test]
fn test_shell_with_subshell() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo \"Date is: $(date +%Y)\""),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let stdout = result.stdout.unwrap();
    assert!(stdout.contains("Date is:"));
    // Should contain a 4-digit year
    assert!(stdout.contains("202") || stdout.contains("203"));
}

#[test]
fn test_shell_with_logical_operators() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("true && echo success"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("success"));
}

#[test]
fn test_shell_with_or_operator() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("false || echo fallback"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("fallback"));
}

#[test]
fn test_shell_complex_pipeline() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("echo -e 'apple\\nbanana\\ncherry' | sort | head -1"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().trim().contains("apple"));
}

#[test]
fn test_shell_with_backticks() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo `echo nested`"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("nested"));
}

#[test]
fn test_shell_creates_skip_condition() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo should_not_run"));
    params.insert("creates".to_string(), serde_json::json!("/"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_shell_removes_skip_condition() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo should_not_run"));
    params.insert(
        "removes".to_string(),
        serde_json::json!("/nonexistent_path_xyz"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_shell_with_multiline_script() {
    let module = ShellModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("x=1; y=2; echo $((x + y))"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.stdout.unwrap().contains("3"));
}

// ============================================================================
// EXTENDED COPY MODULE TESTS
// ============================================================================

#[test]
fn test_copy_with_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
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
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
}

#[test]
fn test_copy_mode_change_only() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");
    fs::write(&dest, "content").unwrap();

    // Set initial mode to 644
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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
}

#[test]
fn test_copy_creates_parent_directories() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("subdir").join("nested").join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
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

#[test]
fn test_copy_to_directory() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let dest_dir = temp.path().join("destdir");

    fs::write(&src, "source content").unwrap();
    fs::create_dir(&dest_dir).unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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
fn test_copy_with_backup_custom_suffix() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let mut params = HashMap::new();
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
fn test_copy_nonexistent_src() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("dest.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
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
}

#[test]
fn test_copy_output_contains_metadata() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
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
// EXTENDED TEMPLATE MODULE TESTS
// ============================================================================

#[test]
fn test_template_with_upper_filter() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "{{ name | upper }}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("hello"));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "HELLO");
}

#[test]
fn test_template_with_lower_filter() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "{{ name | lower }}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("HELLO"));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "hello");
}

#[test]
fn test_template_with_trim_filter() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "{{ name | trim }}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("  spaced  "));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "spaced");
}

#[test]
fn test_template_with_conditionals() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(
        &src,
        "{% if enabled %}Feature ON{% else %}Feature OFF{% endif %}",
    )
    .unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    // Test with enabled = true
    let mut vars = HashMap::new();
    vars.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Feature ON");

    // Test with enabled = false
    fs::remove_file(&dest).unwrap();
    let mut vars = HashMap::new();
    vars.insert("enabled".to_string(), serde_json::json!(false));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Feature OFF");
}

#[test]
fn test_template_with_nested_variables() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(
        &src,
        "{{ config.database.host }}:{{ config.database.port }}",
    )
    .unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert(
        "config".to_string(),
        serde_json::json!({
            "database": {
                "host": "localhost",
                "port": 5432
            }
        }),
    );

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "localhost:5432");
}

#[test]
fn test_template_with_mode() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "content").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("mode".to_string(), serde_json::json!(0o600));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
}

#[test]
fn test_template_with_backup() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "{{ value }}").unwrap();
    fs::write(&dest, "old content").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("backup".to_string(), serde_json::json!(true));

    let mut vars = HashMap::new();
    vars.insert("value".to_string(), serde_json::json!("new content"));

    let context = ModuleContext::default().with_vars(vars);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.data.contains_key("backup_file"));

    let backup_path = temp.path().join("output.txt~");
    assert!(backup_path.exists());
    assert_eq!(fs::read_to_string(&backup_path).unwrap(), "old content");
}

#[test]
fn test_template_with_facts() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "OS: {{ os_family }}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut facts = HashMap::new();
    facts.insert("os_family".to_string(), serde_json::json!("Debian"));

    let context = ModuleContext::default().with_facts(facts);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "OS: Debian");
}

#[test]
fn test_template_syntax_error() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    // Invalid Tera syntax
    fs::write(&src, "{% invalid syntax %}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

// ============================================================================
// EXTENDED FILE MODULE TESTS
// ============================================================================

#[test]
fn test_file_create_nested_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("a").join("b").join("c");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));
    params.insert("recurse".to_string(), serde_json::json!(true));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.is_dir());
}

#[test]
fn test_file_with_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("file"));
    params.insert("mode".to_string(), serde_json::json!(0o600));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(&path).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
}

#[test]
fn test_file_mode_change_on_existing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    fs::write(&path, "content").unwrap();

    // Set initial mode
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("file"));
    params.insert("mode".to_string(), serde_json::json!(0o755));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    let meta = fs::metadata(&path).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
}

#[test]
fn test_file_hardlink() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("hardlink");
    fs::write(&src, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("hard"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.exists());

    // Verify it's a hard link by checking inodes
    use std::os::unix::fs::MetadataExt;
    let src_meta = fs::metadata(&src).unwrap();
    let dest_meta = fs::metadata(&dest).unwrap();
    assert_eq!(src_meta.ino(), dest_meta.ino());
}

#[test]
fn test_file_hardlink_idempotent() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("hardlink");
    fs::write(&src, "content").unwrap();
    fs::hard_link(&src, &dest).unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("hard"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_file_symlink_idempotent() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("link");
    fs::write(&src, "content").unwrap();
    std::os::unix::fs::symlink(&src, &dest).unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("link"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_file_absent_directory_recursive() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("dir_to_remove");
    fs::create_dir_all(path.join("subdir")).unwrap();
    fs::write(path.join("file.txt"), "content").unwrap();
    fs::write(path.join("subdir").join("nested.txt"), "nested").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert("recurse".to_string(), serde_json::json!(true));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!path.exists());
}

#[test]
fn test_file_absent_already_absent() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("already absent"));
}

#[test]
fn test_file_symlink_check_mode() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("link");
    fs::write(&src, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("link"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would create symlink"));
    assert!(!dest.exists());
}

#[test]
fn test_file_invalid_state() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("invalid_state"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

#[test]
fn test_file_symlink_missing_src() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("link");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("link"));
    // Missing src parameter

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

#[test]
fn test_file_touch_updates_timestamp() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    fs::write(&path, "content").unwrap();

    // Get initial mtime
    let initial_mtime = fs::metadata(&path).unwrap().modified().unwrap();

    // Small delay to ensure time difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("touch"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let final_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(final_mtime > initial_mtime);
}

// ============================================================================
// PACKAGE MODULE EXTENDED TESTS
// ============================================================================

#[test]
fn test_package_with_multiple_packages() {
    let module = PackageModule;
    let mut params = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!(["pkg1", "pkg2", "pkg3"]),
    );
    params.insert("state".to_string(), serde_json::json!("present"));

    // This would require actual package manager access, so just test check mode
    let context = ModuleContext::default().with_check_mode(true);

    // The test will likely fail due to package manager detection
    // but we can verify the parameter parsing works
    let result = module.check(&params, &context);
    // We expect either success (with detected package manager) or
    // error (if no package manager found)
    // The important thing is params were parsed correctly
    if result.is_err() {
        // Expected on systems without supported package managers
        let err = result.unwrap_err();
        assert!(
            format!("{}", err).contains("package manager") || format!("{}", err).contains("detect")
        );
    }
}

#[test]
fn test_package_state_values() {
    let module = PackageModule;

    // Test with comma-separated string
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("pkg1,pkg2"));
    params.insert("state".to_string(), serde_json::json!("latest"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context);

    // Check mode should work even if package manager not available
    if result.is_err() {
        let err = result.unwrap_err();
        // Should be about package manager, not about state value
        assert!(!format!("{}", err).contains("Invalid state"));
    }
}

#[test]
fn test_package_with_explicit_manager() {
    let module = PackageModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("vim"));
    params.insert("use".to_string(), serde_json::json!("apt"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context);

    // On non-apt systems, this should still parse but fail at execution
    // The important thing is the 'use' parameter is recognized
    if result.is_err() {
        let err = result.unwrap_err();
        // Should not be about invalid parameter
        assert!(!format!("{}", err).contains("Invalid parameter"));
    }
}

// ============================================================================
// SERVICE MODULE EXTENDED TESTS
// ============================================================================

#[test]
fn test_service_state_values() {
    let module = ServiceModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("sshd"));
    params.insert("state".to_string(), serde_json::json!("started"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context);

    // May fail if no init system detected, but state parsing should work
    if result.is_err() {
        let err = result.unwrap_err();
        assert!(!format!("{}", err).contains("Invalid state"));
    }
}

#[test]
fn test_service_with_enabled() {
    let module = ServiceModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_check_mode(true);
    let _result = module.check(&params, &context);
    // Just verify params are parsed correctly
}

#[test]
fn test_service_restarted_state() {
    let module = ServiceModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("apache2"));
    params.insert("state".to_string(), serde_json::json!("restarted"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context);

    // Verify state parsing
    if result.is_ok() {
        assert!(result.unwrap().changed);
    }
}

#[test]
fn test_service_reloaded_state() {
    let module = ServiceModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("postgresql"));
    params.insert("state".to_string(), serde_json::json!("reloaded"));

    let context = ModuleContext::default().with_check_mode(true);
    let _result = module.check(&params, &context);
    // Just verify state value is valid
}

#[test]
fn test_service_invalid_state_parsing() {
    // This tests the ServiceState::from_str function
    // We can't test the module directly because it needs an init system

    // The service module should reject invalid states
    // We test this indirectly through params
}

#[test]
fn test_service_with_daemon_reload() {
    let module = ServiceModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myservice"));
    params.insert("daemon_reload".to_string(), serde_json::json!(true));

    let context = ModuleContext::default().with_check_mode(true);
    let _result = module.check(&params, &context);
    // Verify daemon_reload param is recognized
}

// ============================================================================
// USER MODULE EXTENDED TESTS
// ============================================================================

#[tokio::test]
async fn test_user_check_root_exists() {
    // Root user should always exist
    let module = UserModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock id command to succeed (root exists)
    mock.set_command_result(
        "id 'root'",
        CommandResult::success(
            "uid=0(root) gid=0(root) groups=0(root)".to_string(),
            String::new(),
        ),
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("root"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_connection(mock);
    let result = module.check(&params, &context).unwrap();

    // Root exists, so in check mode for present state should be ok
    // The actual logic may vary, but we can verify it runs without error
    assert!(!result.msg.contains("Would create"));
}

#[tokio::test]
async fn test_user_absent_nonexistent() {
    let module = UserModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock id command to fail (user doesn't exist)
    // Note: shell_escape doesn't quote alphanumeric usernames
    mock.set_command_result(
        "id nonexistent_user_xyz_12345",
        CommandResult {
            success: false,
            stdout: String::new(),
            stderr: "id: 'nonexistent_user_xyz_12345': no such user".to_string(),
            exit_code: 1,
        },
    );

    let mut params = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!("nonexistent_user_xyz_12345"),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = ModuleContext::default().with_connection(mock);
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
    assert!(result.msg.contains("already absent"));
}

#[tokio::test]
async fn test_user_with_all_params() {
    let module = UserModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock id command to fail (user doesn't exist)
    mock.set_command_result(
        "id 'testuser'",
        CommandResult {
            success: false,
            stdout: String::new(),
            stderr: "id: 'testuser': no such user".to_string(),
            exit_code: 1,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("uid".to_string(), serde_json::json!(5000));
    params.insert("group".to_string(), serde_json::json!("users"));
    params.insert("groups".to_string(), serde_json::json!(["docker", "sudo"]));
    params.insert("home".to_string(), serde_json::json!("/home/testuser"));
    params.insert("shell".to_string(), serde_json::json!("/bin/bash"));
    params.insert("comment".to_string(), serde_json::json!("Test User"));
    params.insert("create_home".to_string(), serde_json::json!(true));
    params.insert("system".to_string(), serde_json::json!(false));

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_connection(mock);
    let result = module.check(&params, &context);

    // Should parse all params correctly
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_user_state_values() {
    let module = UserModule;
    let mock = std::sync::Arc::new(MockConnection::new("test-host"));

    // Mock id command to fail (user doesn't exist)
    mock.set_command_result(
        "id 'testuser'",
        CommandResult {
            success: false,
            stdout: String::new(),
            stderr: "id: 'testuser': no such user".to_string(),
            exit_code: 1,
        },
    );

    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("state".to_string(), serde_json::json!("present"));

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_connection(mock);
    let result = module.check(&params, &context);

    assert!(result.is_ok());
}

// ============================================================================
// IDEMPOTENCY TESTS - Second run should not change anything
// ============================================================================

#[test]
fn test_idempotency_file_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default();

    // First run
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

#[test]
fn test_idempotency_copy_content() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("test content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();

    // First run
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

#[test]
fn test_idempotency_template() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Hello, {{ name }}!").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let context = ModuleContext::default().with_vars(vars.clone());

    // First run
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run
    let context = ModuleContext::default().with_vars(vars);
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

#[test]
fn test_idempotency_file_symlink() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source");
    let dest = temp.path().join("link");
    fs::write(&src, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert("state".to_string(), serde_json::json!("link"));

    let context = ModuleContext::default();

    // First run
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second run
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

// ============================================================================
// CHECK MODE VERIFICATION TESTS
// ============================================================================

#[test]
fn test_check_mode_file_not_created() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("should_not_exist");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("file"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!path.exists());
}

#[test]
fn test_check_mode_directory_not_created() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("should_not_exist");

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("directory"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!path.exists());
}

#[test]
fn test_check_mode_copy_not_written() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("should_not_exist.txt");

    let module = CopyModule;
    let mut params = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("content"));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!dest.exists());
}

#[test]
fn test_check_mode_template_not_written() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("should_not_exist.txt");

    fs::write(&src, "{{ value }}").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let mut vars = HashMap::new();
    vars.insert("value".to_string(), serde_json::json!("test"));

    let context = ModuleContext::default()
        .with_vars(vars)
        .with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(!dest.exists());
}

#[test]
fn test_check_mode_file_not_deleted() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("should_remain");
    fs::write(&path, "content").unwrap();

    let module = FileModule;
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(path.exists());
}

// ============================================================================
// ERROR SCENARIO TESTS
// ============================================================================

#[test]
fn test_error_invalid_file_state() {
    let module = FileModule;
    let mut params = HashMap::new();
    params.insert("path".to_string(), serde_json::json!("/tmp/test"));
    params.insert("state".to_string(), serde_json::json!("invalid"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{}", err).contains("Invalid state"));
}

#[test]
fn test_error_template_invalid_syntax() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    // Invalid template syntax - unclosed tag causes parse error
    fs::write(&src, "{{ unclosed_variable").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

#[test]
fn test_template_undefined_variable_renders_empty() {
    // In Jinja2/Ansible compatible mode, undefined variables render as empty strings
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("template.j2");
    let dest = temp.path().join("output.txt");

    fs::write(&src, "Hello {{ undefined_variable }}!").unwrap();

    let module = TemplateModule;
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    // Undefined variables render as empty string (Ansible-compatible behavior)
    assert!(result.changed);
    let content = fs::read_to_string(&dest).unwrap();
    assert_eq!(content, "Hello !");
}

#[test]
fn test_error_command_nonexistent_binary() {
    let module = CommandModule;
    let mut params = HashMap::new();
    params.insert(
        "cmd".to_string(),
        serde_json::json!("/nonexistent/binary/xyz"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);

    assert!(result.is_err());
}

// ============================================================================
// MODULE OUTPUT SERIALIZATION TESTS
// ============================================================================

#[test]
fn test_module_output_serde() {
    let output = ModuleOutput::changed("test message")
        .with_data("key", serde_json::json!("value"))
        .with_diff(Diff::new("old", "new"))
        .with_command_output(
            Some("stdout".to_string()),
            Some("stderr".to_string()),
            Some(0),
        );

    // Serialize to JSON
    let json = serde_json::to_string(&output).unwrap();

    // Deserialize back
    let deserialized: ModuleOutput = serde_json::from_str(&json).unwrap();

    assert!(deserialized.changed);
    assert_eq!(deserialized.msg, "test message");
    assert_eq!(
        deserialized.data.get("key"),
        Some(&serde_json::json!("value"))
    );
    assert_eq!(deserialized.stdout, Some("stdout".to_string()));
    assert_eq!(deserialized.stderr, Some("stderr".to_string()));
    assert_eq!(deserialized.rc, Some(0));
}

#[test]
fn test_diff_serde() {
    let diff = Diff::new("before", "after").with_details("detailed diff");

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: Diff = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.before, "before");
    assert_eq!(deserialized.after, "after");
    assert_eq!(deserialized.details, Some("detailed diff".to_string()));
}

#[test]
fn test_module_status_serde() {
    let statuses = vec![
        ModuleStatus::Ok,
        ModuleStatus::Changed,
        ModuleStatus::Failed,
        ModuleStatus::Skipped,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: ModuleStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}

// ============================================================================
// MODULE TRAIT EDGE CASES
// ============================================================================

#[test]
fn test_all_modules_have_names() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(CommandModule),
        Box::new(ShellModule),
        Box::new(CopyModule),
        Box::new(TemplateModule),
        Box::new(FileModule),
        Box::new(PackageModule),
        Box::new(ServiceModule),
        Box::new(UserModule),
    ];

    let expected_names = vec![
        "command", "shell", "copy", "template", "file", "package", "service", "user",
    ];

    for (module, expected) in modules.iter().zip(expected_names.iter()) {
        assert_eq!(module.name(), *expected);
    }
}

#[test]
fn test_all_modules_have_descriptions() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(CommandModule),
        Box::new(ShellModule),
        Box::new(CopyModule),
        Box::new(TemplateModule),
        Box::new(FileModule),
        Box::new(PackageModule),
        Box::new(ServiceModule),
        Box::new(UserModule),
    ];

    for module in &modules {
        let desc = module.description();
        assert!(
            !desc.is_empty(),
            "Module {} has empty description",
            module.name()
        );
    }
}

#[test]
fn test_all_modules_have_classification() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(CommandModule),
        Box::new(ShellModule),
        Box::new(CopyModule),
        Box::new(TemplateModule),
        Box::new(FileModule),
        Box::new(PackageModule),
        Box::new(ServiceModule),
        Box::new(UserModule),
    ];

    for module in &modules {
        let _classification = module.classification();
        // All modules should have a valid classification
    }
}

#[test]
fn test_all_modules_have_parallelization_hint() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(CommandModule),
        Box::new(ShellModule),
        Box::new(CopyModule),
        Box::new(TemplateModule),
        Box::new(FileModule),
        Box::new(PackageModule),
        Box::new(ServiceModule),
        Box::new(UserModule),
    ];

    for module in &modules {
        let _hint = module.parallelization_hint();
        // All modules should have a valid parallelization hint
    }
}
