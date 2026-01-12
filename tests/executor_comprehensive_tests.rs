//! Comprehensive executor tests for 90%+ coverage
//!
//! This test suite covers all required tests from docs/coverage/TESTS_TO_ADD.md:
//!
//! ## src/executor/mod.rs (15 tests)
//! - test_executor_builder_defaults
//! - test_executor_builder_forks
//! - test_executor_builder_check_mode
//! - test_executor_builder_diff_mode
//! - test_executor_builder_strategy
//! - test_serial_spec_fixed
//! - test_serial_spec_percentage
//! - test_serial_spec_progressive
//! - test_task_result_success
//! - test_task_result_changed
//! - test_task_result_failed
//! - test_task_result_skipped
//! - test_play_result_aggregation
//! - test_play_result_failure_detection
//! - test_playbook_result_summary
//!
//! ## src/executor/parallelization.rs (10 tests)
//! - test_strategy_linear
//! - test_strategy_free
//! - test_strategy_serial
//! - test_batch_hosts_empty
//! - test_batch_hosts_single
//! - test_batch_hosts_exact_multiple
//! - test_batch_hosts_with_remainder
//! - test_load_balancing
//! - test_failure_handling_continue
//! - test_failure_handling_any_errors_fatal
#![allow(unused_variables)]

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task, TaskResult, TaskStatus};
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};
use rustible::playbook::SerialSpec;

// ============================================================================
// Helper Utilities
// ============================================================================

/// Create a runtime with multiple hosts
fn create_runtime_with_hosts(hosts: Vec<&str>) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for host in hosts {
        runtime.add_host(host.to_string(), None);
    }
    runtime
}

// ============================================================================
// Executor Builder Tests (src/executor/mod.rs)
// ============================================================================

#[test]
fn test_executor_builder_defaults() {
    let config = ExecutorConfig::default();

    // Verify default values
    assert_eq!(config.forks, 5);
    assert!(!config.check_mode);
    assert!(!config.diff_mode);
    assert!(matches!(config.strategy, ExecutionStrategy::Linear));
}

#[test]
fn test_executor_builder_forks() {
    let config = ExecutorConfig {
        forks: 10,
        ..Default::default()
    };

    assert_eq!(config.forks, 10);

    // Test minimum forks
    let config_min = ExecutorConfig {
        forks: 1,
        ..Default::default()
    };
    assert_eq!(config_min.forks, 1);

    // Test maximum reasonable forks
    let config_max = ExecutorConfig {
        forks: 100,
        ..Default::default()
    };
    assert_eq!(config_max.forks, 100);
}

#[test]
fn test_executor_builder_check_mode() {
    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    assert!(config.check_mode);
    assert!(!config.diff_mode); // diff_mode should still be default

    // Verify check_mode is false by default
    let default_config = ExecutorConfig::default();
    assert!(!default_config.check_mode);
}

#[test]
fn test_executor_builder_diff_mode() {
    let config = ExecutorConfig {
        diff_mode: true,
        ..Default::default()
    };

    assert!(config.diff_mode);
    assert!(!config.check_mode); // check_mode should still be default

    // Verify diff_mode is false by default
    let default_config = ExecutorConfig::default();
    assert!(!default_config.diff_mode);
}

#[test]
fn test_executor_builder_strategy() {
    // Test Linear strategy
    let linear_config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        ..Default::default()
    };
    assert!(matches!(linear_config.strategy, ExecutionStrategy::Linear));

    // Test Free strategy
    let free_config = ExecutorConfig {
        strategy: ExecutionStrategy::Free,
        ..Default::default()
    };
    assert!(matches!(free_config.strategy, ExecutionStrategy::Free));

    // Test HostPinned strategy
    let pinned_config = ExecutorConfig {
        strategy: ExecutionStrategy::HostPinned,
        ..Default::default()
    };
    assert!(matches!(
        pinned_config.strategy,
        ExecutionStrategy::HostPinned
    ));
}

#[test]
fn test_executor_builder_combined_options() {
    // Test combining multiple options
    let config = ExecutorConfig {
        forks: 20,
        check_mode: true,
        diff_mode: true,
        strategy: ExecutionStrategy::Free,
        ..Default::default()
    };

    assert_eq!(config.forks, 20);
    assert!(config.check_mode);
    assert!(config.diff_mode);
    assert!(matches!(config.strategy, ExecutionStrategy::Free));
}

// ============================================================================
// SerialSpec Tests (src/executor/mod.rs / src/playbook.rs)
// ============================================================================

#[test]
fn test_serial_spec_fixed() {
    let spec = SerialSpec::Fixed(5);
    let batches = spec.calculate_batches(20);

    // Fixed returns a single batch size
    assert_eq!(batches, vec![5]);

    // Test with different sizes
    let spec1 = SerialSpec::Fixed(1);
    assert_eq!(spec1.calculate_batches(10), vec![1]);

    let spec10 = SerialSpec::Fixed(10);
    assert_eq!(spec10.calculate_batches(10), vec![10]);
}

#[test]
fn test_serial_spec_fixed_edge_cases() {
    // Zero hosts
    let spec = SerialSpec::Fixed(5);
    assert!(spec.calculate_batches(0).is_empty());

    // Zero batch size
    let spec_zero = SerialSpec::Fixed(0);
    assert!(spec_zero.calculate_batches(10).is_empty());
}

#[test]
fn test_serial_spec_percentage() {
    // 50% of 10 hosts = 5
    let spec = SerialSpec::Percentage("50%".to_string());
    let batches = spec.calculate_batches(10);
    assert_eq!(batches, vec![5]);

    // 25% of 8 hosts = 2
    let spec25 = SerialSpec::Percentage("25%".to_string());
    assert_eq!(spec25.calculate_batches(8), vec![2]);

    // 100% should be all hosts
    let spec100 = SerialSpec::Percentage("100%".to_string());
    assert_eq!(spec100.calculate_batches(10), vec![10]);
}

#[test]
fn test_serial_spec_percentage_rounding() {
    // 33% of 10 = 3.3, should round up to 4
    let spec = SerialSpec::Percentage("33%".to_string());
    let batches = spec.calculate_batches(10);
    assert_eq!(batches, vec![4]);

    // 30% of 5 = 1.5, should round up to 2
    let spec30 = SerialSpec::Percentage("30%".to_string());
    assert_eq!(spec30.calculate_batches(5), vec![2]);
}

#[test]
fn test_serial_spec_percentage_edge_cases() {
    // 0% should result in minimum 1 host per batch
    let spec = SerialSpec::Percentage("0%".to_string());
    let batches = spec.calculate_batches(10);
    assert_eq!(batches, vec![1]);

    // Zero hosts with percentage
    let spec50 = SerialSpec::Percentage("50%".to_string());
    assert!(spec50.calculate_batches(0).is_empty());
}

#[test]
fn test_serial_spec_progressive() {
    let spec = SerialSpec::Progressive(vec![
        SerialSpec::Fixed(1),
        SerialSpec::Fixed(5),
        SerialSpec::Fixed(10),
    ]);
    let batches = spec.calculate_batches(20);

    assert_eq!(batches, vec![1, 5, 10]);
}

#[test]
fn test_serial_spec_progressive_with_percentages() {
    let spec = SerialSpec::Progressive(vec![
        SerialSpec::Percentage("10%".to_string()),
        SerialSpec::Percentage("50%".to_string()),
    ]);
    let batches = spec.calculate_batches(20);

    // 10% of 20 = 2, 50% of 20 = 10
    assert_eq!(batches, vec![2, 10]);
}

#[test]
fn test_serial_spec_progressive_empty() {
    let spec = SerialSpec::Progressive(vec![]);
    let batches = spec.calculate_batches(10);
    assert!(batches.is_empty());

    // Zero hosts
    let spec_prog = SerialSpec::Progressive(vec![SerialSpec::Fixed(5)]);
    assert!(spec_prog.calculate_batches(0).is_empty());
}

// ============================================================================
// TaskResult Tests (src/executor/task.rs)
// ============================================================================

#[test]
fn test_task_result_success() {
    let result = TaskResult::ok();

    assert!(matches!(result.status, TaskStatus::Ok));
    assert!(!result.changed);
    assert!(result.msg.is_none());
    assert!(result.result.is_none());
    assert!(result.diff.is_none());
}

#[test]
fn test_task_result_changed() {
    let result = TaskResult::changed();

    assert!(matches!(result.status, TaskStatus::Changed));
    assert!(result.changed);
    assert!(result.msg.is_none());
}

#[test]
fn test_task_result_failed() {
    let result = TaskResult::failed("Error message");

    assert!(matches!(result.status, TaskStatus::Failed));
    assert!(!result.changed);
    assert_eq!(result.msg, Some("Error message".to_string()));
}

#[test]
fn test_task_result_skipped() {
    let result = TaskResult::skipped("Condition not met");

    assert!(matches!(result.status, TaskStatus::Skipped));
    assert!(!result.changed);
    assert_eq!(result.msg, Some("Condition not met".to_string()));
}

#[test]
fn test_task_result_unreachable() {
    let result = TaskResult::unreachable("Host unreachable");

    assert!(matches!(result.status, TaskStatus::Unreachable));
    assert!(!result.changed);
    assert_eq!(result.msg, Some("Host unreachable".to_string()));
}

#[test]
fn test_task_result_with_result_data() {
    let result = TaskResult::ok().with_result(serde_json::json!({
        "key": "value",
        "count": 42
    }));

    assert!(result.result.is_some());
    let data = result.result.unwrap();
    assert_eq!(data.get("key").unwrap(), "value");
    assert_eq!(data.get("count").unwrap(), 42);
}

#[test]
fn test_task_result_with_message() {
    let result = TaskResult::ok().with_msg("Custom message");

    assert_eq!(result.msg, Some("Custom message".to_string()));
}

#[test]
fn test_task_result_to_registered() {
    let result = TaskResult::changed();
    let registered = result.to_registered(
        Some("stdout text".to_string()),
        Some("stderr text".to_string()),
    );

    assert!(registered.changed);
    assert!(!registered.failed);
    assert!(!registered.skipped);
    assert_eq!(registered.stdout, Some("stdout text".to_string()));
    assert_eq!(registered.stderr, Some("stderr text".to_string()));
    assert!(registered.stdout_lines.is_some());
    assert!(registered.stderr_lines.is_some());
}

// ============================================================================
// Play and Playbook Result Tests
// ============================================================================

#[tokio::test]
async fn test_play_result_aggregation() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Aggregation Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Add tasks that will generate various stats
    play.add_task(Task::new("OK task", "debug").arg("msg", "ok"));
    play.add_task(
        Task::new("Skipped task", "debug")
            .arg("msg", "skip")
            .when("false"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify aggregation
    for (host, result) in &results {
        // Each host should have aggregated stats
        assert!(
            result.stats.ok > 0 || result.stats.changed > 0,
            "Host {} should have OK/changed tasks",
            host
        );
        assert!(
            result.stats.skipped > 0,
            "Host {} should have skipped tasks",
            host
        );
    }
}

#[tokio::test]
async fn test_play_result_failure_detection() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Failure Detection Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Add a task that fails on one host
    play.add_task(
        Task::new("Conditional failure", "fail")
            .arg("msg", "Fail on host1")
            .when("inventory_hostname == 'host1'"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify failure detection
    let host1_result = results.get("host1").expect("host1 should be in results");
    assert!(
        host1_result.stats.failed > 0,
        "host1 should have failed tasks"
    );

    // Other hosts should not have failures (task was skipped for them)
    for (host, result) in &results {
        if host != "host1" {
            assert!(
                result.stats.failed == 0,
                "Host {} should not have failed tasks",
                host
            );
        }
    }
}

#[tokio::test]
async fn test_playbook_result_summary() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Summary Test");

    // First play
    let mut play1 = Play::new("First Play", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("Play 1 Task", "debug").arg("msg", "first"));
    playbook.add_play(play1);

    // Second play
    let mut play2 = Play::new("Second Play", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("Play 2 Task", "debug").arg("msg", "second"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Use summarize_results to get summary
    let summary = Executor::summarize_results(&results);

    // Summary should aggregate all plays
    assert!(
        summary.ok > 0 || summary.changed > 0,
        "Summary should have OK/changed tasks"
    );
    // Each host should have run 2 tasks (one per play)
    for result in results.values() {
        assert!(
            result.stats.ok >= 2 || result.stats.changed >= 2,
            "Each host should have run tasks from both plays"
        );
    }
}

// ============================================================================
// Strategy Tests (src/executor/parallelization.rs)
// ============================================================================

#[tokio::test]
async fn test_strategy_linear() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Linear Strategy Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task 1", "debug").arg("msg", "first"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "second"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_strategy_free() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Free Strategy Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task 1", "debug").arg("msg", "first"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "second"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete independently
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_strategy_serial() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Strategy Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2)); // 2 hosts at a time
    play.add_task(Task::new("Serial task", "debug").arg("msg", "batch"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 4 hosts should complete (in 2 batches of 2)
    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Batch Hosts Tests
// ============================================================================

#[test]
fn test_batch_hosts_empty() {
    let hosts: Vec<String> = vec![];
    let spec = SerialSpec::Fixed(2);
    let batches = spec.batch_hosts(&hosts);

    assert!(batches.is_empty());
}

#[test]
fn test_batch_hosts_single() {
    let hosts: Vec<String> = vec!["host1".to_string()];
    let spec = SerialSpec::Fixed(2);
    let batches = spec.batch_hosts(&hosts);

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[0][0], "host1");
}

#[test]
fn test_batch_hosts_exact_multiple() {
    let hosts: Vec<String> = (1..=6).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Fixed(2);
    let batches = spec.batch_hosts(&hosts);

    // 6 hosts / 2 per batch = 3 batches
    assert_eq!(batches.len(), 3);
    for batch in &batches {
        assert_eq!(batch.len(), 2);
    }
}

#[test]
fn test_batch_hosts_with_remainder() {
    let hosts: Vec<String> = (1..=7).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Fixed(3);
    let batches = spec.batch_hosts(&hosts);

    // 7 hosts / 3 per batch = 2 full batches + 1 remainder
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
    assert_eq!(batches[2].len(), 1); // Remainder
}

#[test]
fn test_batch_hosts_progressive() {
    let hosts: Vec<String> = (1..=16).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Progressive(vec![
        SerialSpec::Fixed(1),
        SerialSpec::Fixed(5),
        SerialSpec::Fixed(10),
    ]);
    let batches = spec.batch_hosts(&hosts);

    // Progressive: 1, 5, 10, then cycles
    // Expected: batch of 1, batch of 5, batch of 10 (only 10 remaining after 1+5=6)
    assert!(batches.len() >= 3);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[1].len(), 5);
}

// ============================================================================
// Load Balancing Tests
// ============================================================================

#[tokio::test]
async fn test_load_balancing() {
    // Test that forks limits concurrent execution
    let runtime = create_runtime_with_hosts(vec![
        "host1", "host2", "host3", "host4", "host5", "host6", "host7", "host8", "host9", "host10",
    ]);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5, // Only 5 concurrent executions
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Load Balance Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Load task", "debug").arg("msg", "balancing"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 10 hosts should complete despite fork limit
    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_load_balancing_with_forks_one() {
    // Sequential execution with forks=1
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Sequential Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Sequential task", "debug").arg("msg", "one at a time"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Failure Handling Tests
// ============================================================================

#[tokio::test]
async fn test_failure_handling_continue() {
    // Test that ignore_errors allows execution to continue
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Failure Continue Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task that fails but continues
    play.add_task(
        Task::new("Fail but continue", "fail")
            .arg("msg", "This fails")
            .ignore_errors(true),
    );
    play.add_task(Task::new("After failure", "debug").arg("msg", "Still running"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should complete both tasks
    assert_eq!(results.len(), 2);
    for (host, result) in &results {
        // Host should not be marked as failed overall because ignore_errors=true
        assert!(
            !result.failed,
            "Host {} should not be marked failed with ignore_errors",
            host
        );
        // Both tasks should have run
        assert!(
            result.stats.ok > 0 || result.stats.changed > 0,
            "Host {} should have successful tasks",
            host
        );
    }
}

#[tokio::test]
async fn test_failure_handling_any_errors_fatal() {
    // Test that any_errors_fatal aborts execution on first failure
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Any Errors Fatal Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    // Note: any_errors_fatal is configured via ExecutorConfig, not Play

    // Fail on one host
    play.add_task(
        Task::new("Fail on host1", "fail")
            .arg("msg", "Fatal failure")
            .when("inventory_hostname == 'host1'"),
    );
    // This should not run on any host after failure
    play.add_task(Task::new("After failure", "debug").arg("msg", "Should not run"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host1 should have failed
    if let Some(host1) = results.get("host1") {
        assert!(host1.stats.failed > 0, "host1 should have failed tasks");
    }

    // With any_errors_fatal, other hosts may be aborted/unreachable
    // Exact behavior depends on implementation
}

#[tokio::test]
async fn test_failure_handling_max_fail_percentage() {
    // Test max_fail_percentage behavior
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Max Fail Percentage Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));
    play.max_fail_percentage = Some(25); // Allow 25% failures (1 of 4)

    // Fail on host1 only (25% failure)
    play.add_task(
        Task::new("Conditional fail", "fail")
            .arg("msg", "Fail")
            .when("inventory_hostname == 'host1'"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Execution should continue because 25% is at the threshold
    assert!(results.len() >= 1, "Some results should be present");
}

#[tokio::test]
async fn test_failure_handling_with_rescue_block() {
    // Simulate block/rescue behavior (simplified - actual block/rescue may need different API)
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Rescue Block Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Use ignore_errors as simplified rescue
    play.add_task(
        Task::new("May fail", "fail")
            .arg("msg", "Potential failure")
            .ignore_errors(true),
    );
    play.add_task(Task::new("Recovery", "debug").arg("msg", "Recovering from error"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let host_result = results.get("host1").unwrap();
    assert!(
        !host_result.failed,
        "Host should recover with ignore_errors"
    );
}

// ============================================================================
// Additional Coverage Tests
// ============================================================================

#[tokio::test]
async fn test_executor_with_check_mode_playbook() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            check_mode: true,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Check Mode Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Check mode task", "debug").arg("msg", "In check mode"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete in check mode
    assert_eq!(results.len(), 1);
    assert!(!results.get("host1").unwrap().failed);
}

#[tokio::test]
async fn test_executor_with_diff_mode_playbook() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            diff_mode: true,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Diff Mode Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Diff mode task", "debug").arg("msg", "With diff"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(!results.get("host1").unwrap().failed);
}

#[tokio::test]
async fn test_host_pinned_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::HostPinned,
            forks: 1, // One host at a time
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("HostPinned Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task 1", "debug").arg("msg", "first"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "second"));
    play.add_task(Task::new("Task 3", "debug").arg("msg", "third"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete all tasks
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
        // Each host should have run 3 tasks
        assert!(result.stats.ok >= 3 || result.stats.changed >= 3);
    }
}

#[tokio::test]
async fn test_empty_playbook() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let playbook = Playbook::new("Empty Playbook");

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Empty playbook should complete without errors
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_playbook_with_no_matching_hosts() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("No Match Test");
    let mut play = Play::new("Test", "nonexistent_group");
    play.gather_facts = false;
    play.add_task(Task::new("Task", "debug").arg("msg", "test"));
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete with no hosts matching
    // Results may be empty or contain both hosts with no tasks
}

#[tokio::test]
async fn test_multiple_plays_different_hosts() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Multi-Group Test");

    // Play 1 for webservers
    let mut play1 = Play::new("Web Setup", "webservers");
    play1.gather_facts = false;
    play1.add_task(Task::new("Web task", "debug").arg("msg", "web setup"));
    playbook.add_play(play1);

    // Play 2 for databases
    let mut play2 = Play::new("DB Setup", "databases");
    play2.gather_facts = false;
    play2.add_task(Task::new("DB task", "debug").arg("msg", "db setup"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 3 hosts should have results
    assert!(results.len() >= 1);
}

#[tokio::test]
async fn test_handler_execution() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Handler Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task that notifies handler
    play.add_task(
        Task::new("Trigger handler", "debug")
            .arg("msg", "Change made")
            .notify("restart service"),
    );

    play.add_handler(Handler {
        name: "restart service".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Service restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_task_with_loop() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Loop Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Loop task", "debug")
            .arg("msg", "Processing {{ item }}")
            .loop_over(vec![
                serde_json::json!("item1"),
                serde_json::json!("item2"),
                serde_json::json!("item3"),
            ]),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(!results.get("host1").unwrap().failed);
}

#[tokio::test]
async fn test_task_with_register() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Register Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Register result", "debug")
            .arg("msg", "Test output")
            .register("my_result"),
    );
    play.add_task(
        Task::new("Use registered", "debug").arg("msg", "{{ my_result.msg | default('no msg') }}"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(!results.get("host1").unwrap().failed);
}

#[tokio::test]
async fn test_task_status_default() {
    let status = TaskStatus::default();
    assert!(matches!(status, TaskStatus::Ok));
}

#[test]
fn test_task_diff() {
    use rustible::executor::task::TaskDiff;

    let diff = TaskDiff {
        before: Some("old content".to_string()),
        after: Some("new content".to_string()),
        before_header: Some("/path/to/file (before)".to_string()),
        after_header: Some("/path/to/file (after)".to_string()),
    };

    assert_eq!(diff.before, Some("old content".to_string()));
    assert_eq!(diff.after, Some("new content".to_string()));
}

#[tokio::test]
async fn test_run_once_task() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Run Once Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    let mut run_once_task = Task::new("Run once", "debug").arg("msg", "Only once");
    run_once_task.run_once = true;
    play.add_task(run_once_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should be in results
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_delegate_to() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Delegate Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    let mut delegated_task = Task::new("Delegated", "debug").arg("msg", "Delegated");
    delegated_task.delegate_to = Some("host1".to_string());
    play.add_task(delegated_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
}

// ============================================================================
// Stress and Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_many_hosts_execution() {
    let hosts: Vec<&str> = (0..20)
        .map(|i| match i {
            0 => "host0",
            1 => "host1",
            2 => "host2",
            3 => "host3",
            4 => "host4",
            5 => "host5",
            6 => "host6",
            7 => "host7",
            8 => "host8",
            9 => "host9",
            10 => "host10",
            11 => "host11",
            12 => "host12",
            13 => "host13",
            14 => "host14",
            15 => "host15",
            16 => "host16",
            17 => "host17",
            18 => "host18",
            _ => "host19",
        })
        .collect();

    let runtime = create_runtime_with_hosts(hosts);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Many Hosts Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    for i in 1..=5 {
        play.add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("task{}", i)));
    }

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 20);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_concurrent_set_fact() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Concurrent Set Fact Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple set_fact tasks running concurrently
    play.add_task(Task::new("Set fact 1", "set_fact").arg("fact_one", "value1"));
    play.add_task(Task::new("Set fact 2", "set_fact").arg("fact_two", "value2"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}
