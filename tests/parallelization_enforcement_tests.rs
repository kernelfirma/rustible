//! Integration tests for parallelization hint enforcement
//!
//! These tests verify that the executor properly enforces module parallelization hints:
//! - FullyParallel: No restrictions
//! - HostExclusive: Only one task per host
//! - RateLimited: Rate limiting enforced
//! - GlobalExclusive: Only one task globally

use rustible::executor::parallelization::ParallelizationManager;
use rustible::modules::ParallelizationHint;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Test that HostExclusive prevents concurrent execution on the same host
#[tokio::test]
async fn test_host_exclusive_enforcement() {
    let manager = Arc::new(ParallelizationManager::new());
    let execution_log = Arc::new(Mutex::new(Vec::new()));

    // Simulate two apt tasks on the same host
    let host = "server1";
    let module = "apt";

    let manager1 = manager.clone();
    let log1 = execution_log.clone();
    let handle1 = tokio::spawn(async move {
        let _guard = manager1
            .acquire(ParallelizationHint::HostExclusive, host, module)
            .await;
        log1.lock().await.push("Task1 started".to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
        log1.lock().await.push("Task1 finished".to_string());
    });

    // Give first task time to acquire lock
    tokio::time::sleep(Duration::from_millis(10)).await;

    let manager2 = manager.clone();
    let log2 = execution_log.clone();
    let handle2 = tokio::spawn(async move {
        let start = Instant::now();
        let _guard = manager2
            .acquire(ParallelizationHint::HostExclusive, host, module)
            .await;
        let wait_time = start.elapsed();
        log2.lock()
            .await
            .push(format!("Task2 started after {:?}", wait_time));
        tokio::time::sleep(Duration::from_millis(50)).await;
        log2.lock().await.push("Task2 finished".to_string());
    });

    handle1.await.unwrap();
    handle2.await.unwrap();

    let log = execution_log.lock().await;

    // Verify execution order: Task1 must complete before Task2 starts
    assert_eq!(log.len(), 4);
    assert!(log[0].contains("Task1 started"));
    assert!(log[1].contains("Task1 finished"));
    assert!(log[2].contains("Task2 started"));
    assert!(log[2].contains("after")); // Should have waited
    assert!(log[3].contains("Task2 finished"));
}

/// Test that HostExclusive allows concurrent execution on different hosts
#[tokio::test]
async fn test_host_exclusive_different_hosts_parallel() {
    let manager = Arc::new(ParallelizationManager::new());
    let execution_log = Arc::new(Mutex::new(Vec::new()));

    let start = Instant::now();

    // Simulate two apt tasks on different hosts
    let manager1 = manager.clone();
    let log1 = execution_log.clone();
    let handle1 = tokio::spawn(async move {
        let _guard = manager1
            .acquire(ParallelizationHint::HostExclusive, "host1", "apt")
            .await;
        log1.lock().await.push("host1 started".to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
        log1.lock().await.push("host1 finished".to_string());
    });

    let manager2 = manager.clone();
    let log2 = execution_log.clone();
    let handle2 = tokio::spawn(async move {
        let _guard = manager2
            .acquire(ParallelizationHint::HostExclusive, "host2", "apt")
            .await;
        log2.lock().await.push("host2 started".to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
        log2.lock().await.push("host2 finished".to_string());
    });

    handle1.await.unwrap();
    handle2.await.unwrap();

    let elapsed = start.elapsed();

    // Both should run in parallel, so total time should be ~100ms not ~200ms
    assert!(
        elapsed < Duration::from_millis(150),
        "Different hosts should run in parallel: took {:?}",
        elapsed
    );
}

/// Test that GlobalExclusive prevents any concurrent execution
#[tokio::test]
async fn test_global_exclusive_enforcement() {
    let manager = Arc::new(ParallelizationManager::new());
    let execution_log = Arc::new(Mutex::new(Vec::new()));

    // Simulate cluster-wide operations on different hosts
    let manager1 = manager.clone();
    let log1 = execution_log.clone();
    let handle1 = tokio::spawn(async move {
        let _guard = manager1
            .acquire(
                ParallelizationHint::GlobalExclusive,
                "host1",
                "cluster_config",
            )
            .await;
        log1.lock().await.push("host1 started".to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
        log1.lock().await.push("host1 finished".to_string());
    });

    // Give first task time to acquire lock
    tokio::time::sleep(Duration::from_millis(10)).await;

    let manager2 = manager.clone();
    let log2 = execution_log.clone();
    let start = Instant::now();
    let handle2 = tokio::spawn(async move {
        let _guard = manager2
            .acquire(
                ParallelizationHint::GlobalExclusive,
                "host2",
                "cluster_config",
            )
            .await;
        let wait_time = start.elapsed();
        log2.lock()
            .await
            .push(format!("host2 started after {:?}", wait_time));
        tokio::time::sleep(Duration::from_millis(50)).await;
        log2.lock().await.push("host2 finished".to_string());
    });

    handle1.await.unwrap();
    handle2.await.unwrap();

    let log = execution_log.lock().await;

    // Verify that host2 waited even though it's a different host
    assert_eq!(log.len(), 4);
    assert!(log[0].contains("host1 started"));
    assert!(log[1].contains("host1 finished"));
    assert!(log[2].contains("host2 started"));
    assert!(log[3].contains("host2 finished"));
}

/// Test that RateLimited enforces rate limits
#[tokio::test]
async fn test_rate_limited_enforcement() {
    let manager = Arc::new(ParallelizationManager::new());

    // 5 requests per second = 200ms per request
    let hint = ParallelizationHint::RateLimited {
        requests_per_second: 5,
    };

    let start = Instant::now();

    let mut handles = vec![];
    for _ in 0..10 {
        let manager = manager.clone();
        let handle = tokio::spawn(async move {
            let _guard = manager.acquire(hint, "host1", "api_module").await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let total_elapsed = start.elapsed();

    // 10 requests at 5 req/sec with 5 initial tokens:
    // - First 5 requests use burst capacity (immediate)
    // - Next 5 requests wait for token refill (5 * 200ms = 1000ms)
    // Total expected: ~1000ms minimum
    assert!(
        total_elapsed >= Duration::from_millis(800),
        "Rate limiting should enforce delays: took {:?}",
        total_elapsed
    );

    // Note: With burst capacity, the first 5 requests complete immediately,
    // so we can't assert all consecutive requests are 200ms apart.
    // The total time assertion above is sufficient to verify rate limiting works.
}

/// Test that FullyParallel has no restrictions
#[tokio::test]
async fn test_fully_parallel_no_restrictions() {
    let manager = Arc::new(ParallelizationManager::new());

    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..20 {
        let manager = manager.clone();
        let handle = tokio::spawn(async move {
            let _guard = manager
                .acquire(
                    ParallelizationHint::FullyParallel,
                    "host1",
                    &format!("module{}", i),
                )
                .await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let elapsed = start.elapsed();

    // All 20 tasks should run in parallel, so total time should be ~50ms not ~1000ms
    assert!(
        elapsed < Duration::from_millis(200),
        "Fully parallel should have no blocking: took {:?}",
        elapsed
    );
}

/// Test mixed parallelization hints
#[tokio::test]
async fn test_mixed_parallelization_hints() {
    let manager = Arc::new(ParallelizationManager::new());
    let execution_log = Arc::new(Mutex::new(Vec::new()));

    // Mix of different parallelization types
    let manager1 = manager.clone();
    let log1 = execution_log.clone();
    let handle1 = tokio::spawn(async move {
        let _guard = manager1
            .acquire(ParallelizationHint::HostExclusive, "host1", "apt")
            .await;
        log1.lock().await.push("apt on host1");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let manager2 = manager.clone();
    let log2 = execution_log.clone();
    let handle2 = tokio::spawn(async move {
        let _guard = manager2
            .acquire(ParallelizationHint::FullyParallel, "host1", "debug")
            .await;
        log2.lock().await.push("debug on host1");
    });

    let manager3 = manager.clone();
    let log3 = execution_log.clone();
    let handle3 = tokio::spawn(async move {
        let _guard = manager3
            .acquire(ParallelizationHint::HostExclusive, "host2", "yum")
            .await;
        log3.lock().await.push("yum on host2");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    handle1.await.unwrap();
    handle2.await.unwrap();
    handle3.await.unwrap();

    let log = execution_log.lock().await;
    assert_eq!(log.len(), 3);

    // Debug should execute immediately (FullyParallel)
    // apt and yum can run in parallel (different hosts)
}

/// Test that stats tracking works correctly
#[tokio::test]
async fn test_parallelization_stats() {
    let manager = Arc::new(ParallelizationManager::new());

    // Acquire various locks
    let _guard1 = manager
        .acquire(ParallelizationHint::HostExclusive, "host1", "apt")
        .await;
    let _guard2 = manager
        .acquire(ParallelizationHint::GlobalExclusive, "host2", "cluster")
        .await;

    let stats = manager.stats();

    // Check that locks are registered
    assert_eq!(stats.host_locks.get("host1"), Some(&0)); // Locked
    assert_eq!(stats.global_available, 0); // Locked

    // Drop guards and check stats again
    drop(_guard1);
    drop(_guard2);

    tokio::time::sleep(Duration::from_millis(10)).await; // Give time for cleanup

    let stats2 = manager.stats();
    assert_eq!(stats2.host_locks.get("host1"), Some(&1)); // Available
    assert_eq!(stats2.global_available, 1); // Available
}
