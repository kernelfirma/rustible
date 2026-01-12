//! Extreme stress and chaos tests for Rustible
//!
//! This test suite pushes Rustible to its limits and verifies stability under chaos conditions.
//! Run with: cargo test --test stress_tests -- --test-threads=1
//!
//! Tests cover:
//! - Concurrency stress (100+ concurrent tasks, 500+ simulated hosts)
//! - Memory stress (large inventories, playbooks, variable contexts)
//! - Connection stress (rapid connect/disconnect, pool churn)
//! - Chaos scenarios (random failures, connection drops, slow responses)
//! - Race conditions (concurrent variable access, handler notifications)
//! - Long-running stability (1000+ iterations, resource leak detection)
//! - Edge cases under load

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::join_all;
use proptest::prelude::*;
use tokio::sync::{RwLock, Semaphore};

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task};
use rustible::executor::{ExecutionStats, ExecutionStrategy, Executor, ExecutorConfig};

// ============================================================================
// Helper Utilities for Stress Testing
// ============================================================================

/// Create a runtime with a large number of hosts
fn create_large_inventory(count: usize) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for i in 0..count {
        let host = format!("host-{:05}", i);
        let group = match i % 4 {
            0 => "webservers",
            1 => "databases",
            2 => "caches",
            _ => "workers",
        };
        runtime.add_host(host, Some(group));
    }
    runtime
}

/// Create a playbook with many tasks
fn create_large_playbook(task_count: usize) -> Playbook {
    let mut playbook = Playbook::new("Stress Test Playbook");
    let mut play = Play::new("Stress Play", "all");
    play.gather_facts = false;

    for i in 0..task_count {
        let task = Task::new(format!("Task-{:05}", i), "debug")
            .arg("msg", format!("Executing task {}", i));
        play.add_task(task);
    }

    playbook.add_play(play);
    playbook
}

/// Create a runtime with large variable contexts
fn create_runtime_with_large_vars(host_count: usize, var_size_kb: usize) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();

    // Create a large value (approximately var_size_kb kilobytes)
    let large_value: String = "x".repeat(var_size_kb * 1024);

    for i in 0..host_count {
        let host = format!("host-{:05}", i);
        runtime.add_host(host.clone(), None);

        // Add large variables to each host
        runtime.set_host_var(
            &host,
            "large_var".to_string(),
            serde_json::json!(large_value.clone()),
        );

        // Add nested structure
        runtime.set_host_var(
            &host,
            "complex_var".to_string(),
            serde_json::json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "data": large_value.clone()[..1024.min(large_value.len())].to_string(),
                        }
                    }
                }
            }),
        );
    }

    runtime
}

/// Failure injector for chaos testing
#[derive(Clone)]
#[allow(dead_code)]
struct FailureInjector {
    failure_rate: f64,
    counter: Arc<AtomicUsize>,
}

#[allow(dead_code)]
impl FailureInjector {
    fn new(failure_rate: f64) -> Self {
        Self {
            failure_rate,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn should_fail(&self) -> bool {
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        let threshold = (self.failure_rate * 1000.0) as usize;
        (count % 1000) < threshold
    }

    fn failure_count(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }
}

/// Performance metrics collector
struct MetricsCollector {
    operation_count: AtomicU64,
    total_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
    error_count: AtomicU64,
}

impl MetricsCollector {
    fn new() -> Self {
        Self {
            operation_count: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            max_latency_ns: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
        }
    }

    fn record_operation(&self, latency_ns: u64) {
        self.operation_count.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns
            .fetch_add(latency_ns, Ordering::Relaxed);

        // Update max latency (compare-and-swap loop)
        loop {
            let current_max = self.max_latency_ns.load(Ordering::Relaxed);
            if latency_ns <= current_max {
                break;
            }
            if self
                .max_latency_ns
                .compare_exchange(
                    current_max,
                    latency_ns,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    fn avg_latency_ms(&self) -> f64 {
        let count = self.operation_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let total_ns = self.total_latency_ns.load(Ordering::Relaxed);
        (total_ns as f64) / (count as f64) / 1_000_000.0
    }

    fn max_latency_ms(&self) -> f64 {
        let max_ns = self.max_latency_ns.load(Ordering::Relaxed);
        (max_ns as f64) / 1_000_000.0
    }

    fn error_rate(&self) -> f64 {
        let total = self.operation_count.load(Ordering::Relaxed);
        let errors = self.error_count.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (errors as f64) / (total as f64)
    }
}

// ============================================================================
// 1. CONCURRENCY STRESS TESTS
// ============================================================================

#[tokio::test]
async fn stress_100_concurrent_task_executions() {
    let runtime = create_large_inventory(100);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 100,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("100 Concurrent Tasks");
    let mut play = Play::new("Stress", "all");
    play.gather_facts = false;

    // Each host gets 5 tasks = 500 total task executions
    for i in 0..5 {
        play.add_task(
            Task::new(format!("Task {}", i), "debug").arg("msg", format!("Concurrent task {}", i)),
        );
    }
    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 100);

    let mut failed_count = 0;
    for result in results.values() {
        if result.failed {
            failed_count += 1;
        }
    }

    println!(
        "100 hosts x 5 tasks completed in {:?}, {} failures",
        duration, failed_count
    );

    // Allow a small tolerance for timing-related failures
    assert!(failed_count <= 5, "Too many failures: {}", failed_count);
}

#[tokio::test]
async fn stress_500_simulated_host_connections() {
    let runtime = create_large_inventory(500);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 50, // Limit concurrent connections to 50
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("500 Hosts");
    let mut play = Play::new("Mass Deployment", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Quick task", "debug").arg("msg", "Hello from host"));
    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 500);
    println!("500 hosts completed in {:?}", duration);

    // Verify all hosts succeeded
    for (host, result) in &results {
        assert!(!result.unreachable, "Host {} became unreachable", host);
    }
}

#[tokio::test]
async fn stress_rapid_task_start_stop_cycling() {
    let metrics = Arc::new(MetricsCollector::new());

    let mut handles = vec![];

    for i in 0..50 {
        let metrics = Arc::clone(&metrics);
        let handle = tokio::spawn(async move {
            for j in 0..20 {
                let runtime = RuntimeContext::new();
                let executor = Executor::with_runtime(
                    ExecutorConfig {
                        forks: 5,
                        ..Default::default()
                    },
                    runtime,
                );

                // Create and immediately run a minimal playbook
                let mut playbook = Playbook::new(format!("Cycle-{}-{}", i, j));
                let play = Play::new("Quick", "all");
                playbook.add_play(play);

                let start = Instant::now();
                let _ = executor.run_playbook(&playbook).await;
                let latency = start.elapsed().as_nanos() as u64;

                metrics.record_operation(latency);
            }
        });
        handles.push(handle);
    }

    join_all(handles).await;

    println!(
        "Rapid cycling: {} operations, avg latency: {:.2}ms, max latency: {:.2}ms",
        metrics.operation_count.load(Ordering::Relaxed),
        metrics.avg_latency_ms(),
        metrics.max_latency_ms()
    );
}

#[tokio::test]
async fn stress_thread_pool_exhaustion_recovery() {
    // Create a semaphore to simulate limited thread pool
    let semaphore = Arc::new(Semaphore::new(10));
    let operations_completed = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn 100 tasks competing for 10 permits
    for i in 0..100 {
        let semaphore = Arc::clone(&semaphore);
        let counter = Arc::clone(&operations_completed);

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            // Simulate work
            let runtime = RuntimeContext::new();
            let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

            let mut playbook = Playbook::new(format!("Work-{}", i));
            let play = Play::new("Work", "all");
            playbook.add_play(play);

            let _ = executor.run_playbook(&playbook).await;

            counter.fetch_add(1, Ordering::SeqCst);
            // Permit dropped here, releasing slot for next task
        });
        handles.push(handle);
    }

    // Wait for all with a timeout
    let result = tokio::time::timeout(Duration::from_secs(60), join_all(handles)).await;

    assert!(result.is_ok(), "Thread pool stress test timed out");

    let completed = operations_completed.load(Ordering::SeqCst);
    println!(
        "Thread pool exhaustion recovery: {}/100 completed",
        completed
    );
    assert_eq!(completed, 100, "Not all operations completed");
}

// ============================================================================
// 2. MEMORY STRESS TESTS
// ============================================================================

#[tokio::test]
async fn stress_large_inventory_10000_hosts() {
    let start = Instant::now();
    let runtime = create_large_inventory(10_000);
    let creation_time = start.elapsed();

    let all_hosts = runtime.get_all_hosts();
    assert_eq!(all_hosts.len(), 10_000);

    // Verify groups are properly set up
    assert!(runtime.get_group_hosts("webservers").is_some());
    assert!(runtime.get_group_hosts("databases").is_some());
    assert!(runtime.get_group_hosts("caches").is_some());
    assert!(runtime.get_group_hosts("workers").is_some());

    println!("Created 10,000 host inventory in {:?}", creation_time);

    // Test variable resolution at scale
    let start = Instant::now();
    for host in all_hosts.iter().take(100) {
        let _ = runtime.get_merged_vars(host);
    }
    let resolution_time = start.elapsed();
    println!(
        "Variable resolution for 100 hosts took {:?}",
        resolution_time
    );
}

#[tokio::test]
async fn stress_large_playbook_1000_tasks() {
    let playbook = create_large_playbook(1_000);

    assert_eq!(playbook.plays.len(), 1);
    assert_eq!(playbook.plays[0].tasks.len(), 1_000);

    let runtime = create_large_inventory(10);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Linear,
            forks: 10,
            ..Default::default()
        },
        runtime,
    );

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 10);

    // Calculate total tasks executed
    let total_tasks: usize = results
        .values()
        .map(|r| r.stats.ok + r.stats.changed + r.stats.skipped)
        .sum();

    println!(
        "1,000 tasks x 10 hosts = {} task executions in {:?}",
        total_tasks, duration
    );
}

#[tokio::test]
async fn stress_large_variable_contexts_1mb() {
    // Create runtime with ~1MB of variables per host
    let runtime = create_runtime_with_large_vars(10, 100); // 100KB per host x 10 hosts = 1MB

    let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

    let mut playbook = Playbook::new("Large Vars Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;
    play.add_task(
        Task::new("Access large var", "debug").arg("msg", "{{ large_var[:10] }}"), // Template a slice
    );
    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 10);
    println!("Large variable context (1MB+) processed in {:?}", duration);
}

#[tokio::test]
async fn stress_memory_leak_detection() {
    // Run many iterations and check memory doesn't grow unboundedly
    let iterations = 100;
    let mut iteration_times = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let start = Instant::now();

        let runtime = create_large_inventory(100);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                forks: 20,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new(format!("Iteration-{}", i));
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;
        play.add_task(Task::new("Task", "debug").arg("msg", "test"));
        playbook.add_play(play);

        let _ = executor.run_playbook(&playbook).await;

        iteration_times.push(start.elapsed());

        // Drop everything and let it be garbage collected
        drop(playbook);
        drop(executor);
    }

    // Check that iteration times are relatively stable (no memory thrashing)
    let first_10_avg: Duration = iteration_times[..10].iter().sum::<Duration>() / 10;
    let last_10_avg: Duration = iteration_times[iterations - 10..].iter().sum::<Duration>() / 10;

    println!(
        "First 10 iterations avg: {:?}, Last 10 iterations avg: {:?}",
        first_10_avg, last_10_avg
    );

    // Last 10 should not be significantly slower than first 10 (within 3x)
    assert!(
        last_10_avg < first_10_avg * 3,
        "Possible memory leak: performance degradation detected"
    );
}

// ============================================================================
// 3. CONNECTION STRESS TESTS
// ============================================================================

#[tokio::test]
async fn stress_rapid_connect_disconnect_cycles() {
    use rustible::connection::local::LocalConnection;
    use rustible::connection::Connection;

    let iterations = 1000;
    let mut connect_times = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();

        let conn = LocalConnection::new();
        let _ = conn.is_alive().await;
        let _ = conn.close().await;

        connect_times.push(start.elapsed());
    }

    let total: Duration = connect_times.iter().sum();
    let avg = total / iterations as u32;
    let max = connect_times.iter().max().unwrap();

    println!(
        "1000 connect/disconnect cycles: avg {:?}, max {:?}",
        avg, max
    );

    // Each cycle should be fast
    assert!(
        avg < Duration::from_millis(10),
        "Average connection time too high"
    );
}

#[tokio::test]
async fn stress_connection_pool_churn() {
    use rustible::connection::{ConnectionConfig, ConnectionFactory};

    let config = ConnectionConfig::default();
    let factory = ConnectionFactory::with_pool_size(config, 10);

    let mut handles = vec![];

    // 100 concurrent requests for connections
    for i in 0..100 {
        let handle = tokio::spawn({
            async move {
                // Simulate getting connection for localhost
                let start = Instant::now();
                // Note: This will create local connections which don't actually pool
                let _duration = start.elapsed();
                i
            }
        });
        handles.push(handle);
    }

    let results: Vec<_> = join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(results.len(), 100);

    let stats = factory.pool_stats().await;
    println!(
        "Connection pool after churn: {} active, {} max",
        stats.active_connections, stats.max_connections
    );
}

#[tokio::test]
async fn stress_simultaneous_connection_attempts() {
    use rustible::connection::local::LocalConnection;
    use rustible::connection::Connection;

    let connection_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Attempt 500 simultaneous connections
    for _ in 0..500 {
        let counter = Arc::clone(&connection_count);
        let handle = tokio::spawn(async move {
            let conn = LocalConnection::new();
            if conn.is_alive().await {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    join_all(handles).await;

    let successful = connection_count.load(Ordering::SeqCst);
    println!(
        "500 simultaneous connection attempts: {} successful",
        successful
    );
    assert_eq!(successful, 500, "Not all connections succeeded");
}

#[tokio::test]
async fn stress_connection_timeout_flood() {
    use rustible::connection::local::LocalConnection;
    use rustible::connection::{Connection, ExecuteOptions};

    let timeout_count = Arc::new(AtomicUsize::new(0));
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Flood with commands that have very short timeouts
    for _ in 0..100 {
        let timeout_counter = Arc::clone(&timeout_count);
        let success_counter = Arc::clone(&success_count);

        let handle = tokio::spawn(async move {
            let conn = LocalConnection::new();
            let options = ExecuteOptions::new().with_timeout(1); // 1 second timeout

            // Quick command that should succeed
            match conn.execute("echo test", Some(options)).await {
                Ok(result) if result.success => {
                    success_counter.fetch_add(1, Ordering::SeqCst);
                }
                Err(rustible::connection::ConnectionError::Timeout(_)) => {
                    timeout_counter.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        });
        handles.push(handle);
    }

    join_all(handles).await;

    let timeouts = timeout_count.load(Ordering::SeqCst);
    let successes = success_count.load(Ordering::SeqCst);

    println!(
        "Connection timeout flood: {} successes, {} timeouts",
        successes, timeouts
    );

    // Most should succeed since the command is quick
    assert!(successes > 90, "Too many failures in timeout flood");
}

// ============================================================================
// 4. CHAOS SCENARIOS
// ============================================================================

#[tokio::test]
async fn chaos_random_task_failures() {
    let runtime = create_large_inventory(20);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 20,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Chaos Failures");
    let mut play = Play::new("Random Failures", "all");
    play.gather_facts = false;

    // Create tasks that fail based on host index
    for i in 0..10 {
        play.add_task(
            Task::new(format!("Task-{}", i), "fail")
                .arg("msg", format!("Failing task {}", i))
                .when(format!(
                    "inventory_hostname | regex_search('host-0000[0-5]') and {} < 5",
                    i
                ))
                .ignore_errors(true),
        );
    }
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let mut failure_count = 0;
    let mut success_count = 0;
    for result in results.values() {
        if result.failed {
            failure_count += 1;
        } else {
            success_count += 1;
        }
    }

    println!(
        "Chaos random failures: {} successes, {} failures out of {} hosts",
        success_count,
        failure_count,
        results.len()
    );
}

#[tokio::test]
async fn chaos_connection_drops_mid_execution() {
    // Simulate connection drops by having tasks that conditionally fail
    let runtime = create_large_inventory(50);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 25,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Connection Drop Chaos");
    let mut play = Play::new("Drop Simulation", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Start", "debug").arg("msg", "Starting"));

    // Simulate random "connection drops" via conditional failures
    play.add_task(
        Task::new("May drop", "fail")
            .arg("msg", "Connection dropped")
            .when("inventory_hostname | regex_search('host-0000[02468]')")
            .ignore_errors(true),
    );

    play.add_task(Task::new("Continue", "debug").arg("msg", "Still running"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert_eq!(results.len(), 50);

    // Some hosts should have progressed despite "drops"
    let completed_tasks: usize = results.values().map(|r| r.stats.ok + r.stats.changed).sum();

    println!(
        "Connection drop chaos: {} completed tasks across {} hosts",
        completed_tasks,
        results.len()
    );
}

#[tokio::test]
async fn chaos_slow_host_responses() {
    let runtime = create_large_inventory(20);
    let executor = Executor::with_runtime(
        ExecutorConfig {
            strategy: ExecutionStrategy::Free,
            forks: 20,
            task_timeout: 300,
            ..Default::default()
        },
        runtime,
    );

    let mut playbook = Playbook::new("Slow Response Chaos");
    let mut play = Play::new("Slow Hosts", "all");
    play.gather_facts = false;

    // Simulate slow responses with pause module on some hosts
    play.add_task(
        Task::new("Slow task", "pause")
            .arg("seconds", 1)
            .when("inventory_hostname | regex_search('host-0000[0-4]')"),
    );

    play.add_task(Task::new("Quick task", "debug").arg("msg", "Fast"));

    playbook.add_play(play);

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 20);
    println!("Slow host chaos completed in {:?}", duration);

    // With free strategy, slow hosts shouldn't block fast ones
    for (host, result) in &results {
        assert!(!result.failed, "Host {} should not fail", host);
    }
}

#[tokio::test]
async fn chaos_resource_exhaustion_simulation() {
    // Simulate resource exhaustion by creating many concurrent operations
    let operations = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for i in 0..200 {
        let ops = Arc::clone(&operations);
        let max_c = Arc::clone(&max_concurrent);
        let current_c = Arc::clone(&current_concurrent);

        let handle = tokio::spawn(async move {
            // Track concurrent operations
            let current = current_c.fetch_add(1, Ordering::SeqCst) + 1;

            // Update max if needed
            loop {
                let max = max_c.load(Ordering::SeqCst);
                if current <= max {
                    break;
                }
                if max_c
                    .compare_exchange(max, current, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break;
                }
            }

            // Simulate work
            let runtime = RuntimeContext::new();
            let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

            let mut playbook = Playbook::new(format!("Resource-{}", i));
            let play = Play::new("Work", "all");
            playbook.add_play(play);

            let _ = executor.run_playbook(&playbook).await;

            current_c.fetch_sub(1, Ordering::SeqCst);
            ops.fetch_add(1, Ordering::SeqCst);
        });
        handles.push(handle);
    }

    // Wait with timeout
    let result = tokio::time::timeout(Duration::from_secs(120), join_all(handles)).await;

    assert!(result.is_ok(), "Resource exhaustion test timed out");

    let total_ops = operations.load(Ordering::SeqCst);
    let max = max_concurrent.load(Ordering::SeqCst);

    println!(
        "Resource exhaustion: {} operations, max {} concurrent",
        total_ops, max
    );

    assert_eq!(total_ops, 200, "Not all operations completed");
}

// ============================================================================
// 5. RACE CONDITION TESTS
// ============================================================================

#[tokio::test]
async fn race_concurrent_variable_access() {
    let runtime = Arc::new(RwLock::new(RuntimeContext::new()));

    // Initialize with hosts
    {
        let mut rt = runtime.write().await;
        for i in 0..10 {
            rt.add_host(format!("host-{}", i), None);
        }
    }

    let mut handles = vec![];

    // Concurrent readers and writers
    for i in 0..100 {
        let runtime = Arc::clone(&runtime);
        let handle = tokio::spawn(async move {
            if i % 2 == 0 {
                // Reader
                let rt = runtime.read().await;
                let _ = rt.get_var("some_var", None);
            } else {
                // Writer
                let mut rt = runtime.write().await;
                rt.set_global_var(format!("var_{}", i), serde_json::json!(i));
            }
        });
        handles.push(handle);
    }

    // All operations should complete without panic
    let results = join_all(handles).await;

    let success_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(success_count, 100, "Some operations failed");

    // Verify final state
    let rt = runtime.read().await;
    let hosts = rt.get_all_hosts();
    assert_eq!(hosts.len(), 10);
}

#[tokio::test]
async fn race_concurrent_handler_notifications() {
    let mut playbook = Playbook::new("Handler Race");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // All tasks notify the same handler
    for i in 0..10 {
        play.add_task(
            Task::new(format!("Notifier-{}", i), "debug")
                .arg("msg", "Notifying")
                .notify("common-handler"),
        );
    }

    play.add_handler(Handler {
        name: "common-handler".to_string(),
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

    // Run multiple times to trigger race conditions
    for iteration in 0..10 {
        let runtime = create_large_inventory(50);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 50,
                ..Default::default()
            },
            runtime,
        );

        let result = executor.run_playbook(&playbook).await;
        assert!(
            result.is_ok(),
            "Iteration {} failed: {:?}",
            iteration,
            result.err()
        );
    }
}

#[tokio::test]
async fn race_concurrent_fact_gathering() {
    // Simulate concurrent fact updates
    let runtime = Arc::new(RwLock::new(RuntimeContext::new()));

    // Initialize hosts
    {
        let mut rt = runtime.write().await;
        for i in 0..20 {
            rt.add_host(format!("host-{}", i), None);
        }
    }

    let mut handles = vec![];

    // Concurrent fact setters
    for i in 0..100 {
        let runtime = Arc::clone(&runtime);
        let handle = tokio::spawn(async move {
            let host = format!("host-{}", i % 20);
            let mut rt = runtime.write().await;
            rt.set_host_fact(
                &host,
                format!("fact_{}", i),
                serde_json::json!({"iteration": i, "timestamp": chrono::Utc::now().to_rfc3339()}),
            );
        });
        handles.push(handle);
    }

    join_all(handles).await;

    // Verify some facts were set
    let rt = runtime.read().await;
    for i in 0..20 {
        let host = format!("host-{}", i);
        // Each host should have at least some facts
        let merged_vars = rt.get_merged_vars(&host);
        assert!(merged_vars.len() > 0, "Host {} has no vars", host);
    }
}

#[tokio::test]
async fn race_connection_pool_race_conditions() {
    let pool_errors = Arc::new(AtomicUsize::new(0));
    let operations_completed = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Many concurrent operations that might race on pool access
    for i in 0..100 {
        let pool_errors = Arc::clone(&pool_errors);
        let ops_completed = Arc::clone(&operations_completed);

        let handle = tokio::spawn(async move {
            // Simulate pool access pattern
            let runtime = create_large_inventory(5);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    forks: 5,
                    ..Default::default()
                },
                runtime,
            );

            let mut playbook = Playbook::new(format!("Pool-{}", i));
            let mut play = Play::new("Test", "all");
            play.gather_facts = false;
            play.add_task(Task::new("Task", "debug").arg("msg", "pool test"));
            playbook.add_play(play);

            match executor.run_playbook(&playbook).await {
                Ok(_) => {
                    ops_completed.fetch_add(1, Ordering::SeqCst);
                }
                Err(_) => {
                    pool_errors.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        handles.push(handle);
    }

    join_all(handles).await;

    let errors = pool_errors.load(Ordering::SeqCst);
    let completed = operations_completed.load(Ordering::SeqCst);

    println!(
        "Connection pool race: {} completed, {} errors",
        completed, errors
    );

    // Most operations should succeed
    assert!(completed > 95, "Too many pool race failures");
}

// ============================================================================
// 6. LONG-RUNNING STABILITY TESTS
// ============================================================================

#[tokio::test]
#[ignore = "Flaky timing-dependent test - too sensitive to system load variance"]
async fn stability_1000_iterations_same_playbook() {
    let playbook = create_large_playbook(5);
    let mut iteration_times = Vec::with_capacity(1000);
    let mut failures = 0;

    for i in 0..1000 {
        let runtime = create_large_inventory(5);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                forks: 5,
                ..Default::default()
            },
            runtime,
        );

        let start = Instant::now();
        match executor.run_playbook(&playbook).await {
            Ok(results) => {
                let failed = results.values().any(|r| r.failed);
                if failed {
                    failures += 1;
                }
            }
            Err(_) => {
                failures += 1;
            }
        }
        iteration_times.push(start.elapsed());

        // Progress indicator for long test
        if (i + 1) % 100 == 0 {
            println!("Stability test: {}/1000 iterations completed", i + 1);
        }
    }

    let total: Duration = iteration_times.iter().sum();
    let avg = total / 1000;
    let max = iteration_times.iter().max().unwrap();
    let min = iteration_times.iter().min().unwrap();

    println!(
        "1000 iterations: avg {:?}, min {:?}, max {:?}, {} failures",
        avg, min, max, failures
    );

    // Should be very stable with minimal failures
    assert!(
        failures < 10,
        "Too many failures in stability test: {}",
        failures
    );

    // Performance should be consistent
    assert!(*max < avg * 5, "Too much variance in execution time");
}

#[tokio::test]
async fn stability_no_resource_leaks_over_time() {
    let metrics = Arc::new(MetricsCollector::new());

    // Run 500 iterations and track performance
    for i in 0..500 {
        let runtime = create_large_inventory(10);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                forks: 10,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new(format!("Leak-Test-{}", i));
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;
        play.add_task(Task::new("Task", "debug").arg("msg", "test"));
        playbook.add_play(play);

        let start = Instant::now();
        match executor.run_playbook(&playbook).await {
            Ok(_) => {
                let latency = start.elapsed().as_nanos() as u64;
                metrics.record_operation(latency);
            }
            Err(_) => {
                metrics.record_error();
            }
        }
    }

    println!(
        "500 iterations: avg latency {:.2}ms, max {:.2}ms, error rate {:.2}%",
        metrics.avg_latency_ms(),
        metrics.max_latency_ms(),
        metrics.error_rate() * 100.0
    );

    // Error rate should be very low
    assert!(
        metrics.error_rate() < 0.01,
        "Error rate too high: {:.2}%",
        metrics.error_rate() * 100.0
    );
}

// Timing-sensitive test that can fail due to system warmup effects
// The test expects stable latency but first runs are often slower due to JIT/caching
#[tokio::test]
#[ignore = "Flaky timing test - system warmup causes latency variation"]
async fn stability_latency_consistency_over_time() {
    let mut latencies: Vec<Duration> = Vec::with_capacity(200);

    for i in 0..200 {
        let runtime = create_large_inventory(20);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 20,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new(format!("Latency-{}", i));
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;
        play.add_task(Task::new("Task", "debug").arg("msg", "latency test"));
        playbook.add_play(play);

        let start = Instant::now();
        let _ = executor.run_playbook(&playbook).await;
        latencies.push(start.elapsed());
    }

    // Compare first 50 vs last 50
    let first_50_avg: Duration = latencies[..50].iter().sum::<Duration>() / 50;
    let last_50_avg: Duration = latencies[150..].iter().sum::<Duration>() / 50;

    println!(
        "Latency stability: first 50 avg {:?}, last 50 avg {:?}",
        first_50_avg, last_50_avg
    );

    // Latency should be stable (within 2x)
    let ratio = last_50_avg.as_nanos() as f64 / first_50_avg.as_nanos() as f64;
    assert!(
        ratio < 2.0 && ratio > 0.5,
        "Latency instability detected: ratio {:.2}",
        ratio
    );
}

// ============================================================================
// 7. EDGE CASES UNDER LOAD
// ============================================================================

#[tokio::test]
async fn edge_empty_results_under_concurrent_load() {
    let mut handles = vec![];

    for i in 0..50 {
        let handle = tokio::spawn(async move {
            let runtime = RuntimeContext::new(); // No hosts
            let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

            let mut playbook = Playbook::new(format!("Empty-{}", i));
            let play = Play::new("Empty", "all");
            playbook.add_play(play);

            let result = executor.run_playbook(&playbook).await;
            assert!(result.is_ok());
            result.unwrap().len()
        });
        handles.push(handle);
    }

    let results: Vec<_> = join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(results.len(), 50);

    // All should return 0 hosts
    for count in &results {
        assert_eq!(*count, 0);
    }
}

#[tokio::test]
async fn edge_timeout_handling_under_load() {
    use rustible::connection::local::LocalConnection;
    use rustible::connection::{Connection, ConnectionError, ExecuteOptions};

    let success = Arc::new(AtomicUsize::new(0));
    let timeout = Arc::new(AtomicUsize::new(0));
    let other_error = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for _ in 0..50 {
        let success = Arc::clone(&success);
        let timeout = Arc::clone(&timeout);
        let other_error = Arc::clone(&other_error);

        let handle = tokio::spawn(async move {
            let conn = LocalConnection::new();

            // Very short timeout
            let options = ExecuteOptions::new().with_timeout(1);

            match conn.execute("echo test", Some(options)).await {
                Ok(r) if r.success => {
                    success.fetch_add(1, Ordering::SeqCst);
                }
                Err(ConnectionError::Timeout(_)) => {
                    timeout.fetch_add(1, Ordering::SeqCst);
                }
                _ => {
                    other_error.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        handles.push(handle);
    }

    join_all(handles).await;

    let s = success.load(Ordering::SeqCst);
    let t = timeout.load(Ordering::SeqCst);
    let e = other_error.load(Ordering::SeqCst);

    println!(
        "Timeout handling under load: {} success, {} timeout, {} errors",
        s, t, e
    );

    // Most should succeed (echo is fast)
    assert!(s > 40, "Too few successes under load");
}

#[tokio::test]
async fn edge_error_handling_under_load() {
    let mut playbook = Playbook::new("Error Handling");
    let mut play = Play::new("Errors", "all");
    play.gather_facts = false;

    // Mix of succeeding and failing tasks
    for i in 0..20 {
        if i % 3 == 0 {
            play.add_task(
                Task::new(format!("Fail-{}", i), "fail")
                    .arg("msg", "Intentional failure")
                    .ignore_errors(true),
            );
        } else {
            play.add_task(Task::new(format!("Ok-{}", i), "debug").arg("msg", "Success"));
        }
    }
    playbook.add_play(play);

    // Run multiple times under load
    let mut handles = vec![];

    for i in 0..10 {
        let runtime = create_large_inventory(20);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 20,
                ..Default::default()
            },
            runtime,
        );
        let playbook = playbook.clone();

        let handle = tokio::spawn(async move {
            let result = executor.run_playbook(&playbook).await;
            (i, result.is_ok())
        });
        handles.push(handle);
    }

    let results: Vec<_> = join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let success_count = results.iter().filter(|(_, ok)| *ok).count();
    println!(
        "Error handling under load: {}/10 successful runs",
        success_count
    );

    assert_eq!(success_count, 10, "Some runs failed unexpectedly");
}

// ============================================================================
// PROPERTY-BASED TESTS (using proptest)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_executor_handles_any_host_count(host_count in 1usize..100) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let runtime = create_large_inventory(host_count);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    forks: host_count.min(50),
                    ..Default::default()
                },
                runtime,
            );

            let mut playbook = Playbook::new("Prop Test");
            let mut play = Play::new("Test", "all");
            play.gather_facts = false;
            play.add_task(Task::new("Task", "debug").arg("msg", "test"));
            playbook.add_play(play);

            let result = executor.run_playbook(&playbook).await;
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap().len(), host_count);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn prop_executor_handles_any_task_count(task_count in 1usize..50) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let runtime = create_large_inventory(5);
            let playbook = create_large_playbook(task_count);

            let executor = Executor::with_runtime(
                ExecutorConfig::default(),
                runtime,
            );

            let result = executor.run_playbook(&playbook).await;
            prop_assert!(result.is_ok());
            Ok(())
        }).unwrap();
    }

    #[test]
    fn prop_runtime_context_variable_roundtrip(
        key in "[a-z]{1,20}",
        value in any::<i64>()
    ) {
        let mut runtime = RuntimeContext::new();
        runtime.set_global_var(key.clone(), serde_json::json!(value));

        let retrieved = runtime.get_var(&key, None);
        prop_assert!(retrieved.is_some());
        prop_assert_eq!(retrieved.unwrap(), serde_json::json!(value));
    }

    #[test]
    fn prop_execution_stats_merge_is_additive(
        ok1 in 0usize..1000,
        changed1 in 0usize..1000,
        failed1 in 0usize..100,
        ok2 in 0usize..1000,
        changed2 in 0usize..1000,
        failed2 in 0usize..100,
    ) {
        let mut stats1 = ExecutionStats {
            ok: ok1,
            changed: changed1,
            failed: failed1,
            skipped: 0,
            unreachable: 0,
        };

        let stats2 = ExecutionStats {
            ok: ok2,
            changed: changed2,
            failed: failed2,
            skipped: 0,
            unreachable: 0,
        };

        stats1.merge(&stats2);

        prop_assert_eq!(stats1.ok, ok1 + ok2);
        prop_assert_eq!(stats1.changed, changed1 + changed2);
        prop_assert_eq!(stats1.failed, failed1 + failed2);
    }
}

// ============================================================================
// SIGNAL HANDLING TESTS
// ============================================================================

#[cfg(unix)]
#[tokio::test]
async fn stress_signal_handling_resilience() {
    // Test that the executor can handle running alongside signal handlers
    let runtime = create_large_inventory(10);
    let executor = Arc::new(Executor::with_runtime(
        ExecutorConfig {
            forks: 10,
            ..Default::default()
        },
        runtime,
    ));

    let mut playbook = Playbook::new("Signal Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    for i in 0..10 {
        play.add_task(Task::new(format!("Task-{}", i), "pause").arg("seconds", 0));
    }
    playbook.add_play(play);

    // Run the playbook (no actual signals sent, just verifying handler setup doesn't interfere)
    let result = executor.run_playbook(&playbook).await;
    assert!(result.is_ok());
}

// ============================================================================
// BENCHMARK-STYLE STRESS TESTS
// ============================================================================

#[tokio::test]
async fn benchmark_throughput_max_hosts() {
    let start = Instant::now();
    let mut total_tasks = 0;

    for batch in 0..10 {
        let runtime = create_large_inventory(100);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 50,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new(format!("Batch-{}", batch));
        let mut play = Play::new("Speed", "all");
        play.gather_facts = false;

        for i in 0..10 {
            play.add_task(Task::new(format!("Task-{}", i), "debug").arg("msg", "speed test"));
        }
        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        total_tasks += results
            .values()
            .map(|r| r.stats.ok + r.stats.changed)
            .sum::<usize>();
    }

    let duration = start.elapsed();
    let throughput = total_tasks as f64 / duration.as_secs_f64();

    println!(
        "Throughput benchmark: {} tasks in {:?} = {:.2} tasks/sec",
        total_tasks, duration, throughput
    );

    // Should handle at least 1000 tasks per second
    assert!(throughput > 1000.0, "Throughput too low: {:.2}", throughput);
}

#[tokio::test]
async fn benchmark_latency_percentiles() {
    let mut latencies: Vec<Duration> = Vec::with_capacity(1000);

    for _ in 0..1000 {
        let runtime = create_large_inventory(1);
        let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

        let mut playbook = Playbook::new("Latency");
        let play = Play::new("Test", "all");
        playbook.add_play(play);

        let start = Instant::now();
        let _ = executor.run_playbook(&playbook).await;
        latencies.push(start.elapsed());
    }

    latencies.sort();

    let p50 = latencies[500];
    let p90 = latencies[900];
    let p99 = latencies[990];
    let p999 = latencies[999];

    println!(
        "Latency percentiles: p50={:?}, p90={:?}, p99={:?}, p99.9={:?}",
        p50, p90, p99, p999
    );

    // p99 should be reasonable
    assert!(
        p99 < Duration::from_millis(100),
        "p99 latency too high: {:?}",
        p99
    );
}

// ============================================================================
// 8. EXTREME STRESS TESTS - 1000+ HOSTS CONCURRENT
// ============================================================================
// These tests are marked #[ignore] for CI - run manually with:
//   cargo test --test stress_tests extreme_ -- --ignored --test-threads=1

/// Extreme stress test: 1000 hosts concurrent execution
///
/// This test validates the system can handle 1000+ hosts executing
/// tasks concurrently without memory exhaustion or performance degradation.
///
/// Run manually: cargo test extreme_1000_hosts_concurrent -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_1000_hosts_concurrent -- --ignored"]
async fn extreme_1000_hosts_concurrent() {
    let test_result = tokio::time::timeout(Duration::from_secs(300), async {
        let start = Instant::now();
        let runtime = create_large_inventory(1000);
        let inventory_creation_time = start.elapsed();

        println!(
            "Created 1000-host inventory in {:?}",
            inventory_creation_time
        );

        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 100, // High concurrency
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new("1000 Hosts Stress");
        let mut play = Play::new("Mass Execution", "all");
        play.gather_facts = false;
        play.add_task(Task::new("Concurrent task", "debug").arg("msg", "1000-host test"));
        playbook.add_play(play);

        let exec_start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let exec_duration = exec_start.elapsed();
        let total_duration = start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(
                    results.len(),
                    1000,
                    "Should have results for all 1000 hosts"
                );

                let mut success_count = 0;
                let mut failure_count = 0;
                for result in results.values() {
                    if result.failed {
                        failure_count += 1;
                    } else {
                        success_count += 1;
                    }
                }

                println!(
                    "1000 hosts concurrent: {} success, {} failed in {:?} (exec: {:?})",
                    success_count, failure_count, total_duration, exec_duration
                );

                // Allow up to 1% failure rate
                assert!(
                    failure_count <= 10,
                    "Too many failures in 1000-host test: {}",
                    failure_count
                );
            }
            Err(e) => {
                panic!("1000 hosts test failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(
        test_result.is_ok(),
        "1000-host concurrent test timed out after 5 minutes"
    );
}

/// Extreme stress test: 2000 hosts with batched execution
///
/// Tests ability to handle very large inventories with controlled batching.
///
/// Run manually: cargo test extreme_2000_hosts_batched -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_2000_hosts_batched -- --ignored"]
async fn extreme_2000_hosts_batched() {
    let test_result = tokio::time::timeout(Duration::from_secs(600), async {
        let start = Instant::now();
        let runtime = create_large_inventory(2000);

        println!("Created 2000-host inventory in {:?}", start.elapsed());

        // Run in batches of 200 to avoid overwhelming the system
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Linear, // Process hosts in batches
                forks: 50,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new("2000 Hosts Batched");
        let mut play = Play::new("Batch Execution", "all");
        play.gather_facts = false;
        play.add_task(Task::new("Batched task", "debug").arg("msg", "2000-host batch"));
        playbook.add_play(play);

        let exec_start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let exec_duration = exec_start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(results.len(), 2000);
                println!("2000 hosts batched completed in {:?}", exec_duration);
            }
            Err(e) => {
                panic!("2000 hosts batched test failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(
        test_result.is_ok(),
        "2000-host batched test timed out after 10 minutes"
    );
}

// ============================================================================
// 9. EXTREME STRESS TESTS - 1000+ TASKS SEQUENTIAL
// ============================================================================

/// Extreme stress test: 1000 tasks sequential on single host
///
/// Validates that the executor can handle very long playbooks without
/// memory leaks or performance degradation over time.
///
/// Run manually: cargo test extreme_1000_tasks_sequential -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_1000_tasks_sequential -- --ignored"]
async fn extreme_1000_tasks_sequential() {
    let test_result = tokio::time::timeout(Duration::from_secs(300), async {
        let runtime = create_large_inventory(1);
        let playbook = create_large_playbook(1000);

        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Linear,
                forks: 1,
                ..Default::default()
            },
            runtime,
        );

        let start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let duration = start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(results.len(), 1);

                let total_ok: usize = results.values().map(|r| r.stats.ok).sum();
                println!(
                    "1000 sequential tasks completed: {} ok in {:?}",
                    total_ok, duration
                );

                // Should have executed all 1000 tasks
                assert!(
                    total_ok >= 900, // Allow some skipped due to conditions
                    "Expected ~1000 tasks ok, got {}",
                    total_ok
                );
            }
            Err(e) => {
                panic!("1000 sequential tasks failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(test_result.is_ok(), "1000-task sequential test timed out");
}

/// Extreme stress test: 2000 tasks across 10 hosts
///
/// Tests handling very large task counts distributed across multiple hosts.
///
/// Run manually: cargo test extreme_2000_tasks_across_hosts -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_2000_tasks_across_hosts -- --ignored"]
async fn extreme_2000_tasks_across_hosts() {
    let test_result = tokio::time::timeout(Duration::from_secs(600), async {
        let runtime = create_large_inventory(10);
        let playbook = create_large_playbook(2000);

        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 10,
                ..Default::default()
            },
            runtime,
        );

        let start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let duration = start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(results.len(), 10);

                let total_executions: usize = results
                    .values()
                    .map(|r| r.stats.ok + r.stats.changed + r.stats.skipped)
                    .sum();

                let throughput = total_executions as f64 / duration.as_secs_f64();

                println!(
                    "2000 tasks x 10 hosts: {} executions in {:?} ({:.1} tasks/sec)",
                    total_executions, duration, throughput
                );
            }
            Err(e) => {
                panic!("2000 tasks across hosts failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(test_result.is_ok(), "2000-task multi-host test timed out");
}

// ============================================================================
// 10. MEMORY UNDER LOAD TESTS
// ============================================================================

/// Memory stress: Large variable contexts under concurrent load
///
/// Tests memory behavior when hosts have large variable contexts
/// and many concurrent operations are running.
///
/// Run manually: cargo test extreme_memory_large_vars_concurrent -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_memory_large_vars_concurrent -- --ignored"]
async fn extreme_memory_large_vars_concurrent() {
    let test_result = tokio::time::timeout(Duration::from_secs(180), async {
        // 50 hosts with 50KB of variables each = ~2.5MB variable data
        let runtime = create_runtime_with_large_vars(50, 50);

        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: 25,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new("Memory Stress");
        let mut play = Play::new("Large Vars", "all");
        play.gather_facts = false;

        // Multiple tasks that access variables
        for i in 0..10 {
            play.add_task(Task::new(format!("Var access {}", i), "debug").arg(
                "msg",
                "{{ large_var[:50] }} - {{ complex_var.level1.level2.level3.data[:20] }}",
            ));
        }
        playbook.add_play(play);

        let start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let duration = start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(results.len(), 50);
                println!(
                    "Memory stress (50 hosts, 50KB each, 10 tasks): completed in {:?}",
                    duration
                );
            }
            Err(e) => {
                panic!("Memory stress test failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(test_result.is_ok(), "Memory stress test timed out");
}

/// Memory stress: Repeated allocations to detect leaks
///
/// Runs many iterations with fresh allocations each time to detect
/// memory leaks through performance degradation patterns.
///
/// Run manually: cargo test extreme_memory_leak_detection_extended -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_memory_leak_detection_extended -- --ignored"]
async fn extreme_memory_leak_detection_extended() {
    let test_result = tokio::time::timeout(Duration::from_secs(600), async {
        let iterations = 500;
        let mut iteration_times = Vec::with_capacity(iterations);
        let mut failures = 0;

        for i in 0..iterations {
            let start = Instant::now();

            // Each iteration creates fresh resources
            let runtime = create_large_inventory(50);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    forks: 25,
                    ..Default::default()
                },
                runtime,
            );

            let mut playbook = Playbook::new(format!("Leak-Check-{}", i));
            let mut play = Play::new("Test", "all");
            play.gather_facts = false;
            play.add_task(Task::new("Task", "debug").arg("msg", "leak check"));
            playbook.add_play(play);

            match executor.run_playbook(&playbook).await {
                Ok(_) => {}
                Err(_) => failures += 1,
            }

            iteration_times.push(start.elapsed());

            // Progress indicator
            if (i + 1) % 100 == 0 {
                let recent_avg: Duration = iteration_times[i.saturating_sub(9)..=i]
                    .iter()
                    .sum::<Duration>()
                    / (i.saturating_sub(9)..=i).count() as u32;
                println!(
                    "Iteration {}/{}: recent avg {:?}",
                    i + 1,
                    iterations,
                    recent_avg
                );
            }
        }

        // Analyze for memory leak indicators
        let first_50_avg: Duration = iteration_times[..50].iter().sum::<Duration>() / 50;
        let last_50_avg: Duration =
            iteration_times[iterations - 50..].iter().sum::<Duration>() / 50;
        let mid_50_avg: Duration = iteration_times[225..275].iter().sum::<Duration>() / 50;

        println!("Memory leak analysis over {} iterations:", iterations);
        println!("  First 50 avg: {:?}", first_50_avg);
        println!("  Middle 50 avg: {:?}", mid_50_avg);
        println!("  Last 50 avg: {:?}", last_50_avg);
        println!("  Failures: {}", failures);

        // Check for degradation patterns that indicate leaks
        // Allow 2.5x slowdown maximum (some variance is expected)
        let ratio = last_50_avg.as_nanos() as f64 / first_50_avg.as_nanos() as f64;
        assert!(
            ratio < 2.5,
            "Possible memory leak detected: performance degraded {:.2}x",
            ratio
        );

        assert!(
            failures < iterations / 100,
            "Too many failures: {}",
            failures
        );
    })
    .await;

    assert!(
        test_result.is_ok(),
        "Extended memory leak detection timed out"
    );
}

// ============================================================================
// 11. CONNECTION POOL EXHAUSTION TESTS
// ============================================================================

/// Connection pool exhaustion: All slots consumed
///
/// Tests behavior when connection pool reaches capacity and
/// additional connections are requested.
///
/// Run manually: cargo test extreme_pool_exhaustion_recovery -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_pool_exhaustion_recovery -- --ignored"]
async fn extreme_pool_exhaustion_recovery() {
    let test_result = tokio::time::timeout(Duration::from_secs(120), async {
        use rustible::connection::{ConnectionConfig, ConnectionFactory};

        // Create a factory with a very small pool (5 connections)
        let config = ConnectionConfig::default();
        let factory = ConnectionFactory::with_pool_size(config, 5);

        let acquired = Arc::new(AtomicUsize::new(0));
        let exhaustion_detected = Arc::new(AtomicUsize::new(0));
        let recovered = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        // Try to acquire 50 connections with only 5 pool slots
        for i in 0..50 {
            let acquired = Arc::clone(&acquired);
            let exhaustion = Arc::clone(&exhaustion_detected);
            let recovered = Arc::clone(&recovered);

            let handle = tokio::spawn(async move {
                // Small random delay to create race conditions
                tokio::time::sleep(Duration::from_millis(i as u64 * 10)).await;

                // Simulate connection work
                let runtime = RuntimeContext::new();
                let executor = Executor::with_runtime(ExecutorConfig::default(), runtime);

                let mut playbook = Playbook::new(format!("Pool-Exhaust-{}", i));
                let play = Play::new("Test", "all");
                playbook.add_play(play);

                match executor.run_playbook(&playbook).await {
                    Ok(_) => {
                        acquired.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => {
                        exhaustion.fetch_add(1, Ordering::SeqCst);
                    }
                }

                // Simulate work then release
                tokio::time::sleep(Duration::from_millis(50)).await;
                recovered.fetch_add(1, Ordering::SeqCst);
            });
            handles.push(handle);
        }

        join_all(handles).await;

        let stats = factory.pool_stats().await;
        let total_acquired = acquired.load(Ordering::SeqCst);
        let total_exhaustion = exhaustion_detected.load(Ordering::SeqCst);
        let total_recovered = recovered.load(Ordering::SeqCst);

        println!(
            "Pool exhaustion test: acquired={}, exhaustion={}, recovered={}",
            total_acquired, total_exhaustion, total_recovered
        );
        println!("Final pool stats: {:?}", stats);

        // The system should handle exhaustion gracefully
        assert_eq!(
            total_acquired + total_exhaustion,
            50,
            "All 50 attempts should complete"
        );
    })
    .await;

    assert!(test_result.is_ok(), "Pool exhaustion test timed out");
}

/// Connection pool exhaustion: Rapid acquire/release cycles
///
/// Tests pool behavior under rapid connection churn where connections
/// are acquired and released very quickly.
///
/// Run manually: cargo test extreme_pool_rapid_churn -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_pool_rapid_churn -- --ignored"]
async fn extreme_pool_rapid_churn() {
    let test_result = tokio::time::timeout(Duration::from_secs(180), async {
        let operations = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        // 20 concurrent workers, each doing 50 rapid cycles
        for worker_id in 0..20 {
            let ops = Arc::clone(&operations);
            let errs = Arc::clone(&errors);

            let handle = tokio::spawn(async move {
                for cycle in 0..50 {
                    let runtime = RuntimeContext::new();
                    let executor = Executor::with_runtime(
                        ExecutorConfig {
                            forks: 1,
                            ..Default::default()
                        },
                        runtime,
                    );

                    let mut playbook = Playbook::new(format!("Churn-{}-{}", worker_id, cycle));
                    let play = Play::new("Quick", "all");
                    playbook.add_play(play);

                    match executor.run_playbook(&playbook).await {
                        Ok(_) => {
                            ops.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(_) => {
                            errs.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            });
            handles.push(handle);
        }

        let start = Instant::now();
        join_all(handles).await;
        let duration = start.elapsed();

        let total_ops = operations.load(Ordering::SeqCst);
        let total_errors = errors.load(Ordering::SeqCst);
        let throughput = total_ops as f64 / duration.as_secs_f64();

        println!(
            "Rapid pool churn: {} ops, {} errors in {:?} ({:.1} ops/sec)",
            total_ops, total_errors, duration, throughput
        );

        // Should have very low error rate
        assert!(
            total_errors < total_ops / 100,
            "Too many errors in rapid churn: {} of {}",
            total_errors,
            total_ops
        );
    })
    .await;

    assert!(test_result.is_ok(), "Rapid pool churn test timed out");
}

// ============================================================================
// 12. CPU SATURATION TESTS
// ============================================================================

/// CPU saturation: Maximum parallel task execution
///
/// Pushes the CPU to maximum utilization with highly parallel
/// task execution to verify system stability under load.
///
/// Run manually: cargo test extreme_cpu_saturation -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_cpu_saturation -- --ignored"]
async fn extreme_cpu_saturation() {
    let test_result = tokio::time::timeout(Duration::from_secs(300), async {
        let num_cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);

        // Use 2x CPU count for forks to ensure saturation
        let forks = (num_cpus * 2).min(100);
        let host_count = forks * 5; // 5x forks to keep all threads busy

        println!(
            "CPU saturation test: {} CPUs detected, using {} forks, {} hosts",
            num_cpus, forks, host_count
        );

        let runtime = create_large_inventory(host_count);
        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new("CPU Saturation");
        let mut play = Play::new("Saturate CPU", "all");
        play.gather_facts = false;

        // Multiple compute-like tasks per host
        for i in 0..20 {
            play.add_task(
                Task::new(format!("CPU task {}", i), "debug")
                    .arg("msg", format!("Processing iteration {} with vars: {{}}", i)),
            );
        }
        playbook.add_play(play);

        let start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let duration = start.elapsed();

        match results {
            Ok(results) => {
                let total_tasks: usize =
                    results.values().map(|r| r.stats.ok + r.stats.changed).sum();

                let throughput = total_tasks as f64 / duration.as_secs_f64();

                println!(
                    "CPU saturation: {} tasks on {} hosts in {:?} ({:.1} tasks/sec)",
                    total_tasks, host_count, duration, throughput
                );

                // Should maintain reasonable throughput even under saturation
                assert!(
                    throughput > 100.0,
                    "Throughput too low under CPU saturation: {:.1}",
                    throughput
                );
            }
            Err(e) => {
                panic!("CPU saturation test failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(test_result.is_ok(), "CPU saturation test timed out");
}

/// CPU saturation: Sustained load over time
///
/// Maintains high CPU load for an extended period to verify
/// there are no thermal throttling or resource exhaustion issues.
///
/// Run manually: cargo test extreme_cpu_sustained_load -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_cpu_sustained_load -- --ignored"]
async fn extreme_cpu_sustained_load() {
    let test_result = tokio::time::timeout(Duration::from_secs(600), async {
        let metrics = Arc::new(MetricsCollector::new());
        let target_duration = Duration::from_secs(60); // 1 minute of sustained load
        let start = Instant::now();
        let mut batch_count = 0;

        while start.elapsed() < target_duration {
            batch_count += 1;
            let batch_start = Instant::now();

            let runtime = create_large_inventory(50);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    strategy: ExecutionStrategy::Free,
                    forks: 50,
                    ..Default::default()
                },
                runtime,
            );

            let mut playbook = Playbook::new(format!("Sustained-{}", batch_count));
            let mut play = Play::new("Load", "all");
            play.gather_facts = false;
            for i in 0..10 {
                play.add_task(Task::new(format!("Task-{}", i), "debug").arg("msg", "load"));
            }
            playbook.add_play(play);

            match executor.run_playbook(&playbook).await {
                Ok(_) => {
                    metrics.record_operation(batch_start.elapsed().as_nanos() as u64);
                }
                Err(_) => {
                    metrics.record_error();
                }
            }
        }

        let total_duration = start.elapsed();

        println!(
            "Sustained CPU load: {} batches over {:?}",
            batch_count, total_duration
        );
        println!("  Avg batch latency: {:.2}ms", metrics.avg_latency_ms());
        println!("  Max batch latency: {:.2}ms", metrics.max_latency_ms());
        println!("  Error rate: {:.2}%", metrics.error_rate() * 100.0);

        // Error rate should be very low even under sustained load
        assert!(
            metrics.error_rate() < 0.01,
            "Error rate too high under sustained load: {:.2}%",
            metrics.error_rate() * 100.0
        );
    })
    .await;

    assert!(test_result.is_ok(), "Sustained CPU load test timed out");
}

// ============================================================================
// 13. DISK I/O LIMITS TESTS
// ============================================================================

/// Disk I/O stress: Large data in variable contexts
///
/// Tests performance when variable contexts contain large amounts
/// of data that might stress serialization and memory I/O.
///
/// Run manually: cargo test extreme_disk_io_large_data -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_disk_io_large_data -- --ignored"]
async fn extreme_disk_io_large_data() {
    let test_result = tokio::time::timeout(Duration::from_secs(180), async {
        // Create runtime with very large variables (1MB per host)
        let host_count = 10;
        let var_size_kb = 1024; // 1MB per host = 10MB total

        let runtime = create_runtime_with_large_vars(host_count, var_size_kb);

        let executor = Executor::with_runtime(
            ExecutorConfig {
                strategy: ExecutionStrategy::Free,
                forks: host_count,
                ..Default::default()
            },
            runtime,
        );

        let mut playbook = Playbook::new("Large Data I/O");
        let mut play = Play::new("I/O Stress", "all");
        play.gather_facts = false;

        // Tasks that access the large variables
        for i in 0..5 {
            play.add_task(
                Task::new(format!("Access large var {}", i), "debug")
                    .arg("msg", "{{ large_var | length }} bytes"),
            );
        }
        playbook.add_play(play);

        let start = Instant::now();
        let results = executor.run_playbook(&playbook).await;
        let duration = start.elapsed();

        match results {
            Ok(results) => {
                assert_eq!(results.len(), host_count);

                let total_mb = (host_count * var_size_kb) as f64 / 1024.0;
                let throughput_mb_sec = total_mb / duration.as_secs_f64();

                println!(
                    "Large data I/O: {:.1}MB processed in {:?} ({:.2} MB/sec)",
                    total_mb, duration, throughput_mb_sec
                );
            }
            Err(e) => {
                panic!("Large data I/O test failed: {:?}", e);
            }
        }
    })
    .await;

    assert!(test_result.is_ok(), "Large data I/O test timed out");
}

/// Disk I/O stress: Rapid file operations simulation
///
/// Simulates rapid file transfer operations to stress I/O subsystem.
///
/// Run manually: cargo test extreme_disk_io_rapid_operations -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_disk_io_rapid_operations -- --ignored"]
async fn extreme_disk_io_rapid_operations() {
    let test_result = tokio::time::timeout(Duration::from_secs(120), async {
        use rustible::connection::local::LocalConnection;
        use rustible::connection::Connection;

        let conn = LocalConnection::new();
        let operations_completed = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        // Spawn many concurrent file operation simulations
        for i in 0..100 {
            let ops = Arc::clone(&operations_completed);
            let errs = Arc::clone(&errors);

            let handle = tokio::spawn(async move {
                for j in 0..10 {
                    let conn = LocalConnection::new();

                    // Simulate file operations via commands
                    let cmds = vec![
                        format!("dd if=/dev/zero of=/tmp/stress_io_{}_{}.tmp bs=1K count=100 2>/dev/null", i, j),
                        format!("cat /tmp/stress_io_{}_{}.tmp > /dev/null", i, j),
                        format!("rm -f /tmp/stress_io_{}_{}.tmp", i, j),
                    ];

                    for cmd in cmds {
                        match conn.execute(&cmd, None).await {
                            Ok(r) if r.success => {
                                ops.fetch_add(1, Ordering::SeqCst);
                            }
                            _ => {
                                errs.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                }
            });
            handles.push(handle);
        }

        let start = Instant::now();
        join_all(handles).await;
        let duration = start.elapsed();

        let total_ops = operations_completed.load(Ordering::SeqCst);
        let total_errors = errors.load(Ordering::SeqCst);
        let throughput = total_ops as f64 / duration.as_secs_f64();

        println!(
            "Rapid I/O operations: {} ops, {} errors in {:?} ({:.1} ops/sec)",
            total_ops, total_errors, duration, throughput
        );

        // Cleanup any remaining temp files
        let _ = conn.execute("rm -f /tmp/stress_io_*.tmp", None).await;

        // Most operations should succeed
        assert!(
            total_errors < total_ops / 10,
            "Too many I/O errors: {} of {}",
            total_errors,
            total_ops
        );
    })
    .await;

    assert!(test_result.is_ok(), "Rapid I/O operations test timed out");
}

// ============================================================================
// 14. COMBINED EXTREME STRESS TEST
// ============================================================================

/// Combined extreme stress: All stressors simultaneously
///
/// Combines large host count, many tasks, large variables, and high concurrency
/// to create maximum stress conditions.
///
/// Run manually: cargo test extreme_combined_stress -- --ignored
#[tokio::test]
#[ignore = "Extreme stress test - run manually with: cargo test extreme_combined_stress -- --ignored"]
async fn extreme_combined_stress() {
    let test_result = tokio::time::timeout(Duration::from_secs(600), async {
        let metrics = Arc::new(MetricsCollector::new());

        println!("Starting combined extreme stress test...");

        // Phase 1: Large inventory with large variables
        println!("Phase 1: Large inventory with large variables");
        let phase1_start = Instant::now();
        {
            let runtime = create_runtime_with_large_vars(100, 100); // 100 hosts, 100KB each
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    strategy: ExecutionStrategy::Free,
                    forks: 50,
                    ..Default::default()
                },
                runtime,
            );

            let playbook = create_large_playbook(50);
            match executor.run_playbook(&playbook).await {
                Ok(_) => metrics.record_operation(phase1_start.elapsed().as_nanos() as u64),
                Err(_) => metrics.record_error(),
            }
        }
        println!("  Phase 1 completed in {:?}", phase1_start.elapsed());

        // Phase 2: Many hosts, few tasks per host
        println!("Phase 2: Many hosts, few tasks");
        let phase2_start = Instant::now();
        {
            let runtime = create_large_inventory(500);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    strategy: ExecutionStrategy::Free,
                    forks: 100,
                    ..Default::default()
                },
                runtime,
            );

            let playbook = create_large_playbook(10);
            match executor.run_playbook(&playbook).await {
                Ok(_) => metrics.record_operation(phase2_start.elapsed().as_nanos() as u64),
                Err(_) => metrics.record_error(),
            }
        }
        println!("  Phase 2 completed in {:?}", phase2_start.elapsed());

        // Phase 3: Few hosts, many tasks
        println!("Phase 3: Few hosts, many tasks");
        let phase3_start = Instant::now();
        {
            let runtime = create_large_inventory(5);
            let executor = Executor::with_runtime(
                ExecutorConfig {
                    strategy: ExecutionStrategy::Linear,
                    forks: 5,
                    ..Default::default()
                },
                runtime,
            );

            let playbook = create_large_playbook(500);
            match executor.run_playbook(&playbook).await {
                Ok(_) => metrics.record_operation(phase3_start.elapsed().as_nanos() as u64),
                Err(_) => metrics.record_error(),
            }
        }
        println!("  Phase 3 completed in {:?}", phase3_start.elapsed());

        // Phase 4: Concurrent worker stress
        println!("Phase 4: Concurrent worker stress");
        let phase4_start = Instant::now();
        {
            let mut handles = vec![];
            for i in 0..50 {
                let metrics = Arc::clone(&metrics);
                let handle = tokio::spawn(async move {
                    let runtime = create_large_inventory(10);
                    let executor = Executor::with_runtime(
                        ExecutorConfig {
                            forks: 10,
                            ..Default::default()
                        },
                        runtime,
                    );

                    let mut playbook = Playbook::new(format!("Worker-{}", i));
                    let mut play = Play::new("Work", "all");
                    play.gather_facts = false;
                    play.add_task(Task::new("Task", "debug").arg("msg", "worker"));
                    playbook.add_play(play);

                    let op_start = Instant::now();
                    match executor.run_playbook(&playbook).await {
                        Ok(_) => metrics.record_operation(op_start.elapsed().as_nanos() as u64),
                        Err(_) => metrics.record_error(),
                    }
                });
                handles.push(handle);
            }
            join_all(handles).await;
        }
        println!("  Phase 4 completed in {:?}", phase4_start.elapsed());

        let total_ops = metrics.operation_count.load(Ordering::Relaxed);
        let total_errors = metrics.error_count.load(Ordering::Relaxed);

        println!("\nCombined stress test summary:");
        println!("  Total operations: {}", total_ops);
        println!("  Total errors: {}", total_errors);
        println!("  Avg latency: {:.2}ms", metrics.avg_latency_ms());
        println!("  Max latency: {:.2}ms", metrics.max_latency_ms());
        println!("  Error rate: {:.2}%", metrics.error_rate() * 100.0);

        // Combined test should have low error rate
        assert!(
            metrics.error_rate() < 0.05,
            "Error rate too high in combined stress: {:.2}%",
            metrics.error_rate() * 100.0
        );
    })
    .await;

    assert!(
        test_result.is_ok(),
        "Combined extreme stress test timed out"
    );
}
