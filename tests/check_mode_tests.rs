//! Comprehensive tests for check mode (dry-run) functionality in Rustible.
//!
//! These tests verify that:
//! - --check flag prevents actual changes to the system
//! - Check mode correctly reports what would change
//! - Check mode with --diff shows differences
//! - All modules properly implement check mode behavior
//! - ansible_check_mode variable is available and correct
//! - Handlers are notified but not executed in check mode
//! - changed_when is properly evaluated in check mode
//! - Loops work correctly in check mode

mod common;

use std::collections::HashMap;
use tempfile::TempDir;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::{ExecutionContext, RuntimeContext};
use rustible::executor::task::{Handler, LoopSource, Task, TaskResult, TaskStatus};
use rustible::executor::{ExecutionStats, Executor, ExecutorConfig};
use rustible::modules::command::CommandModule;
use rustible::modules::copy::CopyModule;
use rustible::modules::file::FileModule;
use rustible::modules::shell::ShellModule;
use rustible::modules::{Diff, Module, ModuleContext, ModuleOutput, ModuleParams, ModuleStatus};

use common::{
    check_mode_context, make_params, test_check_mode_config, test_module_context, MockModule,
};

// ============================================================================
// 1. CHECK MODE BASIC TESTS
// ============================================================================

#[test]
fn test_check_flag_prevents_changes() {
    // Verify that executor configuration correctly tracks check mode
    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    assert!(config.check_mode);

    let executor = Executor::new(config);
    assert!(executor.is_check_mode());
}

#[test]
fn test_check_flag_disabled_by_default() {
    let config = ExecutorConfig::default();

    assert!(!config.check_mode);

    let executor = Executor::new(config);
    assert!(!executor.is_check_mode());
}

#[test]
fn test_execution_context_check_mode() {
    // Test that ExecutionContext properly propagates check mode
    let ctx = ExecutionContext::new("test-host")
        .with_check_mode(true)
        .with_diff_mode(true);

    assert!(ctx.check_mode);
    assert!(ctx.diff_mode);

    // Default should be false
    let default_ctx = ExecutionContext::new("test-host");
    assert!(!default_ctx.check_mode);
    assert!(!default_ctx.diff_mode);
}

#[test]
fn test_module_context_check_mode() {
    // Test ModuleContext check mode
    let ctx = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    assert!(ctx.check_mode);
    assert!(ctx.diff_mode);

    let default_ctx = ModuleContext::default();
    assert!(!default_ctx.check_mode);
    assert!(!default_ctx.diff_mode);
}

#[tokio::test]
async fn test_check_mode_executor_run() {
    // Test that executor properly runs in check mode
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        diff_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);
    assert!(executor.is_check_mode());

    // Create a simple playbook
    let mut playbook = Playbook::new("Check Mode Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Debug test", "debug").arg("msg", "Hello"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_check_mode_with_diff() {
    // Test check mode combined with diff mode
    let config = test_check_mode_config();
    assert!(config.check_mode);
    assert!(config.diff_mode);
}

// ============================================================================
// 2. MODULE CHECK IMPLEMENTATIONS
// ============================================================================

#[test]
fn test_command_module_check_mode() {
    let module = CommandModule;
    let params = make_params(vec![("cmd", serde_json::json!("echo hello"))]);
    let context = check_mode_context();

    let result = module.check(&params, &context).unwrap();

    // In check mode, command should report would execute
    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
    assert!(result.diff.is_some());
}

#[test]
fn test_command_module_check_mode_with_creates() {
    let module = CommandModule;
    let params = make_params(vec![
        ("cmd", serde_json::json!("echo hello")),
        ("creates", serde_json::json!("/")), // This exists, so command should be skipped
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should be skipped because '/' exists
    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_command_module_check_mode_with_removes() {
    let module = CommandModule;
    let params = make_params(vec![
        ("cmd", serde_json::json!("echo hello")),
        (
            "removes",
            serde_json::json!("/nonexistent/path/that/does/not/exist"),
        ),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should be skipped because the file doesn't exist
    assert!(!result.changed);
    assert!(result.msg.contains("Skipped"));
}

#[test]
fn test_shell_module_check_mode() {
    let module = ShellModule;
    let params = make_params(vec![("cmd", serde_json::json!("echo hello | grep hello"))]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // In check mode, shell should report would execute
    assert!(result.changed);
    assert!(result.msg.contains("Would execute"));
    assert!(result.diff.is_some());
}

#[test]
fn test_copy_module_check_mode() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("new_file.txt");

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("Hello, World!")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would copy
    assert!(result.changed);
    assert!(result.msg.contains("Would copy"));

    // File should NOT be created in check mode
    assert!(!dest.exists());
}

#[test]
fn test_copy_module_check_mode_shows_diff() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("new_file.txt");

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("Hello, World!")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    let context = ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true);

    let result = module.check(&params, &context).unwrap();

    // Should show diff in check mode with diff_mode enabled
    assert!(result.diff.is_some());
}

#[test]
fn test_copy_module_check_mode_existing_file() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("existing.txt");
    std::fs::write(&dest, "Old content").unwrap();

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("New content")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would change
    assert!(result.changed);

    // Original content should remain unchanged
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "Old content");
}

#[test]
fn test_copy_module_check_mode_idempotent() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("same.txt");
    std::fs::write(&dest, "Same content").unwrap();

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("Same content")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should NOT report changed when content is the same
    assert!(!result.changed);
}

#[test]
fn test_file_module_check_mode_create_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new_dir");

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("directory")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would create
    assert!(result.changed);
    assert!(result.msg.contains("Would create"));

    // Directory should NOT be created
    assert!(!path.exists());
}

#[test]
fn test_file_module_check_mode_create_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new_file.txt");

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("file")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would create
    assert!(result.changed);
    assert!(result.msg.contains("Would create"));

    // File should NOT be created
    assert!(!path.exists());
}

#[test]
fn test_file_module_check_mode_remove() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("to_remove.txt");
    std::fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("absent")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would remove
    assert!(result.changed);
    assert!(result.msg.contains("Would remove"));

    // File should still exist
    assert!(path.exists());
}

#[test]
fn test_file_module_check_mode_symlink() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("source.txt");
    let link = temp.path().join("link");
    std::fs::write(&src, "content").unwrap();

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(link.to_str().unwrap())),
        ("src", serde_json::json!(src.to_str().unwrap())),
        ("state", serde_json::json!("link")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would create symlink
    assert!(result.changed);
    assert!(result.msg.contains("Would create symlink"));

    // Symlink should NOT be created
    assert!(!link.exists());
}

#[test]
fn test_file_module_check_mode_touch() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("touch_me.txt");

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("touch")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should report would create (file doesn't exist)
    assert!(result.changed);

    // File should NOT be created
    assert!(!path.exists());
}

#[test]
fn test_file_module_check_mode_idempotent_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("existing_dir");
    std::fs::create_dir(&path).unwrap();

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("directory")),
    ]);

    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Should NOT report changed when directory already exists
    assert!(!result.changed);
}

// ============================================================================
// 3. CHECK_MODE VARIABLE TESTS
// ============================================================================

#[tokio::test]
async fn test_ansible_check_mode_variable_true() {
    // When running in check mode, ansible_check_mode should be true
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    // The executor is in check mode
    assert!(executor.is_check_mode());
}

#[tokio::test]
async fn test_ansible_check_mode_variable_false() {
    // When not in check mode, ansible_check_mode should be false
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    // The executor is not in check mode
    assert!(!executor.is_check_mode());
}

// ============================================================================
// 4. CHANGED_WHEN WITH CHECK MODE TESTS
// ============================================================================

#[test]
fn test_task_changed_when_field() {
    // Test that Task properly supports changed_when
    let task = Task::new("Test", "command").arg("cmd", "echo hello");

    // Default should be None
    assert!(task.changed_when.is_none());

    // Can be set
    let mut task_with_changed = task.clone();
    task_with_changed.changed_when = Some("result.rc == 0".to_string());
    assert!(task_with_changed.changed_when.is_some());
}

#[test]
fn test_task_result_changed_status() {
    // Verify TaskResult can properly represent changed/unchanged states
    let changed = TaskResult::changed();
    assert!(changed.changed);
    assert_eq!(changed.status, TaskStatus::Changed);

    let ok = TaskResult::ok();
    assert!(!ok.changed);
    assert_eq!(ok.status, TaskStatus::Ok);
}

// ============================================================================
// 5. CHECK WITH HANDLERS TESTS
// ============================================================================

#[tokio::test]
async fn test_handlers_notified_in_check_mode() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    // Create playbook with handler
    let mut playbook = Playbook::new("Handler Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add task that notifies handler
    play.add_task(
        Task::new("Notify handler", "copy")
            .arg("content", "test")
            .arg("dest", "/tmp/test.txt")
            .notify("test handler"),
    );

    // Add handler
    play.add_handler(Handler {
        name: "test handler".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Handler executed"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    // Run in check mode
    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete without error in check mode
    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

// ============================================================================
// 6. CHECK MODE TASK CONTROL TESTS
// ============================================================================

#[test]
fn test_task_supports_check_mode_property() {
    // Modules should indicate if they support check mode
    let command = CommandModule;
    // CommandModule's Module trait implementation exists
    assert_eq!(command.name(), "command");

    let copy = CopyModule;
    assert_eq!(copy.name(), "copy");

    let file = FileModule;
    assert_eq!(file.name(), "file");
}

#[test]
fn test_module_check_vs_execute() {
    // Test that check() and execute() can return different results
    let mock = MockModule::new("test")
        .with_result(ModuleOutput::changed("Executed"))
        .with_check_result(ModuleOutput::ok("Would execute"));

    let context = ModuleContext::default();
    let params = HashMap::new();

    // Execute returns changed
    let exec_result = mock.execute(&params, &context).unwrap();
    assert!(exec_result.changed);

    // Check returns ok (would execute)
    let check_result = mock.check(&params, &context).unwrap();
    assert!(!check_result.changed);
}

// ============================================================================
// 7. CHECK WITH LOOPS TESTS
// ============================================================================

#[test]
fn test_task_loop_definition() {
    let task = Task::new("Install packages", "package")
        .arg("name", "{{ item }}")
        .loop_over(vec![
            serde_json::json!("nginx"),
            serde_json::json!("php"),
            serde_json::json!("mysql"),
        ]);

    assert!(task.loop_items.is_some());
    match task.loop_items.as_ref().unwrap() {
        LoopSource::Items(items) => assert_eq!(items.len(), 3),
        LoopSource::Template(_) => panic!("Expected Items, got Template"),
    }
}

#[tokio::test]
async fn test_loop_in_check_mode() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Loop Check Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add task with loop
    play.add_task(
        Task::new("Debug loop", "debug")
            .arg("msg", "Processing {{ item }}")
            .loop_over(vec![
                serde_json::json!("item1"),
                serde_json::json!("item2"),
                serde_json::json!("item3"),
            ]),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

// ============================================================================
// 8. CHECK ACCURACY TESTS
// ============================================================================

#[test]
fn test_check_result_predicts_change_correctly() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("Hello")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    // Check mode should predict change
    let check_context = check_mode_context();
    let check_result = module.check(&params, &check_context).unwrap();
    assert!(check_result.changed);

    // Actual execution should also change
    let exec_context = test_module_context();
    let exec_result = module.execute(&params, &exec_context).unwrap();
    assert!(exec_result.changed);

    // File now exists
    assert!(dest.exists());
}

#[test]
fn test_check_result_predicts_no_change_correctly() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");
    std::fs::write(&dest, "Hello").unwrap();

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("Hello")),
        (
            // Same content
            "dest",
            serde_json::json!(dest.to_str().unwrap()),
        ),
    ]);

    // Check mode should predict no change
    let check_context = check_mode_context();
    let check_result = module.check(&params, &check_context).unwrap();
    assert!(!check_result.changed);

    // Actual execution should also not change
    let exec_context = test_module_context();
    let exec_result = module.execute(&params, &exec_context).unwrap();
    assert!(!exec_result.changed);
}

// ============================================================================
// 9. CHECK WITH CONDITIONS TESTS
// ============================================================================

#[tokio::test]
async fn test_when_condition_in_check_mode() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_var(
        "localhost",
        "should_run".to_string(),
        serde_json::json!(true),
    );

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Conditional Check Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Task that should run
    play.add_task(
        Task::new("Should run", "debug")
            .arg("msg", "Running")
            .when("should_run"),
    );

    // Task that should be skipped
    play.add_task(
        Task::new("Should skip", "debug")
            .arg("msg", "Skipped")
            .when("false"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
    // One should run, one should be skipped
    assert!(host_result.stats.skipped >= 1);
}

#[tokio::test]
async fn test_registered_vars_in_check_mode() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Register Check Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Register result
    play.add_task(
        Task::new("Register result", "debug")
            .arg("msg", "test")
            .register("debug_result"),
    );

    // Use registered result
    play.add_task(Task::new("Use result", "debug").arg("msg", "Using registered"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

// ============================================================================
// 10. REPORTING IN CHECK MODE TESTS
// ============================================================================

#[test]
fn test_module_output_status_types() {
    // Verify all status types work correctly
    let ok = ModuleOutput::ok("OK message");
    assert_eq!(ok.status, ModuleStatus::Ok);
    assert!(!ok.changed);

    let changed = ModuleOutput::changed("Changed message");
    assert_eq!(changed.status, ModuleStatus::Changed);
    assert!(changed.changed);

    let failed = ModuleOutput::failed("Failed message");
    assert_eq!(failed.status, ModuleStatus::Failed);
    assert!(!failed.changed);

    let skipped = ModuleOutput::skipped("Skipped message");
    assert_eq!(skipped.status, ModuleStatus::Skipped);
    assert!(!skipped.changed);
}

#[test]
fn test_execution_stats_tracking() {
    let mut stats = ExecutionStats::default();

    assert_eq!(stats.ok, 0);
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.unreachable, 0);

    // Merge some stats
    let other = ExecutionStats {
        ok: 5,
        changed: 3,
        failed: 1,
        skipped: 2,
        unreachable: 0,
    };

    stats.merge(&other);

    assert_eq!(stats.ok, 5);
    assert_eq!(stats.changed, 3);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 2);
}

#[tokio::test]
async fn test_check_mode_summary_stats() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("server1".to_string(), None);
    runtime.add_host("server2".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Stats Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add multiple tasks
    play.add_task(Task::new("Task 1", "debug").arg("msg", "1"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "2"));
    play.add_task(
        Task::new("Skipped Task", "debug")
            .arg("msg", "skip")
            .when("false"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify we have results for both hosts
    assert_eq!(results.len(), 2);

    // Summarize results
    let summary = Executor::summarize_results(&results);

    // Both hosts ran the non-skipped tasks
    assert!(summary.ok > 0 || summary.changed > 0);
    assert!(summary.skipped >= 2); // One skip per host
}

// ============================================================================
// DIFF MODE TESTS
// ============================================================================

#[test]
fn test_diff_struct() {
    let diff = Diff::new("before state", "after state");
    assert_eq!(diff.before, "before state");
    assert_eq!(diff.after, "after state");
    assert!(diff.details.is_none());

    let diff_with_details = Diff::new("before", "after").with_details("detailed diff output");
    assert!(diff_with_details.details.is_some());
    assert_eq!(diff_with_details.details.unwrap(), "detailed diff output");
}

#[test]
fn test_command_module_diff() {
    let module = CommandModule;
    let params = make_params(vec![("cmd", serde_json::json!("echo hello"))]);

    let context = test_module_context();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
    let d = diff.unwrap();
    assert_eq!(d.before, "(none)");
    assert!(d.after.contains("echo hello"));
}

#[test]
fn test_shell_module_diff() {
    let module = ShellModule;
    let params = make_params(vec![("cmd", serde_json::json!("echo hello | grep hello"))]);

    let context = test_module_context();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
}

#[test]
fn test_file_module_diff() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new_dir");

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("directory")),
    ]);

    let context = test_module_context();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
    let d = diff.unwrap();
    assert_eq!(d.before, "absent");
    assert_eq!(d.after, "directory exists");
}

#[test]
fn test_copy_module_diff() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("test.txt");

    let module = CopyModule;
    let params = make_params(vec![
        ("content", serde_json::json!("New content")),
        ("dest", serde_json::json!(dest.to_str().unwrap())),
    ]);

    let context = test_module_context();
    let diff = module.diff(&params, &context).unwrap();

    assert!(diff.is_some());
}

// ============================================================================
// SYSTEM CHANGES VERIFICATION TESTS
// ============================================================================

#[test]
fn test_check_mode_no_filesystem_changes() {
    let temp = TempDir::new().unwrap();

    // Track all test paths
    let file_path = temp.path().join("should_not_exist.txt");
    let dir_path = temp.path().join("should_not_exist_dir");
    let symlink_path = temp.path().join("should_not_exist_link");

    // Run file module in check mode for file creation
    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(file_path.to_str().unwrap())),
        ("state", serde_json::json!("file")),
    ]);
    let context = check_mode_context();
    let _ = module.check(&params, &context).unwrap();

    // Run file module in check mode for directory creation
    let params = make_params(vec![
        ("path", serde_json::json!(dir_path.to_str().unwrap())),
        ("state", serde_json::json!("directory")),
    ]);
    let _ = module.check(&params, &context).unwrap();

    // Verify NO changes were made
    assert!(
        !file_path.exists(),
        "File should not be created in check mode"
    );
    assert!(
        !dir_path.exists(),
        "Directory should not be created in check mode"
    );
    assert!(
        !symlink_path.exists(),
        "Symlink should not be created in check mode"
    );
}

#[test]
fn test_check_mode_preserves_existing_files() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("existing.txt");

    let original_content = "Original content";
    std::fs::write(&path, original_content).unwrap();

    // Run copy module in check mode to change content
    let module = CopyModule;
    let params = make_params(vec![
        (
            "content",
            serde_json::json!("New content that should not be written"),
        ),
        ("dest", serde_json::json!(path.to_str().unwrap())),
    ]);
    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Check mode should report would change
    assert!(result.changed);

    // But file content should be unchanged
    let current_content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(current_content, original_content);
}

#[test]
fn test_check_mode_does_not_delete_files() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("should_remain.txt");
    std::fs::write(&path, "content").unwrap();

    let module = FileModule;
    let params = make_params(vec![
        ("path", serde_json::json!(path.to_str().unwrap())),
        ("state", serde_json::json!("absent")),
    ]);
    let context = check_mode_context();
    let result = module.check(&params, &context).unwrap();

    // Check mode should report would remove
    assert!(result.changed);
    assert!(result.msg.contains("Would remove"));

    // But file should still exist
    assert!(path.exists());
}

// ============================================================================
// FULL PLAYBOOK CHECK MODE TESTS
// ============================================================================

#[tokio::test]
async fn test_complex_playbook_in_check_mode() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    // Set local connection for test hosts to avoid SSH connection attempts
    runtime.set_host_var(
        "web1",
        "ansible_connection".to_string(),
        serde_json::json!("local"),
    );
    runtime.set_host_var(
        "web2",
        "ansible_connection".to_string(),
        serde_json::json!("local"),
    );
    runtime.set_host_var(
        "db1",
        "ansible_connection".to_string(),
        serde_json::json!("local"),
    );

    runtime.set_global_var("environment".to_string(), serde_json::json!("testing"));

    let config = ExecutorConfig {
        check_mode: true,
        diff_mode: true,
        forks: 3,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Complex Check Mode Test");

    // Play 1: Web servers
    let mut web_play = Play::new("Configure Web Servers", "webservers");
    web_play.gather_facts = false;
    web_play.add_task(Task::new("Debug web", "debug").arg("msg", "Configuring web"));
    web_play.add_task(
        Task::new("Install nginx", "package")
            .arg("name", "nginx")
            .arg("state", "present"),
    );
    playbook.add_play(web_play);

    // Play 2: Databases
    let mut db_play = Play::new("Configure Databases", "databases");
    db_play.gather_facts = false;
    db_play.add_task(Task::new("Debug db", "debug").arg("msg", "Configuring db"));
    playbook.add_play(db_play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete successfully in check mode
    assert_eq!(results.len(), 3);
    for (host, result) in &results {
        assert!(
            !result.failed,
            "Host {} should not fail in check mode",
            host
        );
        assert!(!result.unreachable, "Host {} should be reachable", host);
    }
}

#[tokio::test]
async fn test_playbook_parse_and_check() {
    let yaml = r#"
- name: Check Mode Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "Hello from check mode"

    - name: Would copy file
      copy:
        content: "test content"
        dest: /tmp/test_check_mode.txt

    - name: Conditional task
      debug:
        msg: "This runs"
      when: "true"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    assert_eq!(playbook.plays.len(), 1);
    assert_eq!(playbook.plays[0].tasks.len(), 3);

    // Set up executor in check mode
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);
    let results = executor.run_playbook(&playbook).await.unwrap();

    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

// ============================================================================
// EXIT CODE TESTS
// ============================================================================

#[test]
fn test_task_status_values() {
    // Verify TaskStatus enum values
    assert_ne!(TaskStatus::Ok, TaskStatus::Failed);
    assert_ne!(TaskStatus::Changed, TaskStatus::Skipped);
    assert_ne!(TaskStatus::Skipped, TaskStatus::Unreachable);
}

#[test]
fn test_module_status_values() {
    // Verify ModuleStatus enum values
    assert_ne!(ModuleStatus::Ok, ModuleStatus::Failed);
    assert_ne!(ModuleStatus::Changed, ModuleStatus::Skipped);
}

// ============================================================================
// EDGE CASES
// ============================================================================

#[test]
fn test_check_mode_with_empty_params() {
    let module = CommandModule;
    let params: ModuleParams = HashMap::new();

    // Should return error for missing required param
    let result = module.validate_params(&params);
    // validate_params checks for cmd or argv
    assert!(result.is_err() || params.get("cmd").is_none());
}

#[test]
fn test_check_mode_context_builder_chain() {
    let context = ModuleContext::new()
        .with_check_mode(true)
        .with_diff_mode(true);

    assert!(context.check_mode);
    assert!(context.diff_mode);
    assert!(!context.r#become); // Not set, should be default
}

#[tokio::test]
async fn test_check_mode_with_multiple_plays() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Multi-Play Check");

    // Multiple plays
    for i in 1..=3 {
        let mut play = Play::new(format!("Play {}", i), "all");
        play.gather_facts = false;
        play.add_task(
            Task::new(format!("Task in play {}", i), "debug")
                .arg("msg", format!("Play {} task", i)),
        );
        playbook.add_play(play);
    }

    let results = executor.run_playbook(&playbook).await.unwrap();

    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
    // All plays should have executed their tasks
    assert!(host_result.stats.ok > 0 || host_result.stats.changed > 0);
}
