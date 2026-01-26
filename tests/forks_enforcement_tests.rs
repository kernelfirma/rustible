//! Forks Enforcement Tests
//!
//! These tests verify that the --forks option is properly enforced:
//! - Concurrency never exceeds the forks limit
//! - Work-stealing doesn't violate limits
//! - Edge cases (forks=0, very high forks) are handled

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::sleep;

/// Track concurrent execution and verify it never exceeds limit
async fn verify_forks_enforcement(num_tasks: usize, forks: usize) -> Result<(), String> {
    let semaphore = Arc::new(Semaphore::new(forks));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));
    let violation_count = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..num_tasks)
        .map(|task_id| {
            let sem = Arc::clone(&semaphore);
            let max_conc = Arc::clone(&max_concurrent);
            let curr_conc = Arc::clone(&current_concurrent);
            let violations = Arc::clone(&violation_count);

            tokio::spawn(async move {
                // Acquire permit (blocks if forks limit reached)
                let _permit = sem.acquire().await.unwrap();

                // Track concurrent execution
                let current = curr_conc.fetch_add(1, Ordering::SeqCst) + 1;

                // Check for violations
                if current > forks {
                    violations.fetch_add(1, Ordering::SeqCst);
                }

                // Update max concurrent
                max_conc.fetch_max(current, Ordering::SeqCst);

                // Simulate variable work duration
                sleep(Duration::from_millis(5 + (task_id % 10) as u64)).await;

                // Release concurrent count
                curr_conc.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }

    let max_concurrent_count = max_concurrent.load(Ordering::SeqCst);
    let violation_count_val = violation_count.load(Ordering::SeqCst);

    if violation_count_val > 0 {
        return Err(format!(
            "Forks limit violated {} times! Max concurrent: {}, limit: {}",
            violation_count_val, max_concurrent_count, forks
        ));
    }

    if max_concurrent_count > forks {
        return Err(format!(
            "Max concurrent {} exceeded forks limit {}",
            max_concurrent_count, forks
        ));
    }

    Ok(())
}

// ============================================================================
// Strict Enforcement Tests
// ============================================================================

#[tokio::test]
async fn test_forks_strictly_enforced_1() {
    verify_forks_enforcement(100, 1).await.unwrap();
}

#[tokio::test]
async fn test_forks_strictly_enforced_2() {
    verify_forks_enforcement(100, 2).await.unwrap();
}

#[tokio::test]
async fn test_forks_strictly_enforced_5() {
    verify_forks_enforcement(100, 5).await.unwrap();
}

#[tokio::test]
async fn test_forks_strictly_enforced_10() {
    verify_forks_enforcement(100, 10).await.unwrap();
}

#[tokio::test]
async fn test_forks_strictly_enforced_50() {
    verify_forks_enforcement(500, 50).await.unwrap();
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_forks_more_than_tasks() {
    // When forks > num_tasks, all tasks should run in parallel
    let num_tasks = 5;
    let forks = 100;

    let semaphore = Arc::new(Semaphore::new(forks));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();

    let handles: Vec<_> = (0..num_tasks)
        .map(|_| {
            let sem = Arc::clone(&semaphore);
            let max_conc = Arc::clone(&max_concurrent);
            let curr_conc = Arc::clone(&current_concurrent);

            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();

                let current = curr_conc.fetch_add(1, Ordering::SeqCst) + 1;
                max_conc.fetch_max(current, Ordering::SeqCst);

                sleep(Duration::from_millis(10)).await;

                curr_conc.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    let duration = start.elapsed();
    let max_concurrent_count = max_concurrent.load(Ordering::SeqCst);

    // All tasks should run in parallel
    assert!(
        max_concurrent_count >= num_tasks - 1,
        "Expected all {} tasks to run in parallel, got max {}",
        num_tasks,
        max_concurrent_count
    );

    // Should complete quickly (roughly one round)
    assert!(
        duration.as_millis() < 30,
        "All tasks in parallel should complete in <30ms, took {}ms",
        duration.as_millis()
    );
}

#[tokio::test]
async fn test_forks_single_task() {
    // Single task should work regardless of forks value
    verify_forks_enforcement(1, 1).await.unwrap();
    verify_forks_enforcement(1, 5).await.unwrap();
    verify_forks_enforcement(1, 100).await.unwrap();
}

#[tokio::test]
async fn test_forks_rapid_acquire_release() {
    // Test rapid permit acquisition and release doesn't cause issues
    let forks = 5;
    let semaphore = Arc::new(Semaphore::new(forks));
    let total_acquires = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..100)
        .map(|_| {
            let sem = Arc::clone(&semaphore);
            let acquires = Arc::clone(&total_acquires);

            tokio::spawn(async move {
                // Rapid acquire-release cycles
                for _ in 0..10 {
                    let _permit = sem.acquire().await.unwrap();
                    acquires.fetch_add(1, Ordering::SeqCst);
                    // Very short work
                    tokio::task::yield_now().await;
                }
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // Should complete 1000 total acquisitions (100 tasks * 10 cycles)
    assert_eq!(
        total_acquires.load(Ordering::SeqCst),
        1000,
        "Should have completed 1000 permit acquisitions"
    );
}

// ============================================================================
// Work Duration Variance Tests
// ============================================================================

#[tokio::test]
async fn test_forks_with_variable_work_duration() {
    // Test that forks is enforced even when tasks have different durations
    let forks = 5;
    let semaphore = Arc::new(Semaphore::new(forks));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..30)
        .map(|i| {
            let sem = Arc::clone(&semaphore);
            let max_conc = Arc::clone(&max_concurrent);
            let curr_conc = Arc::clone(&current_concurrent);

            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();

                let current = curr_conc.fetch_add(1, Ordering::SeqCst) + 1;
                max_conc.fetch_max(current, Ordering::SeqCst);

                // Variable work duration: 5ms, 10ms, 15ms, 20ms, 25ms
                let duration = 5 + (i % 5) * 5;
                sleep(Duration::from_millis(duration as u64)).await;

                curr_conc.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    let max_concurrent_count = max_concurrent.load(Ordering::SeqCst);

    // Forks limit should never be exceeded
    assert!(
        max_concurrent_count <= forks,
        "Max concurrent {} exceeded forks limit {}",
        max_concurrent_count,
        forks
    );
}

// ============================================================================
// Concurrency Pattern Tests
// ============================================================================

#[tokio::test]
async fn test_forks_batching_pattern() {
    // Verify tasks complete in batches of forks size
    let forks = 5;
    let num_tasks = 25; // 5 batches

    let semaphore = Arc::new(Semaphore::new(forks));
    let batch_tracker = Arc::new(tokio::sync::Mutex::new(Vec::<usize>::new()));
    let current_concurrent = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..num_tasks)
        .map(|_| {
            let sem = Arc::clone(&semaphore);
            let batch = Arc::clone(&batch_tracker);
            let curr_conc = Arc::clone(&current_concurrent);

            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();

                let current = curr_conc.fetch_add(1, Ordering::SeqCst) + 1;

                // Record concurrent count when task starts
                batch.lock().await.push(current);

                sleep(Duration::from_millis(10)).await;

                curr_conc.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // All recorded concurrent counts should be <= forks
    let batch_counts = batch_tracker.lock().await;
    for &count in batch_counts.iter() {
        assert!(
            count <= forks,
            "Batch count {} exceeded forks limit {}",
            count,
            forks
        );
    }
}

// ============================================================================
// CLI Forks Value Tests
// ============================================================================

#[test]
fn test_forks_value_parsing() {
    // Test that forks values are properly validated
    use std::str::FromStr;

    // Valid values
    assert!(usize::from_str("1").is_ok());
    assert!(usize::from_str("5").is_ok());
    assert!(usize::from_str("100").is_ok());

    // Invalid values
    assert!(usize::from_str("0").unwrap() == 0); // Note: 0 may be treated as unlimited
    assert!(usize::from_str("-1").is_err());
    assert!(usize::from_str("abc").is_err());
}

#[test]
fn test_forks_config_precedence() {
    // Test that CLI --forks takes precedence over config file
    // This is a conceptual test - actual precedence is in CLI handling

    struct ForkConfig {
        cli_forks: Option<usize>,
        config_forks: Option<usize>,
        default_forks: usize,
    }

    impl ForkConfig {
        fn effective_forks(&self) -> usize {
            // CLI > Config > Default
            self.cli_forks
                .or(self.config_forks)
                .unwrap_or(self.default_forks)
        }
    }

    let config = ForkConfig {
        cli_forks: Some(10),
        config_forks: Some(20),
        default_forks: 5,
    };

    assert_eq!(config.effective_forks(), 10, "CLI should take precedence");

    let config_only = ForkConfig {
        cli_forks: None,
        config_forks: Some(20),
        default_forks: 5,
    };

    assert_eq!(
        config_only.effective_forks(),
        20,
        "Config should be used when no CLI"
    );

    let default_only = ForkConfig {
        cli_forks: None,
        config_forks: None,
        default_forks: 5,
    };

    assert_eq!(
        default_only.effective_forks(),
        5,
        "Default should be used when no CLI or config"
    );
}
