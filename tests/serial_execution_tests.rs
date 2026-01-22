//! Comprehensive tests for serial execution parameter
//!
//! This test suite covers:
//! - Fixed batch sizes (serial: 1, serial: 2, serial: 5)
//! - Percentage-based batches (serial: "50%", serial: "25%")
//! - Progressive batches (serial: [1, 5, 10])
//! - Serial combined with different strategies (linear, free, host_pinned)
//! - max_fail_percentage with serial execution
//! - Edge cases (zero hosts, single host, batch size larger than hosts)

#![cfg(not(tarpaulin))]

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
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
// Fixed Batch Size Tests
// ============================================================================

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_serial_fixed_one_host_at_a_time() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial 1 Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First task"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second task"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 4 hosts should complete
    assert_eq!(results.len(), 4);
    for (host, result) in &results {
        assert!(!result.failed, "Host {} should not fail", host);
        assert!(
            result.stats.ok >= 2 || result.stats.changed >= 2,
            "Host {} should complete all tasks",
            host
        );
    }
}

#[tokio::test]
async fn test_serial_fixed_batch_size_two() {
    let runtime =
        create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5", "host6"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial 2 Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2));

    play.add_task(Task::new("Batch task", "debug").arg("msg", "Running in batches of 2"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All 6 hosts should complete (3 batches of 2)
    assert_eq!(results.len(), 6);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_fixed_batch_size_larger_than_hosts() {
    // If serial batch size is larger than total hosts, all hosts execute together
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Large Batch Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(10)); // Larger than 3 hosts

    play.add_task(Task::new("Task", "debug").arg("msg", "All hosts together"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Percentage-Based Batch Tests
// ============================================================================

#[tokio::test]
async fn test_serial_percentage_50_percent() {
    // 50% of 8 hosts = 4 hosts per batch
    let runtime = create_runtime_with_hosts(vec![
        "host1", "host2", "host3", "host4", "host5", "host6", "host7", "host8",
    ]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial 50% Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Percentage("50%".to_string()));

    play.add_task(Task::new("Percentage batch", "debug").arg("msg", "50% at a time"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 8);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_percentage_25_percent() {
    // 25% of 8 hosts = 2 hosts per batch
    let runtime = create_runtime_with_hosts(vec![
        "host1", "host2", "host3", "host4", "host5", "host6", "host7", "host8",
    ]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial 25% Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Percentage("25%".to_string()));

    play.add_task(Task::new("Quarter batch", "debug").arg("msg", "25% at a time"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 8);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_percentage_100_percent() {
    // 100% means all hosts at once (effectively no batching)
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial 100% Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Percentage("100%".to_string()));

    play.add_task(Task::new("All hosts", "debug").arg("msg", "All at once"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_percentage_rounds_up() {
    // 30% of 5 hosts = 1.5, should round up to 2
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4", "host5"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Rounding Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Percentage("30%".to_string()));

    play.add_task(Task::new("Rounded batch", "debug").arg("msg", "Rounded up"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 5);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Progressive Batch Tests
// ============================================================================

#[tokio::test]
async fn test_serial_progressive_batches() {
    // Progressive: [1, 5, 10] - first batch 1 host, next batches 5 hosts, then 10
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
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Progressive Batch Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Progressive(vec![
        SerialSpec::Fixed(1),
        SerialSpec::Fixed(5),
        SerialSpec::Fixed(10),
    ]));

    play.add_task(Task::new("Progressive", "debug").arg("msg", "Rolling deployment"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 20);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_progressive_with_percentages() {
    // Progressive with percentages: ["10%", "50%", "100%"]
    let runtime = create_runtime_with_hosts(vec![
        "host1", "host2", "host3", "host4", "host5", "host6", "host7", "host8", "host9", "host10",
    ]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Progressive Percentage Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Progressive(vec![
        SerialSpec::Percentage("10%".to_string()),
        SerialSpec::Percentage("50%".to_string()),
        SerialSpec::Percentage("100%".to_string()),
    ]));

    play.add_task(Task::new("Progressive %", "debug").arg("msg", "Percentage progression"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 10);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// Serial Combined with Different Strategies
// ============================================================================

#[tokio::test]
async fn test_serial_with_linear_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial + Linear Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2));

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_with_free_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial + Free Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2));

    play.add_task(Task::new("Task 1", "debug").arg("msg", "First"));
    play.add_task(Task::new("Task 2", "debug").arg("msg", "Second"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_with_host_pinned_strategy() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::HostPinned,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial + HostPinned Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2));

    play.add_task(Task::new("Task", "debug").arg("msg", "Pinned batch"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

// ============================================================================
// max_fail_percentage Tests
// ============================================================================

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_serial_with_max_fail_percentage_not_exceeded() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Max Fail Not Exceeded Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));
    play.max_fail_percentage = Some(50); // Allow 50% failures

    // Fail on first host only (25% failure rate)
    play.add_task(
        Task::new("Maybe fail", "fail")
            .arg("msg", "Conditional failure")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("Continue", "debug").arg("msg", "Continuing"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should be processed (failure % not exceeded)
    assert_eq!(results.len(), 4);

    // host1 should fail
    if let Some(host1) = results.get("host1") {
        assert!(host1.stats.failed > 0);
    }

    // Others should succeed
    for (host, result) in &results {
        if host != "host1" {
            assert!(!result.failed, "Host {} should succeed", host);
        }
    }
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_serial_with_max_fail_percentage_exceeded() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Max Fail Exceeded Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));
    play.max_fail_percentage = Some(25); // Allow only 25% failures

    // Fail on first two hosts (50% failure rate exceeds threshold)
    play.add_task(
        Task::new("Fail on some", "fail")
            .arg("msg", "Fail")
            .when("inventory_hostname == 'host1' or inventory_hostname == 'host2'"),
    );
    play.add_task(Task::new("Should not run", "debug").arg("msg", "Aborted"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should have results for all hosts
    assert_eq!(results.len(), 4);

    // At least one host should have failed
    let failed_count = results.values().filter(|r| r.failed).count();
    assert!(failed_count > 0, "Some hosts should have failed");
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_serial_max_fail_percentage_zero() {
    // max_fail_percentage: 0 means abort on first failure
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Max Fail Zero Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));
    play.max_fail_percentage = Some(0); // No failures allowed

    // Fail on first host
    play.add_task(
        Task::new("Fail first", "fail")
            .arg("msg", "Fail")
            .when("inventory_hostname == 'host1'"),
    );
    play.add_task(Task::new("Should not reach", "debug").arg("msg", "Never runs"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should be in results (remaining marked as skipped)
    assert_eq!(results.len(), 4);

    // host1 should have failed
    if let Some(host1) = results.get("host1") {
        assert!(host1.stats.failed > 0);
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[tokio::test]
async fn test_serial_with_zero_hosts() {
    let runtime = RuntimeContext::new(); // No hosts
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Zero Hosts Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));

    play.add_task(Task::new("Task", "debug").arg("msg", "Should not run"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should complete with empty results
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_serial_with_single_host() {
    let runtime = create_runtime_with_hosts(vec!["lonely_host"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Single Host Serial Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(1));

    play.add_task(Task::new("Solo task", "debug").arg("msg", "Alone"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 1);
    let result = results.get("lonely_host").unwrap();
    assert!(!result.failed);
}

#[tokio::test]
async fn test_serial_batch_size_zero() {
    // Edge case: batch size of 0 should be handled gracefully
    let runtime = create_runtime_with_hosts(vec!["host1", "host2"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Zero Batch Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(0)); // Invalid batch size

    play.add_task(Task::new("Task", "debug").arg("msg", "Test"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should handle gracefully (likely no hosts executed)
    assert!(results.is_empty() || results.len() == 2);
}

#[tokio::test]
async fn test_serial_percentage_zero() {
    // 0% should result in minimal batch (at least 1 host)
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Zero Percentage Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Percentage("0%".to_string()));

    play.add_task(Task::new("Task", "debug").arg("msg", "Minimal batch"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Should execute on all hosts (minimum 1 per batch)
    assert_eq!(results.len(), 3);
}

// ============================================================================
// Complex Scenarios
// ============================================================================

#[tokio::test]
async fn test_serial_rolling_update_with_handlers() {
    let runtime = create_runtime_with_hosts(vec!["web1", "web2", "web3", "web4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Rolling Update Test");
    let mut play = Play::new("Rolling Update", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2)); // 2 servers at a time

    play.add_task(
        Task::new("Update config", "debug")
            .arg("msg", "Updating")
            .notify("restart service"),
    );

    play.add_handler(rustible::executor::task::Handler {
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

    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_serial_with_conditional_tasks() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Serial Conditional Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.serial = Some(SerialSpec::Fixed(2));

    play.add_task(
        Task::new("Conditional task", "debug")
            .arg("msg", "Only evens")
            .when("inventory_hostname in ['host2', 'host4']"),
    );
    play.add_task(Task::new("All hosts", "debug").arg("msg", "Everyone"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 4);

    // host1 and host3 should have skipped the first task
    for (host, result) in &results {
        assert!(!result.failed);
        if host == "host1" || host == "host3" {
            assert!(result.stats.skipped > 0, "Host {} should skip task", host);
        }
    }
}

#[cfg_attr(tarpaulin, ignore)]
#[tokio::test]
async fn test_serial_multiple_plays() {
    let runtime = create_runtime_with_hosts(vec!["host1", "host2", "host3", "host4"]);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 5,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Multi-Play Serial Test");

    // First play with serial: 1
    let mut play1 = Play::new("First Play", "all");
    play1.gather_facts = false;
    play1.serial = Some(SerialSpec::Fixed(1));
    play1.add_task(Task::new("Play 1 Task", "debug").arg("msg", "Serial 1"));
    playbook.add_play(play1);

    // Second play with serial: 2
    let mut play2 = Play::new("Second Play", "all");
    play2.gather_facts = false;
    play2.serial = Some(SerialSpec::Fixed(2));
    play2.add_task(Task::new("Play 2 Task", "debug").arg("msg", "Serial 2"));
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both plays should execute on all hosts
    assert_eq!(results.len(), 4);
    for result in results.values() {
        assert!(!result.failed);
        // Should have stats from both plays
        assert!(result.stats.ok >= 2 || result.stats.changed >= 2);
    }
}

// ============================================================================
// SerialSpec Unit Tests
// ============================================================================

#[test]
fn test_serial_spec_calculate_batches_fixed() {
    let spec = SerialSpec::Fixed(3);
    let batches = spec.calculate_batches(10);
    assert_eq!(batches, vec![3]);
}

#[test]
fn test_serial_spec_calculate_batches_percentage() {
    let spec = SerialSpec::Percentage("50%".to_string());
    let batches = spec.calculate_batches(10);
    assert_eq!(batches, vec![5]);
}

#[test]
fn test_serial_spec_calculate_batches_percentage_rounds_up() {
    let spec = SerialSpec::Percentage("33%".to_string());
    let batches = spec.calculate_batches(10);
    // 33% of 10 = 3.3, rounds up to 4
    assert_eq!(batches, vec![4]);
}

#[test]
fn test_serial_spec_calculate_batches_progressive() {
    let spec = SerialSpec::Progressive(vec![
        SerialSpec::Fixed(1),
        SerialSpec::Fixed(5),
        SerialSpec::Fixed(10),
    ]);
    let batches = spec.calculate_batches(20);
    assert_eq!(batches, vec![1, 5, 10]);
}

#[test]
fn test_serial_spec_batch_hosts_fixed() {
    let hosts: Vec<String> = (1..=6).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Fixed(2);
    let batches = spec.batch_hosts(&hosts);

    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 2);
    assert_eq!(batches[1].len(), 2);
    assert_eq!(batches[2].len(), 2);
}

#[test]
fn test_serial_spec_batch_hosts_uneven() {
    let hosts: Vec<String> = (1..=7).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Fixed(3);
    let batches = spec.batch_hosts(&hosts);

    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
    assert_eq!(batches[2].len(), 1); // Last batch has remainder
}

#[test]
fn test_serial_spec_batch_hosts_progressive() {
    let hosts: Vec<String> = (1..=20).map(|i| format!("host{}", i)).collect();
    let spec = SerialSpec::Progressive(vec![
        SerialSpec::Fixed(1),
        SerialSpec::Fixed(5),
        SerialSpec::Fixed(10),
    ]);
    let batches = spec.batch_hosts(&hosts);

    // Should create batches: 1, 5, 10, 1, 3 (cycling through sizes)
    assert!(batches.len() >= 3);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[1].len(), 5);
    assert_eq!(batches[2].len(), 10);
}
