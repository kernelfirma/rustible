//! Robust retry logic with exponential backoff and configurable policies.
//!
//! This module provides advanced retry functionality for connection operations,
//! including exponential backoff with jitter, configurable retry policies,
//! and integration with the circuit breaker pattern.

use std::future::Future;
use std::time::Duration;

use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

use super::ConnectionError;

// ============================================================================
// Retry Policy Configuration
// ============================================================================

/// Strategy for calculating retry delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BackoffStrategy {
    /// Fixed delay between retries.
    Fixed,
    /// Linear increase: delay * attempt.
    Linear,
    /// Exponential increase: delay * 2^attempt.
    Exponential,
    /// Exponential with decorrelated jitter for better distribution.
    #[default]
    ExponentialWithJitter,
    /// Fibonacci sequence for gradual increase.
    Fibonacci,
}

/// Configuration for retry behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 means no retries).
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Initial delay between retries.
    #[serde(default = "default_initial_delay")]
    #[serde(with = "humantime_serde")]
    pub initial_delay: Duration,

    /// Maximum delay between retries (caps exponential growth).
    #[serde(default = "default_max_delay")]
    #[serde(with = "humantime_serde")]
    pub max_delay: Duration,

    /// Backoff strategy to use.
    #[serde(default)]
    pub strategy: BackoffStrategy,

    /// Multiplier for exponential/linear backoff (default: 2.0).
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,

    /// Jitter factor (0.0 to 1.0) - randomness added to delays.
    #[serde(default = "default_jitter")]
    pub jitter: f64,

    /// Timeout for each individual attempt.
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub attempt_timeout: Option<Duration>,

    /// Total timeout for all retry attempts combined.
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub total_timeout: Option<Duration>,

    /// Whether to retry on authentication failures.
    #[serde(default)]
    pub retry_on_auth_failure: bool,

    /// Whether to retry on timeout errors.
    #[serde(default = "default_true")]
    pub retry_on_timeout: bool,

    /// Specific error codes that should trigger a retry.
    #[serde(default)]
    pub retryable_codes: Vec<i32>,
}

fn default_max_retries() -> u32 {
    3
}

fn default_initial_delay() -> Duration {
    Duration::from_millis(500)
}

fn default_max_delay() -> Duration {
    Duration::from_secs(30)
}

fn default_multiplier() -> f64 {
    2.0
}

fn default_jitter() -> f64 {
    0.25
}

fn default_true() -> bool {
    true
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            initial_delay: default_initial_delay(),
            max_delay: default_max_delay(),
            strategy: BackoffStrategy::default(),
            multiplier: default_multiplier(),
            jitter: default_jitter(),
            attempt_timeout: None,
            total_timeout: None,
            retry_on_auth_failure: false,
            retry_on_timeout: true,
            retryable_codes: Vec::new(),
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a policy that never retries.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Create an aggressive retry policy for critical operations.
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            strategy: BackoffStrategy::ExponentialWithJitter,
            multiplier: 1.5,
            jitter: 0.3,
            ..Default::default()
        }
    }

    /// Create a conservative retry policy for less critical operations.
    pub fn conservative() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            strategy: BackoffStrategy::Exponential,
            multiplier: 2.0,
            jitter: 0.1,
            ..Default::default()
        }
    }

    /// Set the maximum number of retries.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set the initial delay.
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set the maximum delay.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set the backoff strategy.
    pub fn with_strategy(mut self, strategy: BackoffStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the multiplier.
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Set the jitter factor.
    pub fn with_jitter(mut self, jitter: f64) -> Self {
        self.jitter = jitter.clamp(0.0, 1.0);
        self
    }

    /// Set the per-attempt timeout.
    pub fn with_attempt_timeout(mut self, timeout: Duration) -> Self {
        self.attempt_timeout = Some(timeout);
        self
    }

    /// Set the total timeout for all attempts.
    pub fn with_total_timeout(mut self, timeout: Duration) -> Self {
        self.total_timeout = Some(timeout);
        self
    }

    /// Enable retrying on authentication failures.
    pub fn retry_auth_failures(mut self, retry: bool) -> Self {
        self.retry_on_auth_failure = retry;
        self
    }

    /// Calculate the delay for a given attempt number.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = match self.strategy {
            BackoffStrategy::Fixed => self.initial_delay,
            BackoffStrategy::Linear => self
                .initial_delay
                .mul_f64(1.0 + (attempt as f64 * (self.multiplier - 1.0))),
            BackoffStrategy::Exponential | BackoffStrategy::ExponentialWithJitter => self
                .initial_delay
                .mul_f64(self.multiplier.powi(attempt as i32)),
            BackoffStrategy::Fibonacci => {
                let fib = fibonacci(attempt);
                self.initial_delay.mul_f64(fib as f64)
            }
        };

        // Cap the delay at max_delay
        let capped_delay = base_delay.min(self.max_delay);

        // Apply jitter
        if self.jitter > 0.0 {
            let jitter_range = capped_delay.as_secs_f64() * self.jitter;
            let jitter_value = rand::thread_rng().gen_range(-jitter_range..=jitter_range);
            let jittered_secs = (capped_delay.as_secs_f64() + jitter_value).max(0.0);
            Duration::from_secs_f64(jittered_secs)
        } else {
            capped_delay
        }
    }

    /// Check if an error is retryable according to this policy.
    pub fn is_retryable(&self, error: &ConnectionError) -> bool {
        match error {
            ConnectionError::AuthenticationFailed(_) => self.retry_on_auth_failure,
            ConnectionError::Timeout(_) => self.retry_on_timeout,
            ConnectionError::ConnectionFailed(_) => true,
            ConnectionError::ConnectionClosed => true,
            ConnectionError::PoolExhausted => true,
            ConnectionError::SshError(_) => true,
            ConnectionError::IoError(_) => true,
            // Non-retryable errors
            ConnectionError::HostNotFound(_) => false,
            ConnectionError::InvalidConfig(_) => false,
            ConnectionError::UnsupportedOperation(_) => false,
            // Potentially retryable
            ConnectionError::ExecutionFailed(_) => false,
            ConnectionError::TransferFailed(_) => false,
            ConnectionError::DockerError(_) => false,
            ConnectionError::KubernetesError(_) => false,
        }
    }
}

/// Calculate the nth Fibonacci number (capped for practical use).
fn fibonacci(n: u32) -> u64 {
    if n == 0 {
        return 1;
    }
    if n == 1 {
        return 1;
    }

    let mut a = 1u64;
    let mut b = 1u64;

    for _ in 2..=n.min(50) {
        let temp = a.saturating_add(b);
        a = b;
        b = temp;
    }

    b
}

// ============================================================================
// Retry State and Statistics
// ============================================================================

/// Statistics about retry attempts.
#[derive(Debug, Clone, Default)]
pub struct RetryStats {
    /// Total number of attempts made.
    pub total_attempts: u32,
    /// Number of successful attempts (should be 0 or 1).
    pub successful_attempts: u32,
    /// Number of failed attempts.
    pub failed_attempts: u32,
    /// Total time spent on retries.
    pub total_duration: Duration,
    /// Time spent waiting between retries.
    pub wait_duration: Duration,
    /// The errors encountered during retries.
    pub errors: Vec<String>,
}

impl RetryStats {
    /// Create new empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful attempt.
    pub fn record_success(&mut self, duration: Duration) {
        self.total_attempts += 1;
        self.successful_attempts += 1;
        self.total_duration += duration;
    }

    /// Record a failed attempt.
    pub fn record_failure(&mut self, error: &str, duration: Duration) {
        self.total_attempts += 1;
        self.failed_attempts += 1;
        self.total_duration += duration;
        self.errors.push(error.to_string());
    }

    /// Record wait time.
    pub fn record_wait(&mut self, wait: Duration) {
        self.wait_duration += wait;
        self.total_duration += wait;
    }

    /// Check if any retry succeeded.
    pub fn succeeded(&self) -> bool {
        self.successful_attempts > 0
    }
}

// ============================================================================
// Retry Executor
// ============================================================================

/// Result of a retry operation.
#[derive(Debug)]
pub struct RetryResult<T> {
    /// The result of the operation (if successful).
    pub result: Result<T, ConnectionError>,
    /// Statistics about the retry attempts.
    pub stats: RetryStats,
}

impl<T> RetryResult<T> {
    /// Check if the operation succeeded.
    pub fn is_success(&self) -> bool {
        self.result.is_ok()
    }

    /// Get the number of attempts made.
    pub fn attempts(&self) -> u32 {
        self.stats.total_attempts
    }

    /// Unwrap the result or panic.
    pub fn unwrap(self) -> T {
        self.result.unwrap()
    }

    /// Get the result, consuming self.
    pub fn into_result(self) -> Result<T, ConnectionError> {
        self.result
    }
}

/// Execute an operation with retry logic.
///
/// # Arguments
///
/// * `policy` - The retry policy to use.
/// * `operation` - The async operation to execute.
///
/// # Returns
///
/// A `RetryResult` containing the operation result and statistics.
pub async fn retry<T, F, Fut>(policy: &RetryPolicy, mut operation: F) -> RetryResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ConnectionError>>,
{
    let start = std::time::Instant::now();
    let mut stats = RetryStats::new();
    let mut last_error = None;

    for attempt in 0..=policy.max_retries {
        // Check total timeout
        if let Some(total_timeout) = policy.total_timeout {
            if start.elapsed() >= total_timeout {
                debug!(
                    elapsed = ?start.elapsed(),
                    timeout = ?total_timeout,
                    "Total retry timeout exceeded"
                );
                break;
            }
        }

        // Wait before retry (not on first attempt)
        if attempt > 0 {
            let delay = policy.delay_for_attempt(attempt - 1);
            debug!(
                attempt = attempt,
                delay = ?delay,
                "Waiting before retry attempt"
            );
            stats.record_wait(delay);
            tokio::time::sleep(delay).await;
        }

        let attempt_start = std::time::Instant::now();
        trace!(attempt = attempt, "Starting attempt");

        // Execute with optional per-attempt timeout
        let result = if let Some(attempt_timeout) = policy.attempt_timeout {
            match tokio::time::timeout(attempt_timeout, operation()).await {
                Ok(r) => r,
                Err(_) => Err(ConnectionError::Timeout(attempt_timeout.as_secs())),
            }
        } else {
            operation().await
        };

        let attempt_duration = attempt_start.elapsed();

        match result {
            Ok(value) => {
                stats.record_success(attempt_duration);
                debug!(
                    attempts = stats.total_attempts,
                    duration = ?stats.total_duration,
                    "Operation succeeded"
                );
                return RetryResult {
                    result: Ok(value),
                    stats,
                };
            }
            Err(e) => {
                stats.record_failure(&e.to_string(), attempt_duration);

                // Check if error is retryable
                if !policy.is_retryable(&e) {
                    debug!(
                        error = %e,
                        "Non-retryable error encountered, stopping retries"
                    );
                    return RetryResult {
                        result: Err(e),
                        stats,
                    };
                }

                warn!(
                    attempt = attempt,
                    max_retries = policy.max_retries,
                    error = %e,
                    "Attempt failed, will retry"
                );
                last_error = Some(e);
            }
        }
    }

    // All retries exhausted
    let error = last_error
        .unwrap_or_else(|| ConnectionError::ConnectionFailed("Max retries exceeded".to_string()));

    warn!(
        attempts = stats.total_attempts,
        duration = ?stats.total_duration,
        error = %error,
        "All retry attempts exhausted"
    );

    RetryResult {
        result: Err(error),
        stats,
    }
}

/// Execute an operation with retry logic, returning just the result.
///
/// This is a convenience wrapper around `retry` that discards the stats.
pub async fn retry_simple<T, F, Fut>(
    policy: &RetryPolicy,
    operation: F,
) -> Result<T, ConnectionError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ConnectionError>>,
{
    retry(policy, operation).await.into_result()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_default_policy() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.initial_delay, Duration::from_millis(500));
        assert_eq!(policy.strategy, BackoffStrategy::ExponentialWithJitter);
    }

    #[test]
    fn test_no_retry_policy() {
        let policy = RetryPolicy::no_retry();
        assert_eq!(policy.max_retries, 0);
    }

    #[test]
    fn test_delay_calculation_fixed() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Fixed)
            .with_initial_delay(Duration::from_secs(1))
            .with_jitter(0.0);

        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(1));
    }

    #[test]
    fn test_delay_calculation_exponential() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Exponential)
            .with_initial_delay(Duration::from_secs(1))
            .with_multiplier(2.0)
            .with_max_delay(Duration::from_secs(100))
            .with_jitter(0.0);

        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(4));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(8));
    }

    #[test]
    fn test_delay_caps_at_max() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Exponential)
            .with_initial_delay(Duration::from_secs(1))
            .with_multiplier(2.0)
            .with_max_delay(Duration::from_secs(5))
            .with_jitter(0.0);

        assert_eq!(policy.delay_for_attempt(10), Duration::from_secs(5));
    }

    #[test]
    fn test_fibonacci() {
        assert_eq!(fibonacci(0), 1);
        assert_eq!(fibonacci(1), 1);
        assert_eq!(fibonacci(2), 2);
        assert_eq!(fibonacci(3), 3);
        assert_eq!(fibonacci(4), 5);
        assert_eq!(fibonacci(5), 8);
    }

    #[test]
    fn test_retryable_errors() {
        let policy = RetryPolicy::default();

        assert!(policy.is_retryable(&ConnectionError::ConnectionFailed("test".to_string())));
        assert!(policy.is_retryable(&ConnectionError::Timeout(30)));
        assert!(policy.is_retryable(&ConnectionError::ConnectionClosed));
        assert!(!policy.is_retryable(&ConnectionError::InvalidConfig("test".to_string())));
        assert!(!policy.is_retryable(&ConnectionError::AuthenticationFailed("test".to_string())));

        let policy_with_auth = policy.retry_auth_failures(true);
        assert!(policy_with_auth
            .is_retryable(&ConnectionError::AuthenticationFailed("test".to_string())));
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let policy = RetryPolicy::new().with_max_retries(3);

        let result = retry(&policy, || async { Ok::<_, ConnectionError>(42) }).await;

        assert!(result.is_success());
        assert_eq!(result.attempts(), 1);
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let policy = RetryPolicy::new()
            .with_max_retries(3)
            .with_initial_delay(Duration::from_millis(10))
            .with_jitter(0.0);

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = retry(&policy, || {
            let count = counter_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if count < 2 {
                    Err(ConnectionError::ConnectionFailed("temporary".to_string()))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert!(result.is_success());
        assert_eq!(result.attempts(), 3); // 2 failures + 1 success
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_all_attempts_fail() {
        let policy = RetryPolicy::new()
            .with_max_retries(2)
            .with_initial_delay(Duration::from_millis(10))
            .with_jitter(0.0);

        let result = retry(&policy, || async {
            Err::<i32, _>(ConnectionError::ConnectionFailed("permanent".to_string()))
        })
        .await;

        assert!(!result.is_success());
        assert_eq!(result.attempts(), 3); // 1 initial + 2 retries
        assert!(result.stats.errors.len() == 3);
    }

    #[tokio::test]
    async fn test_retry_stops_on_non_retryable() {
        let policy = RetryPolicy::new().with_max_retries(5);

        let result = retry(&policy, || async {
            Err::<i32, _>(ConnectionError::InvalidConfig("bad config".to_string()))
        })
        .await;

        assert!(!result.is_success());
        assert_eq!(result.attempts(), 1); // Should stop immediately
    }
}
