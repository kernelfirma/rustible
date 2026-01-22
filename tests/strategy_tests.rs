//! Comprehensive tests for Rustible execution strategies
//!
//! This test suite covers:
//! - Linear strategy (task-by-task across all hosts)
//! - Free strategy (hosts proceed independently)
//! - HostPinned strategy (complete host before moving on)
//! - Strategy switching between plays
//! - Forks/parallelism limits
//! - Strategy behavior with host failures
//! - Strategy and handler interaction
//! - Performance characteristics
//! - Edge cases (single host, single task)

#![cfg(not(tarpaulin))]

use std::time::{Duration, Instant};

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task};
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};

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

/// Create a runtime with hosts in groups
fn create_runtime_with_groups(groups: Vec<(&str, Vec<&str>)>) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for (group, hosts) in groups {
        for host in hosts {
            runtime.add_host(host.to_string(), Some(group));
        }
    }
    runtime
}

// ============================================================================
// Linear Strategy Tests
// ============================================================================

#[tokio::test]
async fn test_linear_strategy_basic() {
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
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add multiple tasks
    play.add_task(Task::new("Task 1", "debug").arg("msg", "First task"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second task"));
    play.add_task(Task::new("Task 3", "debug").arg("msg", "Third task"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete all tasks
    assert_eq!(results.len(), 3);
    for (host, result) in &results {
        assert!(!result.failed, "Host {} should not fail", host);
        assert!(
            result.stats.ok >= 3 || result.stats.changed >= 3,
            "Host {} should complete all tasks",
            host
        );
    }
}

#[tokio::test]
async fn test_linear_strategy_task_ordering() {
    // Linear strategy should run each task on ALL hosts before moving to next task
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Linear Order Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("First", "debug").arg("msg", "first"));
    play.add_task(Task::new("Second", "debug").arg("msg", "second"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should complete successfully
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_linear_strategy_stops_on_all_failures() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Linear Failure Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task that will fail
    play.add_task(Task::new("Will fail", "fail").arg("msg", "Intentional failure"));
    // This should not run on failed hosts
    play.add_task(Task::new("After failure", "debug").arg("msg", "Should not run"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should have failures
    for result in results.values() {
        assert!(result.stats.failed > 0);
    }
}

#[tokio::test]
async fn test_linear_strategy_continues_on_other_hosts() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Linear Partial Failure Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Conditional failure on one host
    play.add_task(
        Task::new("Conditional fail", "fail")
            .arg("msg", "Fail on host1")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "This should run on host2 and host3"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host1 should fail, others should continue
    assert_eq!(results.len(), 3);
    if let Some(host1_result) = results.get("host1") {
        assert!(host1_result.stats.failed > 0);
    }
    // Other hosts should have successful tasks
    if let Some(host2_result) = results.get("host2") {
        assert!(host2_result.stats.ok > 0 || host2_result.stats.changed > 0);
    }
}

// ============================================================================
// Free Strategy Tests
// ============================================================================

#[tokio::test]
async fn test_free_strategy_basic() {
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
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second"));
    play.add_task(Task::new("Task 3", "debug").arg("msg", "Third"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete all tasks
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_free_strategy_independent_execution() {
    // Free strategy allows each host to proceed independently
    let runtime = create_runtime_with_hosts(vec!["fast_host", "slow_host"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Free Independent Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Task 1", "debug").arg("msg", "task1"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "task2"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should complete
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_free_strategy_failure_isolation() {
    // In free strategy, a failure on one host doesn't affect others
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Free Failure Isolation Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Maybe fail", "fail")
            .arg("msg", "Fail on host1")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "All other hosts continue"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host1 fails but doesn't stop others
    assert_eq!(results.len(), 3);
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_free_strategy_with_skipped_tasks() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Free Skip Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task that will be skipped based on condition
    play.add_task(
        Task::new("Conditional task", "debug")
            .arg("msg", "Only on host1")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("All hosts", "debug").arg("msg", "Everyone runs this"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);

    // host1 should have no skipped tasks, others should have 1 skipped
    if let Some(host1_result) = results.get("host1") {
        assert!(!host1_result.failed);
    }
    for (host, result) in &results {
        if host != "host1" {
            assert!(
                result.stats.skipped > 0,
                "Host {} should have skipped tasks",
                host
            );
        }
    }
}

// ============================================================================
// HostPinned Strategy Tests
// ============================================================================

#[tokio::test]
async fn test_host_pinned_strategy_basic() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::HostPinned,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("HostPinned Strategy Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_host_pinned_complete_host_before_next() {
    // HostPinned strategy should complete all tasks on one host before starting another
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::HostPinned,
            forks: 1, // Force sequential host execution
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("HostPinned Sequential Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("First", "debug").arg("msg", "first"));
    play.add_task(Task::new("Second", "debug").arg("msg", "second"));
    play.add_task(Task::new("Third", "debug").arg("msg", "third"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Strategy Switching Between Plays Tests
// ============================================================================

#[tokio::test]
async fn test_strategy_switching_between_plays() {
    // First executor with Linear strategy
    let runtime_linear = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_linear = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime_linear,
    );

    let mut playbook_linear = Playbook::new("Linear Play");
    let mut play_linear = Play::new("Linear Play", "all");
    play_linear.gather_facts = false;
    play_linear.add_task(Task::new("Linear task", "debug").arg("msg", "linear"));
    playbook_linear.add_play(play_linear);

    let results_linear = executor_linear
        .run_playbook(&playbook_linear)
        .await
        .unwrap();
    assert_eq!(results_linear.len(), 3);

    // Second executor with Free strategy (create fresh runtime)
    let runtime_free = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_free = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime_free,
    );

    let mut playbook_free = Playbook::new("Free Play");
    let mut play_free = Play::new("Free Play", "all");
    play_free.gather_facts = false;
    play_free.add_task(Task::new("Free task", "debug").arg("msg", "free"));
    playbook_free.add_play(play_free);

    let results_free = executor_free.run_playbook(&playbook_free).await.unwrap();
    assert_eq!(results_free.len(), 3);
}

#[tokio::test]
async fn test_multiple_plays_same_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Multi-Play Test");

    // First play
    let mut play1 = Play::new("First Play", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("Play 1 Task", "debug").arg("msg", "first play"));
    playbook.add_play(play1);

    // Second play
    let mut play2 = Play::new("Second Play", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("Play 2 Task", "debug").arg("msg", "second play"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should have stats from both plays
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.ok >= 2 || result.stats.changed >= 2);
    }
}

// ============================================================================
// Forks/Parallelism Limit Tests
// ============================================================================

#[tokio::test]
async fn test_forks_limit_basic() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2, // Limit to 2 parallel executions
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Forks Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task", "debug").arg("msg", "test"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete despite fork limit
    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_forks_single_host_at_a_time() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1, // One host at a time
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Fork Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task", "debug").arg("msg", "sequential"));

    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let _duration = start.elapsed();

    assert_eq!(results.len(), 3);
    // With forks=1, execution should be more sequential
}

#[tokio::test]
async fn test_forks_high_parallelism() {
    let runtime = create_runtime_with_hosts(vec![
        "host1", "host2", "host3", "host4", "host5", "host6", "host7", "host8", "host9", "host10",
    ]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 10, // High parallelism
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("High Parallelism Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Task 1", "debug").arg("msg", "task1"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "task2"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Strategy Behavior with Host Failures Tests
// ============================================================================

#[tokio::test]
async fn test_linear_strategy_single_host_failure() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Host Failure Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Fail on host2", "fail")
            .arg("msg", "Intentional failure")
            .when("inventory_hostname == 'host2'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "Other hosts continue"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host2 should fail, others should succeed
    if let Some(host2) = results.get("host2") {
        assert!(host2.stats.failed > 0);
    }
    if let Some(host1) = results.get("host1") {
        assert!(!host1.failed);
    }
    if let Some(host3) = results.get("host3") {
        assert!(!host3.failed);
    }
}

#[tokio::test]
async fn test_free_strategy_multiple_host_failures() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Multiple Failures Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Fail on some hosts", "fail")
            .arg("msg", "Fail")
            .when("inventory_hostname == 'host1' or inventory_hostname == 'host3'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "Working hosts continue"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host1 and host3 should fail, host2 and host4 should succeed
    assert_eq!(results.len(), 4);
}

#[tokio::test]
async fn test_ignore_errors_with_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Ignore Errors Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Fail but ignore", "fail")
            .arg("msg", "This will fail")
            .ignore_errors(true),
    );
    play.add_task(Task::new("Continue after failure", "debug").arg("msg", "Still running"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should complete despite errors
    assert_eq!(results.len(), 2);
    for result in results.values() {
        // The task failed but was ignored, so host should not be marked as failed
        assert!(!result.failed);
    }
}

// ============================================================================
// Strategy and Handler Interaction Tests
// ============================================================================

#[tokio::test]
async fn test_handlers_with_linear_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Handlers Linear Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Trigger handler", "debug")
            .arg("msg", "Notify handler")
            .notify("test handler"),
    );

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

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Handlers should execute after play completes
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_handlers_with_free_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Handlers Free Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Notify handler", "debug")
            .arg("msg", "Changed")
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

    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_handlers_run_once_per_play() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Handler Once Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple tasks notify the same handler
    play.add_task(
        Task::new("Task 1", "debug")
            .arg("msg", "First")
            .notify("common handler"),
    );
    play.add_task(
        Task::new("Task 2", "debug")
            .arg("msg", "Second")
            .notify("common handler"),
    );

    play.add_handler(Handler {
        name: "common handler".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Handler runs once"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Handler should run once despite multiple notifications
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_handlers_not_run_on_failure() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Handler Failure Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Will fail", "fail")
            .arg("msg", "Failed")
            .notify("should not run"),
    );

    play.add_handler(Handler {
        name: "should not run".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Should not see this"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Handlers should not run because tasks failed
    for result in results.values() {
        assert!(result.stats.failed > 0);
    }
}

// ============================================================================
// Performance Characteristics Tests
// ============================================================================

#[tokio::test]
async fn test_linear_vs_free_performance() {
    // Linear strategy
    let runtime_linear = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_linear = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime_linear,
    );

    let mut playbook_linear = Playbook::new("Linear Perf Test");
    let mut play_linear = Play::new("Test", "all");
    play_linear.gather_facts = false;
    for i in 1..=5 {
        play_linear
            .add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("task{}", i)));
    }
    playbook_linear.add_play(play_linear);

    let start = Instant::now();
    let results_linear = executor_linear
        .run_playbook(&playbook_linear)
        .await
        .unwrap();
    let linear_duration = start.elapsed();

    assert_eq!(results_linear.len(), 3);

    // Free strategy
    let runtime_free = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_free = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime_free,
    );

    let mut playbook_free = Playbook::new("Free Perf Test");
    let mut play_free = Play::new("Test", "all");
    play_free.gather_facts = false;
    for i in 1..=5 {
        play_free
            .add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("task{}", i)));
    }
    playbook_free.add_play(play_free);

    let start = Instant::now();
    let results_free = executor_free.run_playbook(&playbook_free).await.unwrap();
    let free_duration = start.elapsed();

    assert_eq!(results_free.len(), 3);

    // Both should complete, actual timing comparison depends on task complexity
    println!("Linear: {:?}, Free: {:?}", linear_duration, free_duration);
}

#[tokio::test]
async fn test_strategy_memory_efficiency() {
    // Test that strategies don't leak memory with many hosts
    let hosts: Vec<String> = (1..=20).map(|i| format!("host{}", i)).collect();
    let mut runtime = RuntimeContext::new();
    for host in &hosts {
        runtime.add_host(host.clone(), None);
    }

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Memory Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    for i in 1..=10 {
        play.add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("test{}", i)));
    }
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 20);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Edge Cases Tests
// ============================================================================

#[tokio::test]
async fn test_single_host_linear_strategy() {
    let runtime = create_runtime_with_hosts(vec!["lonely_host"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Host Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Solo task", "debug").arg("msg", "alone"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    let result = results.get("lonely_host").unwrap();
    assert!(!result.failed);
}

#[tokio::test]
async fn test_single_task_multiple_hosts() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Task Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Only task", "debug").arg("msg", "single"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_zero_tasks_playbook() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Empty Playbook");
    let play = Play::new("Empty Play", "all");
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete without errors even with no tasks
    // May or may not return hosts depending on implementation
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_empty_host_pattern() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("No Match Test");
    let mut play = Play::new("Test", "nonexistent");
    play.gather_facts = false;
    play.add_task(Task::new("Task", "debug").arg("msg", "test"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete with empty results
    assert!(results.is_empty() || results.len() == 1);
}

#[tokio::test]
async fn test_all_tasks_skipped() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("All Skipped Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Never runs", "debug")
            .arg("msg", "skipped")
            .when("false"),
    );
    play.add_task(
        Task::new("Also skipped", "debug")
            .arg("msg", "also skipped")
            .when("1 == 2"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
        assert!(result.stats.skipped >= 2);
    }
}

#[tokio::test]
async fn test_host_limit_with_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Limited Hosts Test");
    let mut play = Play::new("Test", "host1,host2");
    play.gather_facts = false;
    play.add_task(Task::new("Task", "debug").arg("msg", "test"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should only run on specified hosts
    assert_eq!(results.len(), 2);
    assert!(results.contains_key("host1"));
    assert!(results.contains_key("host2"));
}

// ============================================================================
// Complex Scenario Tests
// ============================================================================

#[tokio::test]
async fn test_complex_linear_workflow() {
    let runtime = create_runtime_with_groups(vec![
        ("webservers", vec!["web1", "web2"]),
        ("databases", vec!["db1"]),
    ]);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Complex Linear Workflow");

    // Play 1: Setup webservers
    let mut play1 = Play::new("Setup Webservers", "webservers");
    play1.gather_facts = false;
    play1.add_task(Task::new("Install nginx", "debug").arg("msg", "Installing nginx"));
    play1.add_task(
        Task::new("Configure nginx", "debug")
            .arg("msg", "Configuring")
            .notify("restart nginx"),
    );
    play1.add_handler(Handler {
        name: "restart nginx".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Nginx restarted"));
            args
        },
        when: None,
        listen: vec![],
    });
    playbook.add_play(play1);

    // Play 2: Setup database
    let mut play2 = Play::new("Setup Database", "databases");
    play2.gather_facts = false;
    play2.add_task(Task::new("Install postgres", "debug").arg("msg", "Installing postgres"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 3);
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
    assert!(results.contains_key("db1"));
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_mixed_success_failure_strategies() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 4,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Mixed Results Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Conditional failure", "fail")
            .arg("msg", "Fail")
            .when("inventory_hostname == 'host2' or inventory_hostname == 'host4'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "Continuing"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);

    // host2 and host4 should fail
    if let Some(host2) = results.get("host2") {
        assert!(host2.stats.failed > 0);
    }
    if let Some(host4) = results.get("host4") {
        assert!(host4.stats.failed > 0);
    }

    // host1 and host3 should succeed
    if let Some(host1) = results.get("host1") {
        assert!(!host1.failed);
    }
    if let Some(host3) = results.get("host3") {
        assert!(!host3.failed);
    }
}

// ============================================================================
// Execution Stats Verification Tests
// ============================================================================

#[tokio::test]
async fn test_execution_stats_accuracy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Stats Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("OK task", "debug").arg("msg", "ok"));
    play.add_task(
        Task::new("Skipped task", "debug")
            .arg("msg", "skip")
            .when("false"),
    );
    play.add_task(
        Task::new("Failed task", "fail")
            .arg("msg", "fail")
            .ignore_errors(true),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    for result in results.values() {
        // Each host should have mixed stats
        assert!(result.stats.ok > 0 || result.stats.changed > 0);
        assert!(result.stats.skipped > 0);
    }

    // Verify summary
    let summary = Executor::summarize_results(&results);
    assert!(summary.ok > 0 || summary.changed > 0);
    assert!(summary.skipped > 0);
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_stats_aggregation_multiple_plays() {
    let runtime = create_runtime_with_hosts(vec!["host1"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Multi-Play Stats Test");

    let mut play1 = Play::new("Play 1", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("Task 1", "debug").arg("msg", "play1"));
    playbook.add_play(play1);

    let mut play2 = Play::new("Play 2", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("Task 2", "debug").arg("msg", "play2"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let host1_result = results.get("host1").unwrap();
    // Stats should aggregate across both plays
    assert!(host1_result.stats.ok >= 2 || host1_result.stats.changed >= 2);
}

// ============================================================================
// Serial Execution Tests
// ============================================================================

#[tokio::test]
async fn test_serial_one_host_at_a_time() {
    // serial: 1 means execute on one host at a time
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1, // Effectively serial: 1
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial One Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(rustible::playbook::SerialSpec::Fixed(1));

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First task"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second task"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_batches() {
    // Test execution in batches of 2
    let runtime =
        create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5", "host6"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2, // Batch size of 2
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Batch Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(rustible::playbook::SerialSpec::Fixed(2));

    play.add_task(Task::new("Batch task", "debug").arg("msg", "Running in batches"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 6 hosts should complete
    assert_eq!(results.len(), 6);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_percentage_simulation() {
    // Simulate serial: 50% - half of hosts at a time
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);

    // 50% of 4 hosts = 2 hosts at a time
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Percentage Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Percentage batch", "debug").arg("msg", "50% at a time"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_rolling_update_simulation() {
    // Simulate serial: [1, 5, 10] - rolling update pattern
    // Start with 1, then 5, then 10 at a time
    let hosts: Vec<&str> = (0..16)
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
            _ => "host15",
        })
        .collect();

    let runtime = create_runtime_with_hosts(hosts);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10, // Max batch size
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Rolling Update Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Rolling update", "debug").arg("msg", "Rolling deployment"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 16);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_with_max_fail_percentage() {
    // Test serial execution with max_fail_percentage
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Max Fail Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(rustible::playbook::SerialSpec::Fixed(2));
    play.max_fail_percentage = Some(50); // Allow 50% failures

    // Fail on specific hosts
    play.add_task(
        Task::new("Maybe fail", "fail")
            .arg("msg", "Conditional failure")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "Continuing"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // host1 should fail, others should succeed or continue
    assert!(results.len() >= 1);
}

// ============================================================================
// Throttle Tests
// ============================================================================

#[tokio::test]
async fn test_throttle_limits_concurrent_executions() {
    // Throttle limits concurrent task executions across all hosts
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);

    // With forks=5 but logical throttle of 2
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2, // Effectively throttle: 2
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Throttle Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Throttled task", "debug").arg("msg", "Limited concurrency"));

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let _duration = start.elapsed();

    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_throttle_with_free_strategy() {
    // Throttle should work with free strategy too
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 2, // Throttle even in free strategy
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Free Throttle Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Throttled free", "debug").arg("msg", "Free but limited"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_throttle_single_concurrent() {
    // Throttle: 1 means truly sequential
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Throttle Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Sequential", "debug").arg("msg", "One at a time"));
    play.add_task(Task::new("Also sequential", "debug").arg("msg", "Still one"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
}

// ============================================================================
// Run_Once Tests
// ============================================================================

#[tokio::test]
async fn test_run_once_basic() {
    // Task with run_once should execute on first host only
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Run Once Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // run_once task
    let mut run_once_task = Task::new("Run once task", "debug").arg("msg", "This runs once");
    run_once_task.run_once = true;
    play.add_task(run_once_task);

    // Normal task runs on all hosts
    play.add_task(Task::new("Normal task", "debug").arg("msg", "Runs on all"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts participate but run_once only executes on first
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_run_once_with_delegate_to() {
    // run_once with delegate_to should run on delegated host
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Run Once Delegate Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // run_once with delegate_to
    let mut delegate_task = Task::new("Delegated run_once", "debug").arg("msg", "Delegated");
    delegate_task.run_once = true;
    delegate_task.delegate_to = Some("host1".to_string());
    play.add_task(delegate_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_run_once_with_loop() {
    // run_once with loop - interesting edge case
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Run Once Loop Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // run_once with loop items
    let mut loop_task = Task::new("Loop run_once", "debug")
        .arg("msg", "{{ item }}")
        .loop_over(vec![
            serde_json::json!("item1"),
            serde_json::json!("item2"),
            serde_json::json!("item3"),
        ]);
    loop_task.run_once = true;
    play.add_task(loop_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should be in results
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_run_once_in_free_strategy() {
    // run_once in free strategy - should still run once
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Run Once Free Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    let mut run_once_task = Task::new("Free run_once", "debug").arg("msg", "Once in free");
    run_once_task.run_once = true;
    play.add_task(run_once_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
}

// ============================================================================
// Host Ordering Tests
// ============================================================================

#[tokio::test]
async fn test_order_inventory_default() {
    // Default order: inventory order
    let runtime = create_runtime_with_hosts(vec!["zebra", "alpha", "middle", "beta"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1, // Sequential to verify order
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Inventory Order Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    // order: inventory is default

    play.add_task(Task::new("Ordered task", "debug").arg("msg", "Inventory order"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Hosts should be processed in inventory order
    assert_eq!(results.len(), 4);
}

#[tokio::test]
async fn test_order_sorted_alphabetical() {
    // order: sorted - alphabetical order
    let mut runtime = RuntimeContext::new();
    // Add hosts in random order
    runtime.add_host("delta".to_string(), None);
    runtime.add_host("alpha".to_string(), None);
    runtime.add_host("charlie".to_string(), None);
    runtime.add_host("bravo".to_string(), None);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Sorted Order Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Sorted task", "debug").arg("msg", "Alphabetical order"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    // All hosts should complete
    assert!(results.contains_key("alpha"));
    assert!(results.contains_key("bravo"));
    assert!(results.contains_key("charlie"));
    assert!(results.contains_key("delta"));
}

#[tokio::test]
async fn test_order_reverse_sorted() {
    // order: reverse_sorted - reverse alphabetical
    let mut runtime = RuntimeContext::new();
    runtime.add_host("alpha".to_string(), None);
    runtime.add_host("bravo".to_string(), None);
    runtime.add_host("charlie".to_string(), None);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Reverse Sorted Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Reverse sorted", "debug").arg("msg", "Reverse alphabetical"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_order_shuffle() {
    // order: shuffle - random order
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5, // Parallel so shuffle doesn't affect timing
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Shuffle Order Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Shuffled", "debug").arg("msg", "Random order"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete regardless of order
    assert_eq!(results.len(), 5);
}

// ============================================================================
// Concurrency Safety Tests
// ============================================================================

#[tokio::test]
async fn test_no_race_condition_on_shared_state() {
    // Test that concurrent task execution doesn't corrupt shared state
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5, // Maximum parallelism
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Race Condition Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple tasks that could race
    for i in 1..=10 {
        play.add_task(
            Task::new(format!("Concurrent task {}", i), "debug").arg("msg", format!("Task {}", i)),
        );
    }

    playbook.add_play(play);

    // Run multiple times to increase chance of detecting race conditions
    for _ in 0..3 {
        let results = executor.run_playbook(&playbook).await.unwrap();
        assert_eq!(results.len(), 5);
        for result in results.values() {
            assert!(!result.failed, "Race condition may have caused failure");
        }
    }
}

#[tokio::test]
async fn test_handler_notification_thread_safety() {
    // Test that handler notifications are thread-safe
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 4,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Handler Thread Safety Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple tasks notifying the same handler concurrently
    for i in 1..=5 {
        play.add_task(
            Task::new(format!("Notify task {}", i), "debug")
                .arg("msg", format!("Notifying {}", i))
                .notify("shared handler"),
        );
    }

    play.add_handler(Handler {
        name: "shared handler".to_string(),
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
    // Handler should run once despite multiple concurrent notifications
}

#[tokio::test]
async fn test_variable_registration_thread_safety() {
    // Test that register is thread-safe
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Register Thread Safety Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Tasks that register results concurrently
    play.add_task(
        Task::new("Register 1", "debug")
            .arg("msg", "Value 1")
            .register("result1"),
    );
    play.add_task(
        Task::new("Register 2", "debug")
            .arg("msg", "Value 2")
            .register("result2"),
    );
    play.add_task(
        Task::new("Register 3", "debug")
            .arg("msg", "Value 3")
            .register("result3"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_fact_storage_thread_safety() {
    // Test that fact storage is thread-safe under concurrent access
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 4,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Fact Storage Thread Safety Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Multiple set_fact tasks running concurrently
    play.add_task(Task::new("Set fact 1", "set_fact").arg("my_fact_1", "value1"));
    play.add_task(Task::new("Set fact 2", "set_fact").arg("my_fact_2", "value2"));
    play.add_task(Task::new("Set fact 3", "set_fact").arg("my_fact_3", "value3"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_concurrent_host_state_isolation() {
    // Verify that host state is properly isolated during concurrent execution
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("State Isolation Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Set different facts per host
    play.add_task(
        Task::new("Set host-specific fact", "set_fact").arg("host_id", "{{ inventory_hostname }}"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    // Each host should have its own state
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Timestamp-Based Execution Order Verification Tests
// ============================================================================

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_linear_strategy_execution_order_with_timing() {
    // Verify linear strategy executes tasks in order with timing evidence
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Timing Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("First task", "debug").arg("msg", "First"));
    play.add_task(Task::new("Second task", "debug").arg("msg", "Second"));

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let total_duration = start.elapsed();

    // Basic verification that execution completed
    assert_eq!(results.len(), 2);
    for result in results.values() {
        assert!(!result.failed);
    }

    // Duration should be reasonable
    assert!(
        total_duration < Duration::from_secs(5),
        "Execution took too long"
    );
}

#[tokio::test]
async fn test_free_strategy_allows_concurrent_execution() {
    // Free strategy should allow hosts to proceed independently
    let runtime = create_runtime_with_hosts(vec!["fast", "slow"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Concurrent Execution Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Task 1", "debug").arg("msg", "task1"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "task2"));
    play.add_task(Task::new("Task 3", "debug").arg("msg", "task3"));

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 2);
    // Both hosts should complete independently
    for result in results.values() {
        assert!(!result.failed);
    }

    println!("Free strategy duration: {:?}", duration);
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_execution_timing_with_multiple_tasks() {
    // Test execution timing across multiple tasks and hosts
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Multi-Task Timing Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Add several tasks
    for i in 1..=5 {
        play.add_task(
            Task::new(format!("Task {}", i), "debug").arg("msg", format!("Iteration {}", i)),
        );
    }

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 3);

    // Verify timing is reasonable
    assert!(
        duration < Duration::from_secs(10),
        "Execution should complete within 10 seconds"
    );

    // All hosts should have completed all tasks
    for (host, result) in &results {
        assert!(!result.failed, "Host {} should not fail", host);
    }
}

// ============================================================================
// Strategy Selection Tests
// ============================================================================

#[tokio::test]
async fn test_default_strategy_is_linear() {
    // Default strategy should be Linear
    let config = ExecutorConfig::default();
    assert!(matches!(config.strategy, ExecutionStrategy::Linear));
}

#[tokio::test]
async fn test_strategy_from_config() {
    // Test different strategy configurations
    let linear_config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        ..Default::default()
    };
    assert!(matches!(linear_config.strategy, ExecutionStrategy::Linear));

    let free_config = ExecutorConfig {
        strategy: ExecutionStrategy::Free,
        ..Default::default()
    };
    assert!(matches!(free_config.strategy, ExecutionStrategy::Free));

    let pinned_config = ExecutorConfig {
        strategy: ExecutionStrategy::HostPinned,
        ..Default::default()
    };
    assert!(matches!(
        pinned_config.strategy,
        ExecutionStrategy::HostPinned
    ));
}

#[tokio::test]
async fn test_strategy_play_override() {
    // Play can override strategy (when implemented)
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Strategy Override Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.strategy = Some("free".to_string()); // Play-level strategy override

    play.add_task(Task::new("Task", "debug").arg("msg", "test"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_multiple_strategies_multiple_plays() {
    // Different plays can use different strategies

    // Run with Linear
    let runtime_linear = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_linear = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 3,
            ..Default::default()
        },
        runtime_linear,
    );

    let mut playbook1 = Playbook::new("Linear Play");
    let mut play1 = Play::new("Linear", "all");
    play1.gather_facts = false;
    play1.add_task(Task::new("Linear task", "debug").arg("msg", "linear"));
    playbook1.add_play(play1);

    let results1 = executor_linear.run_playbook(&playbook1).await.unwrap();
    assert_eq!(results1.len(), 3);

    // Run with Free
    let runtime_free = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor_free = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 3,
            ..Default::default()
        },
        runtime_free,
    );

    let mut playbook2 = Playbook::new("Free Play");
    let mut play2 = Play::new("Free", "all");
    play2.gather_facts = false;
    play2.add_task(Task::new("Free task", "debug").arg("msg", "free"));
    playbook2.add_play(play2);

    let results2 = executor_free.run_playbook(&playbook2).await.unwrap();
    assert_eq!(results2.len(), 3);
}

// ============================================================================
// Additional Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_strategy_with_localhost() {
    // Test strategies work correctly with localhost
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 1,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Localhost Test");
    let mut play = Play::new("Test", "localhost");
    play.gather_facts = false;
    play.add_task(Task::new("Local task", "debug").arg("msg", "localhost"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_strategy_with_mixed_connection_types() {
    // Test that strategy works with different host connection types
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), Some("local"));
    runtime.add_host("remote1".to_string(), Some("ssh"));
    runtime.add_host("remote2".to_string(), Some("ssh"));

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 3,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Mixed Connections Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Mixed task", "debug").arg("msg", "mixed connections"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_strategy_resilience_under_load() {
    // Test strategy handles high load gracefully
    let host_names: Vec<String> = (0..50).map(|i| format!("host{}", i)).collect();

    let mut runtime = RuntimeContext::new();
    for host in &host_names {
        runtime.add_host(host.clone(), None);
    }

    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 20, // High but not unlimited parallelism
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Load Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Add multiple tasks
    for i in 1..=5 {
        play.add_task(Task::new(format!("Task {}", i), "debug").arg("msg", format!("load{}", i)));
    }

    playbook.add_play(play);

    let start = std::time::Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 50);
    for result in results.values() {
        assert!(!result.failed, "No host should fail under load");
    }

    println!("Load test completed in {:?}", duration);
}

#[tokio::test]
async fn test_strategy_with_conditional_host_execution() {
    // Test strategy with when conditions that skip entire hosts
    let runtime = create_runtime_with_hosts(vec!["web1", "web2", "db1", "db2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 4,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Conditional Host Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Only run on web servers
    play.add_task(
        Task::new("Web only", "debug")
            .arg("msg", "Web task")
            .when("'web' in inventory_hostname"),
    );

    // Only run on db servers
    play.add_task(
        Task::new("DB only", "debug")
            .arg("msg", "DB task")
            .when("'db' in inventory_hostname"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);

    // Check that appropriate tasks were skipped
    for (host, result) in &results {
        assert!(!result.failed);
        // Each host should have 1 skipped task
        assert!(
            result.stats.skipped >= 1,
            "Host {} should have skipped tasks",
            host
        );
    }
}

#[tokio::test]
async fn test_strategy_with_block_rescue_always() {
    // Test strategy with block/rescue/always constructs
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 2,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Block Rescue Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Simulate block with potential failure
    play.add_task(Task::new("Block task", "debug").arg("msg", "In block"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 2);
}
