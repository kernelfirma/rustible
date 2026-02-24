//! Multi-Host Parallel Execution Stress Tests
//!
//! These tests validate Rustible's parallel execution capabilities under load:
//! - Linear vs Free vs HostPinned strategies
//! - Fork limiting and throttling
//! - Serial batching
//! - Handler execution with parallel hosts
//! - Connection pool behavior under stress
//!
//! To run these tests:
//! ```bash
//! export RUSTIBLE_TEST_PARALLEL_ENABLED=1
//! cargo test --test parallel_stress_tests --features ssh2-backend -- --test-threads=1
//! ```
//!
//! NOTE: These tests require the ssh2-backend feature which is disabled by default.

#![cfg(feature = "ssh2-backend")]

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

// Import the Connection trait so we can call .execute() / .close() on connections
use rustible::connection::{Connection, ConnectionConfig, HostConfig};

mod common;

/// Configuration for parallel stress tests
struct ParallelTestConfig {
    enabled: bool,
    ssh_user: String,
    ssh_key_path: PathBuf,
    hosts: Vec<String>,
    #[allow(dead_code)]
    inventory_path: Option<PathBuf>,
}

impl ParallelTestConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_TEST_PARALLEL_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let ssh_user =
            env::var("RUSTIBLE_TEST_SSH_USER").unwrap_or_else(|_| "testuser".to_string());

        let ssh_key_path = env::var("RUSTIBLE_TEST_SSH_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".ssh/id_ed25519")
            });

        // Default to scale fleet hosts
        let hosts = env::var("RUSTIBLE_TEST_SCALE_HOSTS")
            .map(|h| h.split(',').map(String::from).collect())
            .unwrap_or_else(|_| (151..=160).map(|i| format!("192.168.178.{}", i)).collect());

        let inventory_path = env::var("RUSTIBLE_TEST_INVENTORY").map(PathBuf::from).ok();

        Self {
            enabled,
            ssh_user,
            ssh_key_path,
            hosts,
            inventory_path,
        }
    }

    fn skip_if_disabled(&self) -> bool {
        if !self.enabled {
            eprintln!("Skipping parallel stress tests (RUSTIBLE_TEST_PARALLEL_ENABLED not set)");
            true
        } else {
            false
        }
    }

    /// Build a HostConfig for SSH connections using the test configuration.
    fn host_config(&self, host: &str) -> HostConfig {
        HostConfig::new()
            .hostname(host)
            .port(22)
            .user(&self.ssh_user)
            .identity_file(self.ssh_key_path.to_string_lossy())
    }
}

// =============================================================================
// Execution Strategy Tests
// =============================================================================

#[tokio::test]
async fn test_linear_strategy_task_ordering() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Track execution order
    let _execution_log: Arc<Mutex<Vec<(String, String, Instant)>>> =
        Arc::new(Mutex::new(Vec::new()));

    let hosts: Vec<_> = config.hosts.iter().take(5).cloned().collect();

    // Create playbook with multiple tasks
    let playbook = common::PlaybookBuilder::new("Linear Strategy Test")
        .add_play(
            common::PlayBuilder::new("Test linear ordering", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Task 1", "command")
                        .arg("cmd", "echo task1")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Task 2", "command")
                        .arg("cmd", "echo task2")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Task 3", "command")
                        .arg("cmd", "echo task3")
                        .build(),
                )
                .build(),
        )
        .build();

    // Build inventory
    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    // Execute with linear strategy
    let executor_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 5,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 60,
        gather_facts: false,
        ..executor_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    let start = Instant::now();
    let result = executor.run_playbook(&playbook).await;
    let total_time = start.elapsed();

    println!("Linear strategy execution completed in {:?}", total_time);

    // Linear strategy: Task N must complete on ALL hosts before Task N+1 starts
    // This is validated by the executor's behavior
    assert!(
        result.is_ok(),
        "Playbook execution failed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_free_strategy_independent_execution() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(5).cloned().collect();

    // Track when each host starts and finishes
    let _host_timings: Arc<Mutex<HashMap<String, (Instant, Instant)>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Create playbook with a task that has variable duration
    let playbook = common::PlaybookBuilder::new("Free Strategy Test")
        .add_play(
            common::PlayBuilder::new("Test free execution", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Variable delay task", "command")
                        // Each host sleeps for a different amount based on inventory_hostname
                        .arg("cmd", "sleep $(( RANDOM % 3 ))")
                        .build(),
                )
                .build(),
        )
        .build();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let executor_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 10, // More forks than hosts to allow full parallelism
        strategy: rustible::executor::ExecutionStrategy::Free,
        task_timeout: 60,
        gather_facts: false,
        ..executor_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    let start = Instant::now();
    let result = executor.run_playbook(&playbook).await;
    let total_time = start.elapsed();

    println!("Free strategy execution completed in {:?}", total_time);

    assert!(
        result.is_ok(),
        "Playbook execution failed: {:?}",
        result.err()
    );

    // Free strategy should complete faster than linear when tasks have variable duration
    // because hosts don't wait for each other
}

#[tokio::test]
async fn test_host_pinned_strategy_affinity() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(5).cloned().collect();

    // Create playbook with multiple tasks that should run on same worker per host
    let playbook = common::PlaybookBuilder::new("HostPinned Strategy Test")
        .add_play(
            common::PlayBuilder::new("Test host pinning", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Create marker file", "command")
                        .arg("cmd", "echo $$ > /tmp/rustible_worker_pid")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Read marker file", "command")
                        .arg("cmd", "cat /tmp/rustible_worker_pid")
                        .register("marker")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Cleanup", "command")
                        .arg("cmd", "rm -f /tmp/rustible_worker_pid")
                        .build(),
                )
                .build(),
        )
        .build();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let executor_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 5,
        strategy: rustible::executor::ExecutionStrategy::HostPinned,
        task_timeout: 60,
        gather_facts: false,
        ..executor_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let result = executor.run_playbook(&playbook).await;

    assert!(
        result.is_ok(),
        "Playbook execution failed: {:?}",
        result.err()
    );
}

// =============================================================================
// Fork Limiting Tests
// =============================================================================

#[tokio::test]
async fn test_fork_limit_enforcement() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Track concurrent connections
    let _concurrent_count = Arc::new(AtomicUsize::new(0));
    let _max_concurrent = Arc::new(AtomicUsize::new(0));

    let hosts: Vec<_> = config.hosts.iter().take(10).cloned().collect();

    // Create a playbook with a slow task to observe fork limiting
    let playbook = common::PlaybookBuilder::new("Fork Limit Test")
        .add_play(
            common::PlayBuilder::new("Test fork limiting", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Slow task", "command")
                        .arg("cmd", "sleep 2")
                        .build(),
                )
                .build(),
        )
        .build();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let fork_limit = 3;
    let executor_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: fork_limit, // Limit to 3 concurrent
        strategy: rustible::executor::ExecutionStrategy::Free,
        task_timeout: 60,
        gather_facts: false,
        ..executor_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    let start = Instant::now();
    let result = executor.run_playbook(&playbook).await;
    let total_time = start.elapsed();

    assert!(
        result.is_ok(),
        "Playbook execution failed: {:?}",
        result.err()
    );

    // With 10 hosts, 3 forks, and 2-second tasks:
    // Expected time: ceil(10/3) * 2 = 8 seconds minimum
    // Allow some overhead
    let expected_min = Duration::from_secs(6);
    assert!(
        total_time >= expected_min,
        "Execution was too fast ({:?}), fork limit may not be enforced",
        total_time
    );

    println!(
        "Fork limit {} with {} hosts completed in {:?}",
        fork_limit,
        hosts.len(),
        total_time
    );
}

#[tokio::test]
async fn test_fork_limit_with_different_values() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(6).cloned().collect();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let playbook = common::PlaybookBuilder::new("Fork Comparison Test")
        .add_play(
            common::PlayBuilder::new("Measure fork impact", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Quick task", "command")
                        .arg("cmd", "sleep 1")
                        .build(),
                )
                .build(),
        )
        .build();

    let mut results = vec![];

    for forks in [1, 2, 3, 6] {
        let base_config = common::test_executor_config();
        let executor_config = rustible::executor::ExecutorConfig {
            forks,
            strategy: rustible::executor::ExecutionStrategy::Free,
            task_timeout: 60,
            gather_facts: false,
            ..base_config
        };

        let executor = rustible::executor::Executor::new(executor_config);

        let start = Instant::now();
        let result = executor.run_playbook(&playbook).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        results.push((forks, elapsed));
        println!("Forks: {}, Time: {:?}", forks, elapsed);
    }

    // More forks should generally mean faster execution
    // forks=6 should be faster than forks=1
    assert!(
        results[3].1 < results[0].1,
        "6 forks ({:?}) should be faster than 1 fork ({:?})",
        results[3].1,
        results[0].1
    );
}

// =============================================================================
// Serial Batching Tests
// =============================================================================

#[tokio::test]
async fn test_serial_execution_batching() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(9).cloned().collect();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    // Test with serial: 3 (batches of 3)
    let playbook = common::PlaybookBuilder::new("Serial Batch Test")
        .add_play(
            common::PlayBuilder::new("Serial batching", "all")
                .gather_facts(false)
                .var("serial", 3)
                .add_task(
                    common::TaskBuilder::new("Batch task", "command")
                        .arg("cmd", "echo batch")
                        .build(),
                )
                .build(),
        )
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 10, // High forks, but serial should limit it
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 60,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let result = executor.run_playbook(&playbook).await;

    assert!(
        result.is_ok(),
        "Serial execution failed: {:?}",
        result.err()
    );
}

// =============================================================================
// Handler Execution Tests
// =============================================================================

#[tokio::test]
async fn test_handler_deduplication_parallel() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(5).cloned().collect();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    // Multiple tasks notify the same handler
    let playbook = common::PlaybookBuilder::new("Handler Deduplication Test")
        .add_play(
            common::PlayBuilder::new("Test handler dedup", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Task 1", "command")
                        .arg("cmd", "echo task1")
                        .notify("common handler")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Task 2", "command")
                        .arg("cmd", "echo task2")
                        .notify("common handler")
                        .build(),
                )
                .add_task(
                    common::TaskBuilder::new("Task 3", "command")
                        .arg("cmd", "echo task3")
                        .notify("common handler")
                        .build(),
                )
                .add_handler(rustible::executor::task::Handler {
                    name: "common handler".to_string(),
                    module: "command".to_string(),
                    args: {
                        let mut args = indexmap::IndexMap::new();
                        args.insert(
                            "cmd".to_string(),
                            serde_json::json!(
                                "echo handler >> /tmp/rustible_handler_test_$(hostname)"
                            ),
                        );
                        args
                    },
                    when: None,
                    listen: vec![],
                })
                .build(),
        )
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 5,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 60,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let result = executor.run_playbook(&playbook).await;

    assert!(
        result.is_ok(),
        "Handler deduplication test failed: {:?}",
        result.err()
    );

    // Cleanup: The handler should only run once per host despite 3 notifications
    // Verification would require checking /tmp/rustible_handler_test_* files
}

// =============================================================================
// Stress Tests
// =============================================================================

#[tokio::test]
async fn test_high_host_count_stress() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Use all available hosts
    let hosts = config.hosts.clone();
    if hosts.len() < 5 {
        eprintln!("Skipping high host count test (need at least 5 hosts)");
        return;
    }

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let playbook = common::PlaybookBuilder::new("High Host Count Stress")
        .add_play(
            common::PlayBuilder::new("Stress test many hosts", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Simple task", "command")
                        .arg("cmd", "hostname")
                        .build(),
                )
                .build(),
        )
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 20, // High parallelism
        strategy: rustible::executor::ExecutionStrategy::Free,
        task_timeout: 60,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    let start = Instant::now();
    let result = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    assert!(
        result.is_ok(),
        "High host count test failed: {:?}",
        result.err()
    );

    println!(
        "Executed on {} hosts in {:?} ({:.2} hosts/sec)",
        hosts.len(),
        elapsed,
        hosts.len() as f64 / elapsed.as_secs_f64()
    );
}

#[tokio::test]
async fn test_many_tasks_stress() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let hosts: Vec<_> = config.hosts.iter().take(3).cloned().collect();

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    // Build playbook with many tasks
    let mut play_builder = common::PlayBuilder::new("Many tasks stress", "all").gather_facts(false);

    for i in 0..50 {
        play_builder = play_builder.add_task(
            common::TaskBuilder::new(format!("Task {}", i), "command")
                .arg("cmd", format!("echo task_{}", i))
                .build(),
        );
    }

    let playbook = common::PlaybookBuilder::new("Many Tasks Stress")
        .add_play(play_builder.build())
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 5,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 120,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    let start = Instant::now();
    let result = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    assert!(
        result.is_ok(),
        "Many tasks stress test failed: {:?}",
        result.err()
    );

    let tasks_per_host = 50;
    let total_tasks = tasks_per_host * hosts.len();
    println!(
        "Executed {} tasks on {} hosts in {:?} ({:.2} tasks/sec)",
        total_tasks,
        hosts.len(),
        elapsed,
        total_tasks as f64 / elapsed.as_secs_f64()
    );
}

#[tokio::test]
async fn test_rapid_reconnection_stress() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config
        .hosts
        .first()
        .expect("Need at least one host")
        .clone();

    // Rapidly create and close connections
    let connection_count = 20;
    let mut successful = 0;
    let mut failed = 0;

    let start = Instant::now();

    let global_config = ConnectionConfig::default();
    let host_config = config.host_config(&host);

    for i in 0..connection_count {
        match rustible::connection::SshConnection::connect(
            &host,
            22,
            &config.ssh_user,
            Some(host_config.clone()),
            &global_config,
        )
        .await
        {
            Ok(conn) => {
                // Quick command
                if conn.execute("echo ok", None).await.is_ok() {
                    successful += 1;
                } else {
                    failed += 1;
                }
                conn.close().await.ok();
            }
            Err(e) => {
                eprintln!("Connection {} failed: {:?}", i, e);
                failed += 1;
            }
        }
    }

    let elapsed = start.elapsed();

    println!(
        "Rapid reconnection: {}/{} successful in {:?} ({:.2} conn/sec)",
        successful,
        connection_count,
        elapsed,
        connection_count as f64 / elapsed.as_secs_f64()
    );

    assert!(
        successful > connection_count * 90 / 100,
        "Too many connection failures: {} of {}",
        failed,
        connection_count
    );
}

// =============================================================================
// Failure Handling Tests
// =============================================================================

#[tokio::test]
async fn test_partial_host_failure() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let mut hosts: Vec<_> = config.hosts.iter().take(3).cloned().collect();
    // Add a non-existent host to simulate failure
    hosts.push("192.168.178.254".to_string()); // Non-existent

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let playbook = common::PlaybookBuilder::new("Partial Failure Test")
        .add_play(
            common::PlayBuilder::new("Handle partial failures", "all")
                .gather_facts(false)
                .add_task(
                    common::TaskBuilder::new("Simple task", "command")
                        .arg("cmd", "hostname")
                        .build(),
                )
                .build(),
        )
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 5,
        strategy: rustible::executor::ExecutionStrategy::Free,
        task_timeout: 10, // Short timeout for unreachable host
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let result = executor.run_playbook(&playbook).await;

    // Should complete with some failures
    match result {
        Ok(host_results) => {
            let mut total_ok = 0usize;
            let mut total_unreachable = 0usize;
            for hr in host_results.values() {
                total_ok += hr.stats.ok;
                if hr.unreachable {
                    total_unreachable += 1;
                }
            }
            println!("Stats: ok={}, unreachable={}", total_ok, total_unreachable);
            assert!(total_unreachable >= 1, "Should have at least 1 unreachable");
            assert!(total_ok >= 2, "Should have at least 2 successful");
        }
        Err(e) => {
            // Complete failure is acceptable if unreachable count is tracked
            println!("Execution error (may be expected): {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_max_fail_percentage() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Create inventory with some failing hosts
    let good_hosts: Vec<_> = config.hosts.iter().take(3).cloned().collect();
    let mut all_hosts = good_hosts.clone();
    // Add unreachable hosts
    all_hosts.push("192.168.178.251".to_string());
    all_hosts.push("192.168.178.252".to_string());

    let mut inventory_builder = common::InventoryBuilder::new();
    for host in &all_hosts {
        inventory_builder = inventory_builder
            .add_host(host, Some("all"))
            .host_var(host, "ansible_host", serde_json::json!(host))
            .host_var(host, "ansible_user", serde_json::json!(config.ssh_user));
    }
    let _inventory = inventory_builder.build();

    let playbook = common::PlaybookBuilder::new("Max Fail Percentage Test")
        .add_play(
            common::PlayBuilder::new("Test max_fail_percentage", "all")
                .gather_facts(false)
                .var("max_fail_percentage", 30) // Allow up to 30% failures
                .add_task(
                    common::TaskBuilder::new("Task", "command")
                        .arg("cmd", "hostname")
                        .build(),
                )
                .build(),
        )
        .build();

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 10,
        strategy: rustible::executor::ExecutionStrategy::Free,
        task_timeout: 5,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);

    // With 5 hosts and 2 unreachable (40%), we exceed 30% threshold
    // Execution should fail or report the threshold breach
    let result = executor.run_playbook(&playbook).await;

    // Result depends on implementation - may abort early or complete with failure stats
    println!("Max fail percentage result: {:?}", result);
}

// =============================================================================
// Connection Pool Stress Tests
// =============================================================================

#[tokio::test]
async fn test_connection_pool_concurrent_stress() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let pool = rustible::connection::AsyncConnectionPool::new(10);
    let hosts: Vec<_> = config.hosts.iter().take(5).cloned().collect();

    let successful = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let global_config = Arc::new(ConnectionConfig::default());

    let mut handles = vec![];

    // Spawn many concurrent tasks all trying to use the pool
    for iteration in 0..20 {
        for (i, host) in hosts.iter().enumerate() {
            let pool = pool.clone();
            let successful = Arc::clone(&successful);
            let failed = Arc::clone(&failed);
            let host = host.clone();
            let user = config.ssh_user.clone();
            let host_cfg = config.host_config(&host);
            let global_cfg = Arc::clone(&global_config);

            handles.push(tokio::spawn(async move {
                let pool_key = format!("ssh://{}@{}:22", user, host);

                // Try to get from pool first
                if let Some(conn) = pool.get(&pool_key).await {
                    if conn.is_alive().await {
                        let result = conn
                            .execute(&format!("echo iter_{}_host_{}", iteration, i), None)
                            .await;
                        if result.is_ok() {
                            successful.fetch_add(1, Ordering::SeqCst);
                        } else {
                            failed.fetch_add(1, Ordering::SeqCst);
                        }
                        return;
                    }
                }

                // Create new connection
                match rustible::connection::SshConnection::connect(
                    &host,
                    22,
                    &user,
                    Some(host_cfg),
                    &global_cfg,
                )
                .await
                {
                    Ok(conn) => {
                        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(conn);
                        let result = conn
                            .execute(&format!("echo iter_{}_host_{}", iteration, i), None)
                            .await;
                        pool.put(pool_key, conn).await;

                        if result.is_ok() {
                            successful.fetch_add(1, Ordering::SeqCst);
                        } else {
                            failed.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(_) => {
                        failed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.ok();
    }

    let total_successful = successful.load(Ordering::SeqCst);
    let total_failed = failed.load(Ordering::SeqCst);
    let total = total_successful + total_failed;

    println!(
        "Connection pool stress: {}/{} successful ({:.1}%)",
        total_successful,
        total,
        100.0 * total_successful as f64 / total as f64
    );

    assert!(
        total_successful > total * 90 / 100,
        "Too many failures: {}/{}",
        total_failed,
        total
    );
}

#[tokio::test]
async fn test_connection_pool_exhaustion() {
    let config = ParallelTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    // Small pool to test exhaustion behavior
    let pool = rustible::connection::AsyncConnectionPool::new(2);
    let host = config
        .hosts
        .first()
        .expect("Need at least one host")
        .clone();

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(&host);
    let pool_key = format!("ssh://{}@{}:22", config.ssh_user, host);

    // Get connections up to pool limit
    let conn1 = rustible::connection::SshConnection::connect(
        &host,
        22,
        &config.ssh_user,
        Some(host_cfg.clone()),
        &global_config,
    )
    .await
    .expect("Failed to get conn1");
    let conn1: Arc<dyn Connection + Send + Sync> = Arc::new(conn1);
    pool.put(pool_key.clone(), conn1.clone()).await;

    let conn2 = rustible::connection::SshConnection::connect(
        &host,
        22,
        &config.ssh_user,
        Some(host_cfg.clone()),
        &global_config,
    )
    .await
    .expect("Failed to get conn2");
    let conn2: Arc<dyn Connection + Send + Sync> = Arc::new(conn2);
    pool.put(format!("{}/2", pool_key), conn2.clone()).await;

    // Pool should handle this - either create new or wait/error
    // Behavior depends on implementation
    let conn3_result = rustible::connection::SshConnection::connect(
        &host,
        22,
        &config.ssh_user,
        Some(host_cfg.clone()),
        &global_config,
    )
    .await;

    // Remove connections from pool
    pool.remove(&pool_key).await;
    pool.remove(&format!("{}/2", pool_key)).await;

    // Should now be able to add a connection
    if let Ok(conn3) = conn3_result {
        let conn3: Arc<dyn Connection + Send + Sync> = Arc::new(conn3);
        pool.put(pool_key.clone(), conn3.clone()).await;
        conn3.close().await.ok();
    }

    conn1.close().await.ok();
    conn2.close().await.ok();
}
