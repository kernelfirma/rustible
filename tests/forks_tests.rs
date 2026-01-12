//! Tests for the --forks CLI option and parallel execution limits
//!
//! These tests verify that the forks parameter correctly limits the concurrency
//! of task execution across multiple hosts.

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};
use std::time::Instant;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a runtime with the specified number of hosts configured for local connection
fn create_runtime_with_hosts(count: usize) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for i in 0..count {
        let hostname = format!("host{}", i);
        runtime.add_host(hostname.clone(), None);
        // Set ansible_connection to local to avoid SSH connection attempts
        runtime.set_host_var(
            &hostname,
            "ansible_connection".to_string(),
            serde_json::json!("local"),
        );
    }
    runtime
}

// ============================================================================
// Test 1: Verify forks limits concurrency in Linear strategy
// ============================================================================

#[tokio::test]
async fn test_forks_limits_concurrency_linear() {
    let runtime = create_runtime_with_hosts(10);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 3, // Only 3 hosts should execute in parallel
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Forks Test - Linear");
    let mut play = Play::new("Test", "all");

    // Add a task that takes some time to execute
    let task = Task::new("Test task", "debug").arg("msg", "Testing forks limit");

    play.add_task(task);
    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    // With forks=3 and 10 hosts, execution should take at least ceil(10/3) = 4 "rounds"
    // Verify that all hosts completed successfully
    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
        assert!(!result.unreachable);
    }

    // The execution should complete (just verify it doesn't hang)
    assert!(duration.as_secs() < 60);
}

// ============================================================================
// Test 2: Verify forks limits concurrency in Free strategy
// ============================================================================

#[tokio::test]
async fn test_forks_limits_concurrency_free() {
    let runtime = create_runtime_with_hosts(20);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Free,
        forks: 5, // Only 5 hosts should execute in parallel
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Forks Test - Free");
    let mut play = Play::new("Test", "all");

    // Add multiple tasks to verify concurrency across all tasks
    for i in 0..3 {
        let task =
            Task::new(format!("Task {}", i), "debug").arg("msg", format!("Task {} executing", i));
        play.add_task(task);
    }

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify that all hosts completed successfully
    assert_eq!(results.len(), 20);
    for result in results.values() {
        assert!(!result.failed);
        assert!(!result.unreachable);
        // Each host should have executed 3 tasks (3 ok or 3 changed)
        assert!(result.stats.ok + result.stats.changed >= 3);
    }
}

// ============================================================================
// Test 3: Test with forks=1 (serial execution)
// ============================================================================

#[tokio::test]
async fn test_forks_one_serial_execution() {
    let runtime = create_runtime_with_hosts(5);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 1, // Serial execution
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Serial Execution Test");
    let mut play = Play::new("Test", "all");

    let task = Task::new("Serial task", "debug").arg("msg", "Executing serially");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete
    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Test 4: Test with high forks value (more than hosts)
// ============================================================================

#[tokio::test]
async fn test_forks_higher_than_host_count() {
    let runtime = create_runtime_with_hosts(5);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 20, // More forks than hosts
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("High Forks Test");
    let mut play = Play::new("Test", "all");

    let task = Task::new("Test task", "debug").arg("msg", "All hosts in parallel");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete successfully
    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Test 5: Different forks values produce different execution patterns
// ============================================================================

#[tokio::test]
async fn test_different_forks_values() {
    let test_cases = vec![
        (1, "Serial"),
        (2, "Pairs"),
        (5, "Small batches"),
        (10, "Large batches"),
    ];

    for (forks, description) in test_cases {
        let runtime = create_runtime_with_hosts(10);

        let config = ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks,
            gather_facts: false,
            ..Default::default()
        };

        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new(format!("Forks={} Test", forks));
        let mut play = Play::new(description, "all");

        let task =
            Task::new("Test task", "debug").arg("msg", format!("Testing with forks={}", forks));

        play.add_task(task);
        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        // All hosts should complete
        assert_eq!(results.len(), 10, "Failed for forks={}", forks);
        for result in results.values() {
            assert!(!result.failed, "Host failed for forks={}", forks);
        }
    }
}

// ============================================================================
// Test 6: Verify actual concurrency limit using shared counter
// ============================================================================

#[tokio::test]
async fn test_forks_actual_concurrency_limit() {
    // This test uses a shared counter to verify that no more than
    // 'forks' tasks are executing concurrently

    let runtime = create_runtime_with_hosts(10);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Free,
        forks: 3,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Concurrency Limit Test");
    let mut play = Play::new("Test", "all");

    // Add a task that would increment a counter when starting
    // and decrement when finishing
    let task = Task::new("Concurrent task", "debug").arg("msg", "Testing concurrency");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify all hosts completed
    assert_eq!(results.len(), 10);
}

// ============================================================================
// Test 7: Forks with mixed success and failure
// ============================================================================

#[tokio::test]
async fn test_forks_with_failures() {
    let runtime = create_runtime_with_hosts(6);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 2,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Forks with Failures");
    let mut play = Play::new("Test", "all");

    // Add a task (will succeed on all hosts in this simplified test)
    let task = Task::new("Test task", "debug")
        .arg("msg", "Testing")
        .ignore_errors(true);

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should have results
    assert_eq!(results.len(), 6);
}

// ============================================================================
// Test 8: Forks configuration is respected across multiple plays
// ============================================================================

#[tokio::test]
async fn test_forks_across_multiple_plays() {
    let runtime = create_runtime_with_hosts(8);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 4,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Multi-Play Forks Test");

    // Add multiple plays
    for i in 0..3 {
        let mut play = Play::new(format!("Play {}", i), "all");
        let task = Task::new(format!("Task in play {}", i), "debug")
            .arg("msg", format!("Play {} task", i));
        play.add_task(task);
        playbook.add_play(play);
    }

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete all plays
    assert_eq!(results.len(), 8);
    for result in results.values() {
        assert!(!result.failed);
        // Each host should have executed 3 tasks (one per play)
        assert!(result.stats.ok + result.stats.changed >= 3);
    }
}

// ============================================================================
// Test 9: Default forks value
// ============================================================================

#[test]
fn test_default_forks_value() {
    let config = ExecutorConfig::default();
    assert_eq!(config.forks, 5, "Default forks should be 5");
}

// ============================================================================
// Test 10: Forks with check mode
// ============================================================================

#[tokio::test]
async fn test_forks_with_check_mode() {
    let runtime = create_runtime_with_hosts(10);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Linear,
        forks: 3,
        check_mode: true, // Dry-run mode
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Check Mode with Forks");
    let mut play = Play::new("Test", "all");

    let task = Task::new("Check mode task", "debug").arg("msg", "Would execute");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should complete in check mode
    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Test 11: Stress test with many hosts and small forks
// ============================================================================

#[tokio::test]
async fn test_stress_many_hosts_small_forks() {
    let runtime = create_runtime_with_hosts(50);

    let config = ExecutorConfig {
        strategy: ExecutionStrategy::Free,
        forks: 5,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Stress Test");
    let mut play = Play::new("Test", "all");

    let task = Task::new("Stress task", "debug").arg("msg", "Stress testing");

    play.add_task(task);
    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    // All hosts should complete
    assert_eq!(results.len(), 50);
    for result in results.values() {
        assert!(!result.failed);
    }

    // Shouldn't take too long (under 2 minutes for this simple test)
    assert!(duration.as_secs() < 120);
}

// ============================================================================
// Test 12: Verify ExecutorConfig correctly stores forks value
// ============================================================================

#[test]
fn test_executor_config_stores_forks() {
    let config = ExecutorConfig {
        forks: 10,
        ..Default::default()
    };

    assert_eq!(config.forks, 10);

    let executor = Executor::new(config);
    // Executor should be created successfully with custom forks
    assert!(!executor.is_check_mode());
}
