//! Parallelization enforcement for safe concurrent module execution
//!
//! This module provides synchronization primitives to enforce the parallelization
//! hints declared by modules, preventing race conditions and resource contention.

use crate::modules::ParallelizationHint;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::debug;

/// Token bucket for rate limiting
struct TokenBucket {
    /// Maximum tokens (capacity)
    capacity: u32,
    /// Current number of tokens
    tokens: f64,
    /// Tokens added per second
    refill_rate: f64,
    /// Last refill time
    last_refill: Instant,
}

impl TokenBucket {
    fn new(requests_per_second: u32) -> Self {
        Self {
            capacity: requests_per_second,
            tokens: requests_per_second as f64,
            refill_rate: requests_per_second as f64,
            last_refill: Instant::now(),
        }
    }

    /// Try to acquire a token, returns true if successful
    fn try_acquire(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;
    }

    /// Get time until next token is available
    fn time_until_available(&mut self) -> Duration {
        self.refill();
        if self.tokens >= 1.0 {
            Duration::from_millis(0)
        } else {
            let tokens_needed = 1.0 - self.tokens;
            let seconds = tokens_needed / self.refill_rate;
            Duration::from_secs_f64(seconds)
        }
    }
}

/// Manages parallelization constraints across module executions
#[derive(Clone)]
pub struct ParallelizationManager {
    /// Per-host semaphores for HostExclusive modules (one execution per host)
    host_semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    /// Global mutex for GlobalExclusive modules (one execution across all hosts)
    global_mutex: Arc<Semaphore>,
    /// Token buckets for rate-limited modules, keyed by module name
    rate_limiters: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

impl ParallelizationManager {
    /// Create a new parallelization manager
    pub fn new() -> Self {
        Self {
            host_semaphores: Arc::new(Mutex::new(HashMap::new())),
            global_mutex: Arc::new(Semaphore::new(1)),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquire necessary permits/locks for a module execution
    ///
    /// # Arguments
    /// * `hint` - The parallelization hint from the module
    /// * `host` - The target host
    /// * `module_name` - Name of the module (for rate limiting)
    ///
    /// # Returns
    /// A guard that holds the necessary permits until dropped
    pub async fn acquire(
        &self,
        hint: ParallelizationHint,
        host: &str,
        module_name: &str,
    ) -> ParallelizationGuard {
        match hint {
            ParallelizationHint::FullyParallel => {
                // No restrictions - immediate execution
                debug!(
                    "Module '{}' on host '{}': FullyParallel (no restrictions)",
                    module_name, host
                );
                ParallelizationGuard::FullyParallel
            }

            ParallelizationHint::HostExclusive => {
                // Only one execution per host
                debug!(
                    "Module '{}' on host '{}': HostExclusive (acquiring per-host lock)",
                    module_name, host
                );
                let semaphore = {
                    let mut semaphores = self.host_semaphores.lock();
                    semaphores
                        .entry(host.to_string())
                        .or_insert_with(|| Arc::new(Semaphore::new(1)))
                        .clone()
                };

                let permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("Semaphore should not be closed");
                debug!(
                    "Module '{}' on host '{}': HostExclusive lock acquired",
                    module_name, host
                );
                ParallelizationGuard::HostExclusive(permit)
            }

            ParallelizationHint::RateLimited {
                requests_per_second,
            } => {
                // Rate-limited - wait until token is available
                debug!(
                    "Module '{}' on host '{}': RateLimited ({} req/sec)",
                    module_name, host, requests_per_second
                );

                // Initialize rate limiter if needed
                {
                    let mut limiters = self.rate_limiters.lock();
                    limiters
                        .entry(module_name.to_string())
                        .or_insert_with(|| TokenBucket::new(requests_per_second));
                }

                // Wait for token availability
                loop {
                    let wait_duration = {
                        let mut limiters = self.rate_limiters.lock();
                        if let Some(bucket) = limiters.get_mut(module_name) {
                            if bucket.try_acquire() {
                                debug!(
                                    "Module '{}' on host '{}': RateLimited token acquired",
                                    module_name, host
                                );
                                break;
                            }
                            bucket.time_until_available()
                        } else {
                            Duration::from_millis(0)
                        }
                    };

                    if wait_duration.as_millis() > 0 {
                        debug!(
                            "Module '{}' on host '{}': RateLimited waiting {:?}",
                            module_name, host, wait_duration
                        );
                        tokio::time::sleep(wait_duration).await;
                    }
                }

                ParallelizationGuard::RateLimited
            }

            ParallelizationHint::GlobalExclusive => {
                // Only one execution globally
                debug!(
                    "Module '{}' on host '{}': GlobalExclusive (acquiring global lock)",
                    module_name, host
                );
                let permit = self
                    .global_mutex
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("Semaphore should not be closed");
                debug!(
                    "Module '{}' on host '{}': GlobalExclusive lock acquired",
                    module_name, host
                );
                ParallelizationGuard::GlobalExclusive(permit)
            }
        }
    }

    /// Get statistics about current parallelization state (for debugging/monitoring)
    pub fn stats(&self) -> ParallelizationStats {
        let host_locks = self
            .host_semaphores
            .lock()
            .iter()
            .map(|(host, sem)| (host.clone(), sem.available_permits()))
            .collect();

        let global_available = self.global_mutex.available_permits();

        let rate_limiter_states = self
            .rate_limiters
            .lock()
            .iter()
            .map(|(name, bucket)| {
                (
                    name.clone(),
                    RateLimiterState {
                        tokens: bucket.tokens,
                        capacity: bucket.capacity,
                        refill_rate: bucket.refill_rate,
                    },
                )
            })
            .collect();

        ParallelizationStats {
            host_locks,
            global_available,
            rate_limiter_states,
        }
    }
}

impl Default for ParallelizationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that holds parallelization permits until dropped
pub enum ParallelizationGuard {
    /// No restrictions - drops immediately
    FullyParallel,
    /// Holds a per-host semaphore permit
    HostExclusive(OwnedSemaphorePermit),
    /// Rate-limited - token was consumed
    RateLimited,
    /// Holds the global semaphore permit
    GlobalExclusive(OwnedSemaphorePermit),
}

impl Drop for ParallelizationGuard {
    fn drop(&mut self) {
        match self {
            ParallelizationGuard::FullyParallel => {
                // Nothing to release
            }
            ParallelizationGuard::HostExclusive(_permit) => {
                // Permit is automatically released when dropped
                debug!("HostExclusive lock released");
            }
            ParallelizationGuard::RateLimited => {
                // Token was already consumed
            }
            ParallelizationGuard::GlobalExclusive(_permit) => {
                // Permit is automatically released when dropped
                debug!("GlobalExclusive lock released");
            }
        }
    }
}

/// Statistics about current parallelization state
#[derive(Debug, Clone)]
pub struct ParallelizationStats {
    /// Available permits per host (0 = locked, 1 = available)
    pub host_locks: HashMap<String, usize>,
    /// Global lock availability (0 = locked, 1 = available)
    pub global_available: usize,
    /// Rate limiter states per module
    pub rate_limiter_states: HashMap<String, RateLimiterState>,
}

/// State of a rate limiter
#[derive(Debug, Clone)]
pub struct RateLimiterState {
    /// Current tokens
    pub tokens: f64,
    /// Maximum tokens
    pub capacity: u32,
    /// Tokens per second
    pub refill_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(tarpaulin, ignore)]
    #[tokio::test]
    async fn test_fully_parallel_no_blocking() {
        let manager = ParallelizationManager::new();

        // Multiple fully parallel operations should execute immediately
        let start = Instant::now();
        let mut handles = vec![];

        for i in 0..10 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager
                    .acquire(
                        ParallelizationHint::FullyParallel,
                        "host1",
                        &format!("test-{}", i),
                    )
                    .await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // All should execute in parallel, so total time should be close to 10ms
        assert!(
            elapsed < Duration::from_millis(50),
            "Fully parallel should not block: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_host_exclusive_blocks_per_host() {
        let manager = Arc::new(ParallelizationManager::new());

        // Two operations on same host should be serialized
        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1
                .acquire(ParallelizationHint::HostExclusive, "host1", "test")
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        // Give first task time to acquire lock
        tokio::time::sleep(Duration::from_millis(20)).await;

        let manager2 = manager.clone();
        let start = Instant::now();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2
                .acquire(ParallelizationHint::HostExclusive, "host1", "test")
                .await;
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        let elapsed = start.elapsed();

        // Second task should have waited for first (use generous timing for CI)
        assert!(
            elapsed >= Duration::from_millis(50),
            "Host exclusive should block: took {:?}",
            elapsed
        );
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[tokio::test]
    async fn test_host_exclusive_different_hosts_parallel() {
        let manager = ParallelizationManager::new();

        // Operations on different hosts should be able to hold their permits at the same time.
        let _guard1 = manager
            .acquire(ParallelizationHint::HostExclusive, "host1", "test")
            .await;
        let _guard2 = manager
            .acquire(ParallelizationHint::HostExclusive, "host2", "test")
            .await;

        let stats = manager.stats();
        assert_eq!(stats.host_locks.get("host1"), Some(&0));
        assert_eq!(stats.host_locks.get("host2"), Some(&0));
    }

    #[tokio::test]
    async fn test_global_exclusive_blocks_all() {
        let manager = Arc::new(ParallelizationManager::new());

        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1
                .acquire(ParallelizationHint::GlobalExclusive, "host1", "test")
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        // Give first task time to acquire lock
        tokio::time::sleep(Duration::from_millis(20)).await;

        let manager2 = manager.clone();
        let start = Instant::now();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2
                .acquire(ParallelizationHint::GlobalExclusive, "host2", "test")
                .await;
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        let elapsed = start.elapsed();

        // Second task should wait even on different host (use generous timing for CI)
        assert!(
            elapsed >= Duration::from_millis(50),
            "Global exclusive should block all hosts: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_rate_limited_enforces_limit() {
        let manager = Arc::new(ParallelizationManager::new());

        // 2 requests per second = 500ms per request
        // Token bucket starts full with capacity tokens, so use more requests than capacity
        let hint = ParallelizationHint::RateLimited {
            requests_per_second: 2,
        };

        // Rate limiters are keyed by module name, so use the same name for all requests
        let module_name = "rate-limited-module";

        // First, drain the bucket by making initial requests (capacity = 2)
        for _ in 0..2 {
            let _guard = manager.acquire(hint, "host1", module_name).await;
        }

        // Now the bucket should be empty, and subsequent requests must wait
        let start = Instant::now();
        let _guard1 = manager.acquire(hint, "host1", module_name).await;
        let first_elapsed = start.elapsed();

        let _guard2 = manager.acquire(hint, "host1", module_name).await;
        let second_elapsed = start.elapsed();

        // First request after draining should wait ~500ms (1/2 second)
        assert!(
            first_elapsed >= Duration::from_millis(400),
            "Rate limiting should enforce delay for first request: took {:?}",
            first_elapsed
        );

        // Second request should wait another ~500ms
        assert!(
            second_elapsed >= Duration::from_millis(800),
            "Rate limiting should enforce delay for second request: took {:?}",
            second_elapsed
        );
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(10);

        // Use all tokens
        for _ in 0..10 {
            assert!(bucket.try_acquire());
        }

        // Should be empty
        assert!(!bucket.try_acquire());

        // Wait and refill
        std::thread::sleep(Duration::from_millis(200));
        bucket.refill();

        // Should have ~2 tokens now (200ms * 10 tokens/sec)
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let manager = Arc::new(ParallelizationManager::new());

        // Acquire some locks
        let _guard1 = manager
            .acquire(ParallelizationHint::HostExclusive, "host1", "test")
            .await;
        let _guard2 = manager
            .acquire(ParallelizationHint::GlobalExclusive, "host2", "test2")
            .await;

        let stats = manager.stats();

        // Check host locks
        assert_eq!(stats.host_locks.get("host1"), Some(&0)); // Locked
        assert_eq!(stats.global_available, 0); // Locked
    }
}
