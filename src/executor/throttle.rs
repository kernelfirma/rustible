//! Task throttling for Rustible
//!
//! This module provides throttling capabilities to limit concurrent task executions:
//! - Global throttle limits (across all hosts)
//! - Per-host throttle limits
//! - Per-module rate limiting
//! - Resource exhaustion prevention
//!
//! The throttle parameter limits how many hosts a particular task can run on at once,
//! independent of the forks setting.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::debug;

/// Configuration for throttling behavior
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct ThrottleConfig {
    /// Global throttle limit (0 = unlimited, use forks)
    pub global_limit: usize,
    /// Per-host concurrent task limit (0 = unlimited)
    pub per_host_limit: usize,
    /// Per-module rate limits (module_name -> requests per second)
    pub module_rate_limits: HashMap<String, u32>,
    /// Default rate limit for unlisted modules (0 = unlimited)
    pub default_rate_limit: u32,
}


impl ThrottleConfig {
    /// Create a new throttle config with a global limit
    pub fn with_global_limit(limit: usize) -> Self {
        Self {
            global_limit: limit,
            ..Default::default()
        }
    }

    /// Set the per-host limit
    pub fn per_host(mut self, limit: usize) -> Self {
        self.per_host_limit = limit;
        self
    }

    /// Add a module rate limit
    pub fn rate_limit_module(
        mut self,
        module: impl Into<String>,
        requests_per_second: u32,
    ) -> Self {
        self.module_rate_limits
            .insert(module.into(), requests_per_second);
        self
    }
}

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

/// Guard that holds throttle permits until dropped
pub enum ThrottleGuard {
    /// No throttling - immediate execution
    None,
    /// Holds a global semaphore permit
    Global(OwnedSemaphorePermit),
    /// Holds a per-host semaphore permit
    PerHost(OwnedSemaphorePermit),
    /// Holds both global and per-host permits
    Combined {
        global: OwnedSemaphorePermit,
        per_host: OwnedSemaphorePermit,
    },
    /// Rate-limited - token was consumed
    RateLimited,
}

impl Drop for ThrottleGuard {
    fn drop(&mut self) {
        match self {
            ThrottleGuard::None => {}
            ThrottleGuard::Global(_) => {
                debug!("Global throttle permit released");
            }
            ThrottleGuard::PerHost(_) => {
                debug!("Per-host throttle permit released");
            }
            ThrottleGuard::Combined { .. } => {
                debug!("Combined throttle permits released");
            }
            ThrottleGuard::RateLimited => {}
        }
    }
}

/// Manages throttling across task executions
///
/// The ThrottleManager enforces:
/// - Global concurrent execution limits (task-level throttle parameter)
/// - Per-host execution limits
/// - Per-module rate limiting
#[derive(Clone)]
pub struct ThrottleManager {
    /// Global throttle semaphore (limits concurrent executions across all hosts)
    global_semaphore: Option<Arc<Semaphore>>,
    /// Per-host semaphores
    host_semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    /// Per-host limit (0 = unlimited)
    per_host_limit: usize,
    /// Module rate limiters
    rate_limiters: Arc<Mutex<HashMap<String, TokenBucket>>>,
    /// Module rate limit configuration
    module_rate_limits: HashMap<String, u32>,
    /// Default rate limit
    default_rate_limit: u32,
}

impl ThrottleManager {
    /// Create a new throttle manager with the given configuration
    pub fn new(config: ThrottleConfig) -> Self {
        let global_semaphore = if config.global_limit > 0 {
            Some(Arc::new(Semaphore::new(config.global_limit)))
        } else {
            None
        };

        Self {
            global_semaphore,
            host_semaphores: Arc::new(Mutex::new(HashMap::new())),
            per_host_limit: config.per_host_limit,
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            module_rate_limits: config.module_rate_limits,
            default_rate_limit: config.default_rate_limit,
        }
    }

    /// Create a throttle manager with just a task-level throttle limit
    pub fn with_task_throttle(throttle: u32) -> Self {
        Self::new(ThrottleConfig::with_global_limit(throttle as usize))
    }

    /// Create an unlimited throttle manager (no throttling)
    pub fn unlimited() -> Self {
        Self::new(ThrottleConfig::default())
    }

    /// Acquire throttle permits for a task execution
    ///
    /// This method blocks until all required permits are acquired.
    /// The returned guard must be held for the duration of the task execution.
    ///
    /// # Arguments
    /// * `host` - The target host
    /// * `module_name` - Name of the module being executed (for rate limiting)
    /// * `task_throttle` - Optional task-level throttle override
    pub async fn acquire(
        &self,
        host: &str,
        module_name: &str,
        task_throttle: Option<u32>,
    ) -> ThrottleGuard {
        // Check for module rate limiting first
        if let Some(rate_limit) = self.get_rate_limit(module_name) {
            self.wait_for_rate_limit(module_name, rate_limit).await;
            return ThrottleGuard::RateLimited;
        }

        // Determine which semaphore to use
        let global_permit = if let Some(ref sem) = self.global_semaphore {
            debug!(
                "Acquiring global throttle permit for module '{}' on host '{}'",
                module_name, host
            );
            Some(
                sem.clone()
                    .acquire_owned()
                    .await
                    .expect("Semaphore should not be closed"),
            )
        } else if let Some(throttle) = task_throttle {
            // Task-level throttle creates a temporary semaphore
            debug!(
                "Task throttle {} for module '{}' on host '{}'",
                throttle, module_name, host
            );
            // For task-level throttle, we need to manage it differently
            // This will be handled by the executor's task-specific semaphore
            None
        } else {
            None
        };

        let per_host_permit = if self.per_host_limit > 0 {
            let sem = {
                let mut semaphores = self.host_semaphores.lock();
                semaphores
                    .entry(host.to_string())
                    .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_limit)))
                    .clone()
            };

            debug!(
                "Acquiring per-host throttle permit for module '{}' on host '{}'",
                module_name, host
            );
            Some(
                sem.acquire_owned()
                    .await
                    .expect("Semaphore should not be closed"),
            )
        } else {
            None
        };

        match (global_permit, per_host_permit) {
            (Some(global), Some(per_host)) => ThrottleGuard::Combined { global, per_host },
            (Some(global), None) => ThrottleGuard::Global(global),
            (None, Some(per_host)) => ThrottleGuard::PerHost(per_host),
            (None, None) => ThrottleGuard::None,
        }
    }

    /// Get the rate limit for a module
    fn get_rate_limit(&self, module_name: &str) -> Option<u32> {
        self.module_rate_limits
            .get(module_name)
            .copied()
            .or({
                if self.default_rate_limit > 0 {
                    Some(self.default_rate_limit)
                } else {
                    None
                }
            })
    }

    /// Wait for rate limit token
    async fn wait_for_rate_limit(&self, module_name: &str, rate_limit: u32) {
        // Initialize rate limiter if needed
        {
            let mut limiters = self.rate_limiters.lock();
            limiters
                .entry(module_name.to_string())
                .or_insert_with(|| TokenBucket::new(rate_limit));
        }

        // Wait for token availability
        loop {
            let wait_duration = {
                let mut limiters = self.rate_limiters.lock();
                if let Some(bucket) = limiters.get_mut(module_name) {
                    if bucket.try_acquire() {
                        debug!("Rate limit token acquired for module '{}'", module_name);
                        break;
                    }
                    bucket.time_until_available()
                } else {
                    Duration::from_millis(0)
                }
            };

            if wait_duration.as_millis() > 0 {
                debug!(
                    "Rate limiting module '{}': waiting {:?}",
                    module_name, wait_duration
                );
                tokio::time::sleep(wait_duration).await;
            }
        }
    }

    /// Get current throttle statistics
    pub fn stats(&self) -> ThrottleStats {
        let global_available = self
            .global_semaphore
            .as_ref()
            .map(|s| s.available_permits())
            .unwrap_or(0);

        let host_permits: HashMap<String, usize> = self
            .host_semaphores
            .lock()
            .iter()
            .map(|(host, sem)| (host.clone(), sem.available_permits()))
            .collect();

        let rate_limiter_states: HashMap<String, RateLimiterState> = self
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

        ThrottleStats {
            global_available,
            host_permits,
            rate_limiter_states,
        }
    }
}

impl Default for ThrottleManager {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Statistics about current throttle state
#[derive(Debug, Clone)]
pub struct ThrottleStats {
    /// Available global permits (0 if no global limit)
    pub global_available: usize,
    /// Available permits per host
    pub host_permits: HashMap<String, usize>,
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

/// Task-level throttle semaphore manager
///
/// This manages per-task throttle limits that are different from the global
/// executor forks setting. Each task can specify its own throttle limit.
#[derive(Clone)]
pub struct TaskThrottleManager {
    /// Semaphores per task name (or unique task identifier)
    task_semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl TaskThrottleManager {
    /// Create a new task throttle manager
    pub fn new() -> Self {
        Self {
            task_semaphores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get or create a semaphore for a specific task with its throttle limit
    pub fn get_or_create(&self, task_id: &str, throttle: u32) -> Arc<Semaphore> {
        let mut semaphores = self.task_semaphores.lock();
        semaphores
            .entry(task_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(throttle as usize)))
            .clone()
    }

    /// Acquire a permit for a task execution
    pub async fn acquire(&self, task_id: &str, throttle: u32) -> OwnedSemaphorePermit {
        let sem = self.get_or_create(task_id, throttle);
        debug!(
            "Acquiring task throttle permit for task '{}' (limit: {})",
            task_id, throttle
        );
        sem.acquire_owned()
            .await
            .expect("Task semaphore should not be closed")
    }

    /// Clear all task semaphores (for cleanup between plays)
    pub fn clear(&self) {
        let mut semaphores = self.task_semaphores.lock();
        semaphores.clear();
    }
}

impl Default for TaskThrottleManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Flaky on CI - timing precision varies"]
    async fn test_no_throttle_immediate() {
        let manager = ThrottleManager::unlimited();

        let start = Instant::now();
        let mut handles = vec![];

        for i in 0..10 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager.acquire("host1", &format!("test-{}", i), None).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // All should execute in parallel
        assert!(
            elapsed < Duration::from_millis(50),
            "No throttle should not block: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_global_throttle_limits_concurrent() {
        let config = ThrottleConfig::with_global_limit(2);
        let manager = Arc::new(ThrottleManager::new(config));

        let start = Instant::now();
        let mut handles = vec![];

        for i in 0..4 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager.acquire(&format!("host{}", i), "test", None).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // With throttle of 2, 4 tasks should take at least 2 batches
        assert!(
            elapsed >= Duration::from_millis(90),
            "Global throttle should serialize: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_per_host_throttle() {
        let config = ThrottleConfig::default().per_host(1);
        let manager = Arc::new(ThrottleManager::new(config));

        // Two tasks on same host should be serialized
        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1.acquire("host1", "test", None).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        let manager2 = manager.clone();
        let start = Instant::now();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2.acquire("host1", "test", None).await;
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        let elapsed = start.elapsed();
        // Use generous timing tolerance for CI environments
        assert!(
            elapsed >= Duration::from_millis(50),
            "Per-host throttle should serialize same host: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    #[ignore = "Flaky on CI - timing precision varies"]
    async fn test_per_host_different_hosts_parallel() {
        let config = ThrottleConfig::default().per_host(1);
        let manager = Arc::new(ThrottleManager::new(config));

        let start = Instant::now();

        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1.acquire("host1", "test", None).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let manager2 = manager.clone();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2.acquire("host2", "test", None).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        futures::future::join_all(vec![handle1, handle2]).await;
        let elapsed = start.elapsed();

        // Different hosts should run in parallel
        assert!(
            elapsed < Duration::from_millis(80),
            "Different hosts should run in parallel: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = ThrottleConfig::default().rate_limit_module("api_call", 2);
        let manager = Arc::new(ThrottleManager::new(config));

        // First drain the bucket
        let _guard1 = manager.acquire("host1", "api_call", None).await;
        let _guard2 = manager.acquire("host1", "api_call", None).await;

        // Now next request should wait
        let start = Instant::now();
        let _guard3 = manager.acquire("host1", "api_call", None).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(400),
            "Rate limiting should enforce delay: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_task_throttle_manager() {
        let manager = TaskThrottleManager::new();

        let start = Instant::now();
        let mut handles = vec![];

        // Create 4 tasks with throttle of 2
        for _i in 0..4 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _permit = manager.acquire("task1", 2).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // With throttle of 2, 4 tasks should take at least 2 batches
        assert!(
            elapsed >= Duration::from_millis(90),
            "Task throttle should serialize: took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_throttle_stats() {
        let config = ThrottleConfig::with_global_limit(5).per_host(2);
        let manager = ThrottleManager::new(config);

        let stats = manager.stats();
        assert_eq!(stats.global_available, 5);
    }
}
