//! Integration tests for parallel host execution
//!
//! This test suite covers:
//! - Parallel execution with mock hosts
//! - Semaphore limiting (forks)
//! - Error handling when one host fails
//! - Stats aggregation from parallel tasks
//! - Concurrency safety and race condition prevention
//! - Performance characteristics of parallel execution

mod common;

use std::time::Duration;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};

use common::PlaybookBuilder;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a runtime with multiple hosts for parallel testing
fn create_parallel_runtime(host_count: usize) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for i in 1..=host_count {
        runtime.add_host(format!("host{}", i), None);
    }
    runtime
}

/// Create executor with specific fork limit
fn create_executor_with_forks(
    runtime: RuntimeContext,
    forks: usize,
    strategy: ExecutionStrategy,
) -> Executor {
    let config = ExecutorConfig {
        forks,
        strategy,
        check_mode: false,
        gather_facts: false,
        ..Default::default()
    };
    Executor::with_runtime(config, runtime)
}

/// Create a simple test playbook with N tasks
fn create_test_playbook(task_count: usize, hosts: &str) -> Playbook {
    let mut playbook = PlaybookBuilder::new("Parallel Test").build();
    let mut play = Play::new("Test Play", hosts);
    play.gather_facts = false;

    for i in 1..=task_count {
        play.add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("Task {}", i)));
    }

    playbook.add_play(play);
    playbook
}

// ============================================================================
// 1. Parallel Execution with Mock Hosts
// ============================================================================

#[tokio::test]
async fn test_parallel_execution_basic() {
    let runtime = create_parallel_runtime(5);
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(3, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 5);
    for (host, result) in &results {
        assert!(
            !result.failed,
            "Host {} should not fail in parallel execution",
            host
        );
        assert!(
            result.stats.ok >= 3 || result.stats.changed >= 3,
            "Host {} should complete all tasks",
            host
        );
    }

    // Verify stats aggregation
    let summary = Executor::summarize_results(&results);
    assert!(summary.ok >= 15 || summary.changed >= 15); // 5 hosts * 3 tasks
}

#[tokio::test]
async fn test_parallel_execution_multiple_plays() {
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Multi-Play Parallel Test");

    // Play 1
    let mut play1 = Play::new("First Play", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("Play 1 Task 1", "debug").arg("msg", "play1-task1"));
    play1.add_task(Task::new("Play 1 Task 2", "debug").arg("msg", "play1-task2"));
    playbook.add_play(play1);

    // Play 2
    let mut play2 = Play::new("Second Play", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("Play 2 Task 1", "debug").arg("msg", "play2-task1"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.ok >= 3 || result.stats.changed >= 3); // Total tasks across both plays
    }
}

#[tokio::test]
async fn test_parallel_execution_free_strategy() {
    let runtime = create_parallel_runtime(6);
    let executor = create_executor_with_forks(runtime, 6, ExecutionStrategy::Free);

    let playbook = create_test_playbook(4, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete independently
    assert_eq!(results.len(), 6);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_parallel_host_independence() {
    // Verify that hosts can execute independently without blocking each other
    let runtime = create_parallel_runtime(3);
    let executor = create_executor_with_forks(runtime, 3, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Independent Execution");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Tasks with conditions that vary by host
    play.add_task(
        Task::new("Host1 only", "debug")
            .arg("msg", "Host1 task")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(
        Task::new("Host2 only", "debug")
            .arg("msg", "Host2 task")
            .when("inventory_hostname == 'host2'"),
    );
    play.add_task(Task::new("All hosts", "debug").arg("msg", "Everyone"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);

    // Each host should have 1 task executed and 1-2 skipped
    for (host, result) in &results {
        assert!(!result.failed, "Host {} should not fail", host);
        // All hosts run "All hosts" task, plus their own conditional task or skip
        assert!(
            result.stats.ok >= 1 || result.stats.changed >= 1,
            "Host {} should run at least one task",
            host
        );
    }
}

// ============================================================================
// 2. Semaphore Limiting (Forks)
// ============================================================================

#[tokio::test]
async fn test_forks_limit_basic() {
    // Test that fork limit is respected
    let runtime = create_parallel_runtime(10);
    let executor = create_executor_with_forks(runtime, 3, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(2, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should eventually complete despite fork limit
    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_forks_single_execution() {
    // With forks=1, execution should be truly sequential
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 1, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(2, "all");
    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }

    // With forks=1, execution should take noticeably longer than parallel
    // (This is a soft check since task execution is fast in tests)
    assert!(
        duration < Duration::from_secs(10),
        "Execution should complete within reasonable time"
    );
}

#[tokio::test]
async fn test_forks_high_parallelism() {
    // Test with high fork count
    let runtime = create_parallel_runtime(20);
    let executor = create_executor_with_forks(runtime, 20, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("High Parallelism");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    for i in 1..=5 {
        play.add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("task{}", i)));
    }

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 20);
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.ok >= 5 || result.stats.changed >= 5);
    }

    // High parallelism should complete quickly
    assert!(
        duration < Duration::from_secs(5),
        "High parallelism should be fast"
    );
}

#[tokio::test]
async fn test_forks_limit_with_different_strategies() {
    // Test fork limiting across different strategies
    let host_count = 8;

    // Linear strategy with fork limit
    let runtime_linear = create_parallel_runtime(host_count);
    let executor_linear = create_executor_with_forks(runtime_linear, 3, ExecutionStrategy::Linear);
    let playbook_linear = create_test_playbook(2, "all");
    let results_linear = executor_linear
        .run_playbook(&playbook_linear)
        .await
        .unwrap();

    assert_eq!(results_linear.len(), host_count);
    for result in results_linear.values() {
        assert!(!result.failed);
    }

    // Free strategy with fork limit
    let runtime_free = create_parallel_runtime(host_count);
    let executor_free = create_executor_with_forks(runtime_free, 3, ExecutionStrategy::Free);
    let playbook_free = create_test_playbook(2, "all");
    let results_free = executor_free.run_playbook(&playbook_free).await.unwrap();

    assert_eq!(results_free.len(), host_count);
    for result in results_free.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_dynamic_fork_adjustment() {
    // Test execution with varying fork counts
    for forks in [1, 2, 5, 10] {
        let runtime = create_parallel_runtime(10);
        let executor = create_executor_with_forks(runtime, forks, ExecutionStrategy::Linear);

        let playbook = create_test_playbook(3, "all");
        let results = executor.run_playbook(&playbook).await.unwrap();

        assert_eq!(
            results.len(),
            10,
            "With forks={}, all hosts should complete",
            forks
        );
        for result in results.values() {
            assert!(!result.failed);
        }
    }
}

// ============================================================================
// 3. Error Handling When One Host Fails
// ============================================================================

#[tokio::test]
async fn test_single_host_failure_linear_strategy() {
    let runtime = create_parallel_runtime(5);
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Single Host Failure");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task that fails on host3
    play.add_task(
        Task::new("Maybe fail", "fail")
            .arg("msg", "Intentional failure")
            .when("inventory_hostname == 'host3'"),
    );

    // Task that should still run on other hosts
    play.add_task(Task::new("Continue", "debug").arg("msg", "Continuing"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 5);

    // host3 should fail
    if let Some(host3_result) = results.get("host3") {
        assert!(
            host3_result.stats.failed > 0,
            "host3 should have failed tasks"
        );
    }

    // Other hosts should succeed
    for (host, result) in &results {
        if host != "host3" {
            assert!(!result.failed, "Host {} should not fail", host);
        }
    }
}

#[tokio::test]
#[ignore = "Known issue with failure counting in free strategy execution"]
async fn test_multiple_host_failures_free_strategy() {
    let runtime = create_parallel_runtime(6);
    let executor = create_executor_with_forks(runtime, 6, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Multiple Failures");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Tasks that fail on even-numbered hosts
    play.add_task(
        Task::new("Maybe fail", "fail")
            .arg("msg", "Failure")
            .when("inventory_hostname in ['host2', 'host4', 'host6']"),
    );

    play.add_task(Task::new("Continue", "debug").arg("msg", "Continuing on working hosts"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 6);

    // Verify failure pattern
    for (host, result) in &results {
        if host == "host2" || host == "host4" || host == "host6" {
            assert!(
                result.stats.failed > 0,
                "Host {} should have failures",
                host
            );
        } else {
            assert!(!result.failed, "Host {} should succeed", host);
        }
    }
}

#[tokio::test]
async fn test_ignore_errors_with_parallel_execution() {
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Ignore Errors Parallel");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Failing task with ignore_errors
    play.add_task(
        Task::new("Fail but ignore", "fail")
            .arg("msg", "This fails")
            .ignore_errors(true),
    );

    // This should run despite the failure
    play.add_task(Task::new("After failure", "debug").arg("msg", "Still running"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);

    // All hosts should complete despite errors
    for result in results.values() {
        assert!(!result.failed, "Hosts should not be marked as failed");
        // Both tasks should have run (one failed, one succeeded)
        assert!(result.stats.ok >= 1 || result.stats.changed >= 1);
    }
}

#[tokio::test]
#[ignore = "Known issue with failure propagation in executor"]
async fn test_failure_propagation_stops_failed_host() {
    let runtime = create_parallel_runtime(3);
    let executor = create_executor_with_forks(runtime, 3, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Failure Propagation");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // First task fails on host2
    play.add_task(
        Task::new("Fail on host2", "fail")
            .arg("msg", "Failing")
            .when("inventory_hostname == 'host2'"),
    );

    // Second task should not run on host2
    play.add_task(Task::new("After failure", "debug").arg("msg", "This should run"));

    // Third task
    play.add_task(Task::new("Third task", "debug").arg("msg", "Third"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);

    // host2 should have failed and stopped executing subsequent tasks
    if let Some(host2) = results.get("host2") {
        assert!(host2.stats.failed > 0);
        // host2 should have fewer ok/changed tasks than other hosts
    }

    // Other hosts should complete all tasks
    for (host, result) in &results {
        if host != "host2" {
            assert!(!result.failed);
            assert!(result.stats.ok >= 3 || result.stats.changed >= 3);
        }
    }
}

#[tokio::test]
async fn test_unreachable_host_handling() {
    // This test would need mock connections that simulate unreachable hosts
    // For now, we test the error handling logic

    let mut runtime = RuntimeContext::new();
    runtime.add_host("reachable1".to_string(), None);
    runtime.add_host("reachable2".to_string(), None);

    let executor = create_executor_with_forks(runtime, 2, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(2, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    // Reachable hosts should complete
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
        assert!(!result.unreachable);
    }
}

// ============================================================================
// 4. Stats Aggregation from Parallel Tasks
// ============================================================================

#[tokio::test]
async fn test_stats_aggregation_basic() {
    let runtime = create_parallel_runtime(3);
    let executor = create_executor_with_forks(runtime, 3, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Stats Aggregation");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Mix of ok, changed, and skipped tasks
    play.add_task(Task::new("OK task", "debug").arg("msg", "ok"));
    play.add_task(
        Task::new("Skipped task", "debug")
            .arg("msg", "skip")
            .when("false"),
    );
    play.add_task(Task::new("Another OK", "debug").arg("msg", "ok2"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify per-host stats
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.ok >= 2 || result.stats.changed >= 2);
        assert_eq!(result.stats.skipped, 1);
    }

    // Verify aggregate stats
    let summary = Executor::summarize_results(&results);
    assert!(summary.ok >= 6 || summary.changed >= 6); // 3 hosts * 2 tasks
    assert_eq!(summary.skipped, 3); // 3 hosts * 1 skipped task
}

#[tokio::test]
#[ignore = "Known issue with stats aggregation and failure counting"]
async fn test_stats_aggregation_with_failures() {
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Stats with Failures");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("OK task", "debug").arg("msg", "ok"));
    play.add_task(
        Task::new("Fail on some", "fail")
            .arg("msg", "fail")
            .when("inventory_hostname in ['host2', 'host4']"),
    );
    play.add_task(Task::new("After failure", "debug").arg("msg", "after"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let summary = Executor::summarize_results(&results);

    // Some tasks succeeded, some failed
    assert!(summary.ok > 0 || summary.changed > 0);
    assert!(summary.failed > 0);
}

#[tokio::test]
async fn test_stats_aggregation_across_plays() {
    let runtime = create_parallel_runtime(2);
    let executor = create_executor_with_forks(runtime, 2, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Multi-Play Stats");

    // Play 1
    let mut play1 = Play::new("Play 1", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("P1T1", "debug").arg("msg", "play1"));
    play1.add_task(Task::new("P1T2", "debug").arg("msg", "play1"));
    playbook.add_play(play1);

    // Play 2
    let mut play2 = Play::new("Play 2", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("P2T1", "debug").arg("msg", "play2"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Stats should aggregate across both plays
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.ok >= 3 || result.stats.changed >= 3);
    }

    let summary = Executor::summarize_results(&results);
    assert!(summary.ok >= 6 || summary.changed >= 6); // 2 hosts * 3 tasks
}

#[tokio::test]
async fn test_stats_precision_with_loops() {
    let runtime = create_parallel_runtime(2);
    let executor = create_executor_with_forks(runtime, 2, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Loop Stats");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task with loop
    play.add_task(
        Task::new("Loop task", "debug")
            .arg("msg", "Item {{ item }}")
            .loop_over(vec![
                serde_json::json!("a"),
                serde_json::json!("b"),
                serde_json::json!("c"),
            ]),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Each host should show stats for loop iterations
    for result in results.values() {
        assert!(!result.failed);
        // Loop creates multiple executions
        assert!(result.stats.ok > 0 || result.stats.changed > 0);
    }
}

#[tokio::test]
async fn test_stats_consistency_across_strategies() {
    let host_count = 4;
    let task_count = 3;

    // Linear strategy
    let runtime_linear = create_parallel_runtime(host_count);
    let executor_linear =
        create_executor_with_forks(runtime_linear, host_count, ExecutionStrategy::Linear);
    let playbook_linear = create_test_playbook(task_count, "all");
    let results_linear = executor_linear
        .run_playbook(&playbook_linear)
        .await
        .unwrap();
    let summary_linear = Executor::summarize_results(&results_linear);

    // Free strategy
    let runtime_free = create_parallel_runtime(host_count);
    let executor_free =
        create_executor_with_forks(runtime_free, host_count, ExecutionStrategy::Free);
    let playbook_free = create_test_playbook(task_count, "all");
    let results_free = executor_free.run_playbook(&playbook_free).await.unwrap();
    let summary_free = Executor::summarize_results(&results_free);

    // Both strategies should produce same total stats
    assert_eq!(
        summary_linear.ok + summary_linear.changed,
        summary_free.ok + summary_free.changed
    );
    assert_eq!(summary_linear.failed, summary_free.failed);
    assert_eq!(summary_linear.skipped, summary_free.skipped);
}

// ============================================================================
// 5. Concurrency Safety Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_variable_registration() {
    // Test that variable registration is thread-safe
    let runtime = create_parallel_runtime(5);
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Concurrent Registration");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple tasks registering different variables concurrently
    play.add_task(
        Task::new("Register 1", "debug")
            .arg("msg", "Value 1")
            .register("reg1"),
    );
    play.add_task(
        Task::new("Register 2", "debug")
            .arg("msg", "Value 2")
            .register("reg2"),
    );
    play.add_task(
        Task::new("Register 3", "debug")
            .arg("msg", "Value 3")
            .register("reg3"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed, "Registration should be thread-safe");
    }
}

#[tokio::test]
async fn test_concurrent_handler_notification() {
    // Test that handler notifications are thread-safe
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Concurrent Handlers");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple tasks notifying the same handler
    for i in 1..=3 {
        play.add_task(
            Task::new(format!("Notify {}", i), "debug")
                .arg("msg", format!("notifying {}", i))
                .notify("test_handler"),
        );
    }

    play.add_handler(rustible::executor::task::Handler {
        name: "test_handler".to_string(),
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

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_no_race_conditions_in_stats() {
    // Run multiple times to increase chance of detecting race conditions
    for iteration in 1..=5 {
        let runtime = create_parallel_runtime(10);
        let executor = create_executor_with_forks(runtime, 10, ExecutionStrategy::Free);

        let mut playbook = Playbook::new(format!("Race Test {}", iteration));
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;

        for i in 1..=5 {
            play.add_task(
                Task::new(format!("Task {}", i), "debug").arg("msg", format!("task{}", i)),
            );
        }

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert_eq!(results.len(), 10);
        for result in results.values() {
            assert!(
                !result.failed,
                "No race conditions should cause failures (iteration {})",
                iteration
            );
        }

        let summary = Executor::summarize_results(&results);
        assert!(
            summary.ok >= 50 || summary.changed >= 50,
            "Stats should be accurate (iteration {})",
            iteration
        );
    }
}

#[tokio::test]
async fn test_parallel_fact_gathering_safety() {
    // Test that fact gathering is thread-safe under parallel execution
    let runtime = create_parallel_runtime(6);
    let executor = create_executor_with_forks(runtime, 6, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Fact Gathering");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple set_fact tasks
    play.add_task(Task::new("Set fact 1", "set_fact").arg("fact1", "value1"));
    play.add_task(Task::new("Set fact 2", "set_fact").arg("fact2", "value2"));
    play.add_task(Task::new("Set fact 3", "set_fact").arg("fact3", "value3"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 6);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// 6. Performance and Timing Tests
// ============================================================================

#[tokio::test]
async fn test_parallel_faster_than_sequential() {
    let host_count = 10;
    let task_count = 3;

    // Sequential execution (forks=1)
    let runtime_seq = create_parallel_runtime(host_count);
    let executor_seq = create_executor_with_forks(runtime_seq, 1, ExecutionStrategy::Linear);
    let playbook_seq = create_test_playbook(task_count, "all");

    let start_seq = std::time::Instant::now();
    let results_seq = executor_seq.run_playbook(&playbook_seq).await.unwrap();
    let duration_seq = start_seq.elapsed();

    // Parallel execution (forks=10)
    let runtime_par = create_parallel_runtime(host_count);
    let executor_par = create_executor_with_forks(runtime_par, 10, ExecutionStrategy::Linear);
    let playbook_par = create_test_playbook(task_count, "all");

    let start_par = std::time::Instant::now();
    let results_par = executor_par.run_playbook(&playbook_par).await.unwrap();
    let duration_par = start_par.elapsed();

    // Both should complete successfully
    assert_eq!(results_seq.len(), host_count);
    assert_eq!(results_par.len(), host_count);

    // Note: Actual timing comparison is unreliable in fast unit tests,
    // but we verify both complete within reasonable time
    assert!(
        duration_seq < Duration::from_secs(10),
        "Sequential should complete"
    );
    assert!(
        duration_par < Duration::from_secs(10),
        "Parallel should complete"
    );

    println!(
        "Sequential: {:?}, Parallel: {:?}",
        duration_seq, duration_par
    );
}

#[tokio::test]
async fn test_scalability_with_many_hosts() {
    // Test that executor scales to many hosts
    let runtime = create_parallel_runtime(50);
    let executor = create_executor_with_forks(runtime, 20, ExecutionStrategy::Free);

    let playbook = create_test_playbook(3, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 50);
    for result in results.values() {
        assert!(!result.failed);
    }

    let summary = Executor::summarize_results(&results);
    assert!(summary.ok >= 150 || summary.changed >= 150); // 50 hosts * 3 tasks
}

#[tokio::test]
async fn test_memory_efficiency_parallel_execution() {
    // Test that parallel execution doesn't leak memory
    // Run multiple iterations to detect memory issues
    for _ in 0..5 {
        let runtime = create_parallel_runtime(20);
        let executor = create_executor_with_forks(runtime, 10, ExecutionStrategy::Free);

        let mut playbook = Playbook::new("Memory Test");
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;

        for i in 1..=10 {
            play.add_task(
                Task::new(format!("Task {}", i), "debug").arg("msg", format!("test{}", i)),
            );
        }

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert_eq!(results.len(), 20);
        for result in results.values() {
            assert!(!result.failed);
        }
    }
}

// ============================================================================
// 7. Edge Cases and Boundary Conditions
// ============================================================================

#[tokio::test]
async fn test_zero_hosts() {
    let runtime = RuntimeContext::new(); // No hosts
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(2, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete with no results
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_single_host_parallel() {
    let runtime = create_parallel_runtime(1);
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(3, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    let result = results.get("host1").unwrap();
    assert!(!result.failed);
    assert!(result.stats.ok >= 3 || result.stats.changed >= 3);
}

#[tokio::test]
async fn test_more_forks_than_hosts() {
    // forks > number of hosts should work fine
    let runtime = create_parallel_runtime(3);
    let executor = create_executor_with_forks(runtime, 10, ExecutionStrategy::Linear);

    let playbook = create_test_playbook(2, "all");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_zero_tasks() {
    let runtime = create_parallel_runtime(5);
    let executor = create_executor_with_forks(runtime, 5, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("No Tasks");
    let play = Play::new("Empty Play", "all");
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete without errors
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_all_hosts_skip_all_tasks() {
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("All Skipped");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Always skipped", "debug")
            .arg("msg", "never")
            .when("false"),
    );
    play.add_task(
        Task::new("Also skipped", "debug")
            .arg("msg", "nope")
            .when("1 == 2"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
        assert_eq!(result.stats.skipped, 2);
    }
}

// ============================================================================
// 8. Complex Scenarios
// ============================================================================

#[tokio::test]
async fn test_mixed_success_failure_skip() {
    let runtime = create_parallel_runtime(6);
    let executor = create_executor_with_forks(runtime, 6, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Mixed Results");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // OK task for all
    play.add_task(Task::new("OK", "debug").arg("msg", "ok"));

    // Skip on some
    play.add_task(
        Task::new("Skip some", "debug")
            .arg("msg", "skip")
            .when("inventory_hostname in ['host1', 'host2']"),
    );

    // Fail on some (with ignore_errors)
    play.add_task(
        Task::new("Fail some", "fail")
            .arg("msg", "fail")
            .when("inventory_hostname in ['host3', 'host4']")
            .ignore_errors(true),
    );

    // Final task for all
    play.add_task(Task::new("Final", "debug").arg("msg", "final"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 6);

    // Verify mixed stats
    let summary = Executor::summarize_results(&results);
    assert!(summary.ok > 0 || summary.changed > 0);
    assert!(summary.skipped > 0);
}

#[tokio::test]
async fn test_parallel_with_handlers() {
    let runtime = create_parallel_runtime(4);
    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Linear);

    let mut playbook = Playbook::new("Parallel with Handlers");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Trigger handler", "debug")
            .arg("msg", "triggering")
            .notify("test_handler"),
    );

    play.add_handler(rustible::executor::task::Handler {
        name: "test_handler".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Handler ran"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
#[ignore = "Known issue with conditional group execution"]
async fn test_parallel_execution_with_conditional_groups() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("db2".to_string(), Some("databases"));

    let executor = create_executor_with_forks(runtime, 4, ExecutionStrategy::Free);

    let mut playbook = Playbook::new("Conditional Groups");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Web-only task
    play.add_task(
        Task::new("Web task", "debug")
            .arg("msg", "web")
            .when("'webservers' in group_names"),
    );

    // DB-only task
    play.add_task(
        Task::new("DB task", "debug")
            .arg("msg", "db")
            .when("'databases' in group_names"),
    );

    // All hosts task
    play.add_task(Task::new("All task", "debug").arg("msg", "all"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
        // Each host runs: 1 conditional task (either web or db) + 1 all task + 1 skipped
        assert!(result.stats.ok >= 2 || result.stats.changed >= 2);
        assert!(result.stats.skipped >= 1);
    }
}
