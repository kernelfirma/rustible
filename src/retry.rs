//! Retry mechanisms for Rustible task execution.
//!
//! This module provides comprehensive retry functionality with:
//! - Exponential backoff with configurable base and multiplier
//! - Jitter support (full, equal, or decorrelated) to prevent thundering herd
//! - Conditional retry logic based on error type or custom predicates
//! - Max retry limits with timeout constraints
//! - Graceful handling of transient failures
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::retry::{RetryPolicy, BackoffStrategy, JitterStrategy};
//! use std::time::Duration;
//!
//! let policy = RetryPolicy::builder()
//!     .max_retries(5)
//!     .initial_delay(Duration::from_secs(1))
//!     .backoff(BackoffStrategy::Exponential { multiplier: 2.0 })
//!     .jitter(JitterStrategy::Full)
//!     .max_delay(Duration::from_secs(60))
//!     .build();
//!
//! // Use with async operations
//! let result = policy.execute(|| async {
//!     // Your fallible operation here
//!     Ok::<_, Error>(())
//! }).await;
//! # Ok(())
//! # }
//! ```

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

/// Backoff strategy for calculating delay between retries.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Constant delay between retries.
    Constant,

    /// Linear backoff: delay = initial_delay * (attempt + 1)
    Linear,

    /// Exponential backoff: delay = initial_delay * multiplier^attempt
    Exponential {
        /// Multiplier for exponential growth (default: 2.0)
        multiplier: f64,
    },

    /// Fibonacci backoff: delay follows fibonacci sequence
    /// Good for gradual backoff that doesn't grow as fast as exponential
    Fibonacci,

    /// Polynomial backoff: delay = initial_delay * attempt^exponent
    Polynomial {
        /// Exponent for polynomial growth (default: 2.0 for quadratic)
        exponent: f64,
    },
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential { multiplier: 2.0 }
    }
}

impl BackoffStrategy {
    /// Calculate the delay for a given attempt number (0-indexed).
    pub fn calculate_delay(&self, attempt: u32, initial_delay: Duration) -> Duration {
        let base_millis = initial_delay.as_millis() as f64;

        let delay_millis = match self {
            Self::Constant => base_millis,
            Self::Linear => base_millis * (attempt as f64 + 1.0),
            Self::Exponential { multiplier } => base_millis * multiplier.powf(attempt as f64),
            Self::Fibonacci => {
                let fib = fibonacci(attempt + 2);
                base_millis * fib as f64
            }
            Self::Polynomial { exponent } => base_millis * ((attempt as f64 + 1.0).powf(*exponent)),
        };

        Duration::from_millis(delay_millis as u64)
    }
}

/// Calculate the nth fibonacci number (1-indexed).
fn fibonacci(n: u32) -> u64 {
    if n <= 1 {
        return n as u64;
    }

    let mut prev = 0u64;
    let mut curr = 1u64;

    for _ in 2..=n {
        let next = prev.saturating_add(curr);
        prev = curr;
        curr = next;
    }

    curr
}

/// Jitter strategy for adding randomness to delays.
///
/// Jitter helps prevent the "thundering herd" problem where many
/// clients retry at exactly the same time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum JitterStrategy {
    /// No jitter - use exact calculated delay.
    None,

    /// Full jitter: random value between 0 and calculated delay.
    /// delay = random(0, calculated_delay)
    #[default]
    Full,

    /// Equal jitter: half the delay plus random jitter.
    /// delay = calculated_delay/2 + random(0, calculated_delay/2)
    Equal,

    /// Decorrelated jitter: random between base delay and 3x previous delay.
    /// More aggressive randomization that can lead to longer delays.
    Decorrelated,

    /// Bounded jitter: adds random value within a percentage range.
    /// delay = calculated_delay * (1 + random(-percentage, +percentage))
    Bounded {
        /// Percentage of delay to use as jitter range (0.0 to 1.0)
        percentage: f64,
    },
}

impl JitterStrategy {
    /// Apply jitter to a calculated delay.
    ///
    /// # Arguments
    /// * `delay` - The base delay before jitter
    /// * `initial_delay` - The original initial delay (for decorrelated jitter)
    /// * `previous_delay` - The previous delay used (for decorrelated jitter)
    pub fn apply(
        &self,
        delay: Duration,
        initial_delay: Duration,
        previous_delay: Option<Duration>,
    ) -> Duration {
        let mut rng = rand::thread_rng();
        let delay_millis = delay.as_millis() as f64;

        let jittered_millis = match self {
            Self::None => delay_millis,
            Self::Full => {
                if delay_millis > 0.0 {
                    rng.gen_range(0.0..delay_millis)
                } else {
                    0.0
                }
            }
            Self::Equal => {
                let half = delay_millis / 2.0;
                if half > 0.0 {
                    half + rng.gen_range(0.0..half)
                } else {
                    0.0
                }
            }
            Self::Decorrelated => {
                let base = initial_delay.as_millis() as f64;
                let prev = previous_delay.map(|d| d.as_millis() as f64).unwrap_or(base);
                let max = (prev * 3.0).max(base);
                if max > base {
                    rng.gen_range(base..max)
                } else {
                    base
                }
            }
            Self::Bounded { percentage } => {
                let jitter_range = delay_millis * percentage;
                delay_millis + rng.gen_range(-jitter_range..jitter_range)
            }
        };

        Duration::from_millis(jittered_millis.max(0.0) as u64)
    }
}

/// Represents the outcome of a retry decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Should retry the operation.
    Retry,
    /// Should not retry - give up.
    GiveUp,
}

/// Trait for determining if an error should trigger a retry.
pub trait RetryCondition<E>: Send + Sync {
    /// Determine if the given error should trigger a retry.
    fn should_retry(&self, error: &E, attempt: u32) -> RetryDecision;
}

/// Always retry on any error (up to max retries).
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysRetry;

impl<E> RetryCondition<E> for AlwaysRetry {
    fn should_retry(&self, _error: &E, _attempt: u32) -> RetryDecision {
        RetryDecision::Retry
    }
}

/// Never retry - fail immediately.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverRetry;

impl<E> RetryCondition<E> for NeverRetry {
    fn should_retry(&self, _error: &E, _attempt: u32) -> RetryDecision {
        RetryDecision::GiveUp
    }
}

/// Retry based on a predicate function.
pub struct PredicateRetry<F> {
    predicate: F,
}

impl<F, E> RetryCondition<E> for PredicateRetry<F>
where
    F: Fn(&E, u32) -> bool + Send + Sync,
{
    fn should_retry(&self, error: &E, attempt: u32) -> RetryDecision {
        if (self.predicate)(error, attempt) {
            RetryDecision::Retry
        } else {
            RetryDecision::GiveUp
        }
    }
}

/// Retry policy configuration.
///
/// Defines how retries should be performed, including:
/// - Maximum number of retries
/// - Delay between retries
/// - Backoff and jitter strategies
/// - Maximum total timeout
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 means no retries, just the initial attempt).
    pub max_retries: u32,

    /// Initial delay before the first retry.
    pub initial_delay: Duration,

    /// Maximum delay between retries (caps exponential growth).
    pub max_delay: Duration,

    /// Maximum total time to spend retrying (including execution time).
    pub max_total_time: Option<Duration>,

    /// Backoff strategy for calculating delays.
    pub backoff: BackoffStrategy,

    /// Jitter strategy for adding randomness.
    pub jitter: JitterStrategy,

    /// Whether to retry on timeout errors.
    pub retry_on_timeout: bool,

    /// Custom retry condition (checked before backoff calculation).
    condition: Option<RetryConditionFn>,
}

type RetryConditionFn = Box<dyn Fn(&str, u32) -> bool + Send + Sync>;

impl std::fmt::Debug for RetryPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetryPolicy")
            .field("max_retries", &self.max_retries)
            .field("initial_delay", &self.initial_delay)
            .field("max_delay", &self.max_delay)
            .field("max_total_time", &self.max_total_time)
            .field("backoff", &self.backoff)
            .field("jitter", &self.jitter)
            .field("retry_on_timeout", &self.retry_on_timeout)
            .field("condition", &self.condition.is_some())
            .finish()
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_total_time: None,
            backoff: BackoffStrategy::default(),
            jitter: JitterStrategy::default(),
            retry_on_timeout: true,
            condition: None,
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy builder.
    pub fn builder() -> RetryPolicyBuilder {
        RetryPolicyBuilder::new()
    }

    /// Create a policy that never retries.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Create a policy with simple constant delay retries.
    pub fn constant(max_retries: u32, delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay: delay,
            max_delay: delay,
            backoff: BackoffStrategy::Constant,
            jitter: JitterStrategy::None,
            ..Default::default()
        }
    }

    /// Create a policy with exponential backoff.
    pub fn exponential(max_retries: u32, initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay,
            max_delay,
            backoff: BackoffStrategy::Exponential { multiplier: 2.0 },
            jitter: JitterStrategy::Full,
            ..Default::default()
        }
    }

    /// Calculate the delay for a given attempt.
    pub fn delay_for_attempt(&self, attempt: u32, previous_delay: Option<Duration>) -> Duration {
        let base_delay = self.backoff.calculate_delay(attempt, self.initial_delay);
        let capped_delay = base_delay.min(self.max_delay);
        self.jitter
            .apply(capped_delay, self.initial_delay, previous_delay)
    }

    /// Check if retrying should continue based on attempt count.
    pub fn should_continue(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }

    /// Check if an error message matches the retry condition.
    pub fn should_retry_error(&self, error_msg: &str, attempt: u32) -> bool {
        if let Some(ref condition) = self.condition {
            condition(error_msg, attempt)
        } else {
            // Default: retry all errors
            true
        }
    }

    /// Execute an async operation with retry logic.
    ///
    /// Returns the result of the operation, or the last error if all retries fail.
    pub async fn execute<F, Fut, T, E>(&self, mut operation: F) -> Result<T, RetryError<E>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let start_time = std::time::Instant::now();
        let mut attempt = 0;
        let mut previous_delay: Option<Duration> = None;
        let mut last_error: Option<E> = None;

        loop {
            // Check total time limit
            if let Some(max_total) = self.max_total_time {
                if start_time.elapsed() >= max_total {
                    return Err(RetryError::TotalTimeoutExceeded {
                        attempts: attempt,
                        elapsed: start_time.elapsed(),
                        last_error,
                    });
                }
            }

            debug!("Retry attempt {} of {}", attempt + 1, self.max_retries + 1);

            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        debug!("Operation succeeded after {} retry attempts", attempt);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    warn!("Attempt {} failed: {:?}", attempt + 1, e);

                    if !self.should_continue(attempt) {
                        return Err(RetryError::MaxRetriesExceeded {
                            attempts: attempt + 1,
                            last_error: e,
                        });
                    }

                    // Calculate and apply delay
                    let delay = self.delay_for_attempt(attempt, previous_delay);

                    // Check if delay would exceed total time limit
                    if let Some(max_total) = self.max_total_time {
                        let remaining = max_total.saturating_sub(start_time.elapsed());
                        if delay > remaining {
                            return Err(RetryError::TotalTimeoutExceeded {
                                attempts: attempt + 1,
                                elapsed: start_time.elapsed(),
                                last_error: Some(e),
                            });
                        }
                    }

                    debug!("Waiting {:?} before retry", delay);
                    tokio::time::sleep(delay).await;

                    previous_delay = Some(delay);
                    last_error = Some(e);
                    attempt += 1;
                }
            }
        }
    }

    /// Execute an async operation with retry logic and a custom success condition.
    ///
    /// The operation will be retried until:
    /// 1. The success condition returns true, OR
    /// 2. Max retries are exhausted, OR
    /// 3. Total timeout is exceeded
    pub async fn execute_until<F, Fut, T, E, C>(
        &self,
        mut operation: F,
        success_condition: C,
    ) -> Result<T, RetryError<E>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
        C: Fn(&T) -> bool,
    {
        let start_time = std::time::Instant::now();
        let mut attempt = 0;
        let mut previous_delay: Option<Duration> = None;

        loop {
            // Check total time limit
            if let Some(max_total) = self.max_total_time {
                if start_time.elapsed() >= max_total {
                    return Err(RetryError::ConditionNotMet {
                        attempts: attempt,
                        elapsed: start_time.elapsed(),
                    });
                }
            }

            debug!(
                "Retry attempt {} of {} (until condition)",
                attempt + 1,
                self.max_retries + 1
            );

            match operation().await {
                Ok(result) => {
                    if success_condition(&result) {
                        if attempt > 0 {
                            debug!("Condition met after {} retry attempts", attempt);
                        }
                        return Ok(result);
                    }

                    // Condition not met - check if we should retry
                    if !self.should_continue(attempt) {
                        return Err(RetryError::ConditionNotMet {
                            attempts: attempt + 1,
                            elapsed: start_time.elapsed(),
                        });
                    }
                }
                Err(e) => {
                    warn!("Attempt {} failed with error: {:?}", attempt + 1, e);

                    if !self.should_continue(attempt) {
                        return Err(RetryError::MaxRetriesExceeded {
                            attempts: attempt + 1,
                            last_error: e,
                        });
                    }
                }
            }

            // Calculate and apply delay
            let delay = self.delay_for_attempt(attempt, previous_delay);

            // Check if delay would exceed total time limit
            if let Some(max_total) = self.max_total_time {
                let remaining = max_total.saturating_sub(start_time.elapsed());
                if delay > remaining {
                    return Err(RetryError::ConditionNotMet {
                        attempts: attempt + 1,
                        elapsed: start_time.elapsed(),
                    });
                }
            }

            debug!("Waiting {:?} before retry (condition not met)", delay);
            tokio::time::sleep(delay).await;

            previous_delay = Some(delay);
            attempt += 1;
        }
    }
}

/// Builder for constructing RetryPolicy instances.
#[derive(Debug)]
pub struct RetryPolicyBuilder {
    policy: RetryPolicy,
}

impl RetryPolicyBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            policy: RetryPolicy::default(),
        }
    }

    /// Set the maximum number of retries.
    pub fn max_retries(mut self, n: u32) -> Self {
        self.policy.max_retries = n;
        self
    }

    /// Set the initial delay before the first retry.
    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.policy.initial_delay = delay;
        self
    }

    /// Set the maximum delay between retries.
    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.policy.max_delay = delay;
        self
    }

    /// Set the maximum total time for all retry attempts.
    pub fn max_total_time(mut self, timeout: Duration) -> Self {
        self.policy.max_total_time = Some(timeout);
        self
    }

    /// Set the backoff strategy.
    pub fn backoff(mut self, strategy: BackoffStrategy) -> Self {
        self.policy.backoff = strategy;
        self
    }

    /// Set the jitter strategy.
    pub fn jitter(mut self, strategy: JitterStrategy) -> Self {
        self.policy.jitter = strategy;
        self
    }

    /// Set whether to retry on timeout errors.
    pub fn retry_on_timeout(mut self, retry: bool) -> Self {
        self.policy.retry_on_timeout = retry;
        self
    }

    /// Set a custom retry condition based on error message.
    pub fn with_condition<F>(mut self, condition: F) -> Self
    where
        F: Fn(&str, u32) -> bool + Send + Sync + 'static,
    {
        self.policy.condition = Some(Box::new(condition));
        self
    }

    /// Build the RetryPolicy.
    pub fn build(self) -> RetryPolicy {
        self.policy
    }
}

impl Default for RetryPolicyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for retry operations.
#[derive(Debug)]
pub enum RetryError<E> {
    /// Maximum number of retries exceeded.
    MaxRetriesExceeded {
        /// Number of attempts made.
        attempts: u32,
        /// The last error encountered.
        last_error: E,
    },

    /// Total time limit exceeded.
    TotalTimeoutExceeded {
        /// Number of attempts made.
        attempts: u32,
        /// Total elapsed time.
        elapsed: Duration,
        /// The last error encountered (if any).
        last_error: Option<E>,
    },

    /// Success condition was never met.
    ConditionNotMet {
        /// Number of attempts made.
        attempts: u32,
        /// Total elapsed time.
        elapsed: Duration,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for RetryError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetryError::MaxRetriesExceeded {
                attempts,
                last_error,
            } => {
                write!(
                    f,
                    "Max retries exceeded after {} attempts. Last error: {}",
                    attempts, last_error
                )
            }
            RetryError::TotalTimeoutExceeded {
                attempts,
                elapsed,
                last_error,
            } => {
                write!(
                    f,
                    "Total timeout exceeded after {} attempts ({:?} elapsed){}",
                    attempts,
                    elapsed,
                    last_error
                        .as_ref()
                        .map(|e| format!(". Last error: {}", e))
                        .unwrap_or_default()
                )
            }
            RetryError::ConditionNotMet { attempts, elapsed } => {
                write!(
                    f,
                    "Success condition not met after {} attempts ({:?} elapsed)",
                    attempts, elapsed
                )
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RetryError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RetryError::MaxRetriesExceeded { last_error, .. } => Some(last_error),
            RetryError::TotalTimeoutExceeded {
                last_error: Some(e),
                ..
            } => Some(e),
            _ => None,
        }
    }
}

/// Retry context that tracks state across retry attempts.
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Current attempt number (0-indexed).
    pub attempt: u32,
    /// Maximum number of retries allowed.
    pub max_retries: u32,
    /// Time when retrying started.
    pub start_time: std::time::Instant,
    /// Delay used for the previous retry (None for first attempt).
    pub previous_delay: Option<Duration>,
    /// Total elapsed time.
    pub elapsed: Duration,
}

impl RetryContext {
    /// Create a new retry context.
    pub fn new(max_retries: u32) -> Self {
        Self {
            attempt: 0,
            max_retries,
            start_time: std::time::Instant::now(),
            previous_delay: None,
            elapsed: Duration::ZERO,
        }
    }

    /// Increment the attempt counter and update elapsed time.
    pub fn next_attempt(&mut self, delay: Duration) {
        self.attempt += 1;
        self.previous_delay = Some(delay);
        self.elapsed = self.start_time.elapsed();
    }

    /// Check if more retries are allowed.
    pub fn can_retry(&self) -> bool {
        self.attempt < self.max_retries
    }

    /// Get the number of remaining retries.
    pub fn remaining_retries(&self) -> u32 {
        self.max_retries.saturating_sub(self.attempt)
    }
}

/// Helper trait for transient error classification.
pub trait TransientError {
    /// Returns true if this error is transient and should be retried.
    fn is_transient(&self) -> bool;
}

/// Common patterns for identifying transient errors.
pub fn is_transient_error_message(msg: &str) -> bool {
    let transient_patterns = [
        "timeout",
        "timed out",
        "connection refused",
        "connection reset",
        "connection closed",
        "temporary failure",
        "temporarily unavailable",
        "try again",
        "service unavailable",
        "too many requests",
        "rate limit",
        "network unreachable",
        "host unreachable",
        "no route to host",
        "broken pipe",
        "resource temporarily unavailable",
        "operation would block",
        "connection aborted",
        "socket hang up",
        "econnreset",
        "econnrefused",
        "etimedout",
        "enetunreach",
        "ehostunreach",
    ];

    let msg_lower = msg.to_lowercase();
    transient_patterns
        .iter()
        .any(|pattern| msg_lower.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_backoff_constant() {
        let strategy = BackoffStrategy::Constant;
        let initial = Duration::from_secs(1);

        assert_eq!(strategy.calculate_delay(0, initial), Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(1, initial), Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(5, initial), Duration::from_secs(1));
    }

    #[test]
    fn test_backoff_linear() {
        let strategy = BackoffStrategy::Linear;
        let initial = Duration::from_secs(1);

        assert_eq!(strategy.calculate_delay(0, initial), Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(1, initial), Duration::from_secs(2));
        assert_eq!(strategy.calculate_delay(2, initial), Duration::from_secs(3));
    }

    #[test]
    fn test_backoff_exponential() {
        let strategy = BackoffStrategy::Exponential { multiplier: 2.0 };
        let initial = Duration::from_secs(1);

        assert_eq!(strategy.calculate_delay(0, initial), Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(1, initial), Duration::from_secs(2));
        assert_eq!(strategy.calculate_delay(2, initial), Duration::from_secs(4));
        assert_eq!(strategy.calculate_delay(3, initial), Duration::from_secs(8));
    }

    #[test]
    fn test_backoff_fibonacci() {
        let strategy = BackoffStrategy::Fibonacci;
        let initial = Duration::from_secs(1);

        // Fibonacci sequence: 1, 1, 2, 3, 5, 8, 13...
        assert_eq!(strategy.calculate_delay(0, initial), Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(1, initial), Duration::from_secs(2));
        assert_eq!(strategy.calculate_delay(2, initial), Duration::from_secs(3));
        assert_eq!(strategy.calculate_delay(3, initial), Duration::from_secs(5));
        assert_eq!(strategy.calculate_delay(4, initial), Duration::from_secs(8));
    }

    #[test]
    fn test_jitter_none() {
        let strategy = JitterStrategy::None;
        let delay = Duration::from_secs(10);
        let initial = Duration::from_secs(1);

        // No jitter should return exact delay
        let result = strategy.apply(delay, initial, None);
        assert_eq!(result, delay);
    }

    #[test]
    fn test_jitter_full_range() {
        let strategy = JitterStrategy::Full;
        let delay = Duration::from_secs(10);
        let initial = Duration::from_secs(1);

        // Full jitter should return value in [0, delay]
        for _ in 0..100 {
            let result = strategy.apply(delay, initial, None);
            assert!(result <= delay);
        }
    }

    #[test]
    fn test_jitter_equal_range() {
        let strategy = JitterStrategy::Equal;
        let delay = Duration::from_secs(10);
        let initial = Duration::from_secs(1);

        // Equal jitter should return value in [delay/2, delay]
        for _ in 0..100 {
            let result = strategy.apply(delay, initial, None);
            assert!(result >= delay / 2);
            assert!(result <= delay);
        }
    }

    #[test]
    fn test_retry_policy_delay_capping() {
        let policy = RetryPolicy {
            max_retries: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff: BackoffStrategy::Exponential { multiplier: 2.0 },
            jitter: JitterStrategy::None,
            ..Default::default()
        };

        // After several retries, delay should be capped at max_delay
        let delay = policy.delay_for_attempt(10, None);
        assert!(delay <= Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let policy = RetryPolicy::constant(3, Duration::from_millis(10));
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result: Result<i32, RetryError<&str>> = policy
            .execute(|| {
                let c = counter_clone.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(42)
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_failures() {
        let policy = RetryPolicy::constant(3, Duration::from_millis(10));
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = policy
            .execute(|| {
                let c = counter_clone.clone();
                async move {
                    let attempt = c.fetch_add(1, Ordering::SeqCst);
                    if attempt < 2 {
                        Err("transient error")
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let policy = RetryPolicy::constant(2, Duration::from_millis(10));
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result: Result<i32, RetryError<&str>> = policy
            .execute(|| {
                let c = counter_clone.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err("persistent error")
                }
            })
            .await;

        assert!(matches!(
            result,
            Err(RetryError::MaxRetriesExceeded { attempts: 3, .. })
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_transient_error_detection() {
        assert!(is_transient_error_message("Connection timeout occurred"));
        assert!(is_transient_error_message("connection refused by server"));
        assert!(is_transient_error_message(
            "Service temporarily unavailable"
        ));
        assert!(is_transient_error_message(
            "Too many requests - rate limited"
        ));
        assert!(!is_transient_error_message("Invalid input provided"));
        assert!(!is_transient_error_message("File not found"));
    }

    #[test]
    fn test_fibonacci() {
        assert_eq!(fibonacci(0), 0);
        assert_eq!(fibonacci(1), 1);
        assert_eq!(fibonacci(2), 1);
        assert_eq!(fibonacci(3), 2);
        assert_eq!(fibonacci(4), 3);
        assert_eq!(fibonacci(5), 5);
        assert_eq!(fibonacci(6), 8);
        assert_eq!(fibonacci(10), 55);
    }

    #[test]
    fn test_retry_context() {
        let mut ctx = RetryContext::new(5);
        assert_eq!(ctx.attempt, 0);
        assert!(ctx.can_retry());
        assert_eq!(ctx.remaining_retries(), 5);

        ctx.next_attempt(Duration::from_secs(1));
        assert_eq!(ctx.attempt, 1);
        assert!(ctx.can_retry());
        assert_eq!(ctx.remaining_retries(), 4);

        for _ in 0..4 {
            ctx.next_attempt(Duration::from_secs(1));
        }

        assert_eq!(ctx.attempt, 5);
        assert!(!ctx.can_retry());
        assert_eq!(ctx.remaining_retries(), 0);
    }
}
