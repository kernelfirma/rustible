//! Retry Policy Module
//!
//! Provides configurable retry strategies with various backoff algorithms:
//!
//! - **Simple**: Fixed number of retries with constant delay
//! - **Exponential Backoff**: Delay doubles with each attempt (with jitter)
//! - **Linear Backoff**: Delay increases linearly
//! - **Fibonacci Backoff**: Delay follows Fibonacci sequence
//! - **Custom**: User-defined retry logic
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::recovery::retry::{RetryPolicy, BackoffStrategy};
//! use std::time::Duration;
//!
//! // Exponential backoff with 5 retries, starting at 1 second
//! let policy = RetryPolicy::exponential_backoff(5, Duration::from_secs(1));
//!
//! // With jitter to prevent thundering herd
//! let policy = RetryPolicy::builder()
//!     .max_retries(5)
//!     .backoff(BackoffStrategy::Exponential { base: Duration::from_secs(1), max: Duration::from_secs(30) })
//!     .jitter(0.25)  // Add up to 25% random jitter
//!     .build();
//! # Ok(())
//! # }
//! ```

use std::time::{Duration, Instant};

use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for retry operations
#[derive(Error, Debug)]
pub enum RetryError {
    #[error("Maximum retries exceeded: {0} attempts")]
    MaxRetriesExceeded(u32),

    #[error("Timeout exceeded")]
    TimeoutExceeded,

    #[error("Non-retryable error: {0}")]
    NonRetryable(String),

    #[error("Operation cancelled")]
    Cancelled,
}

/// Trait for errors that can indicate whether they're retryable
pub trait RetryableError: std::error::Error {
    /// Check if this error is retryable
    fn is_retryable(&self) -> bool;

    /// Get the recommended delay before retry (if any)
    fn retry_after(&self) -> Option<Duration> {
        None
    }
}

/// Default implementation for any error type
impl<E: std::error::Error> RetryableError for E {
    fn is_retryable(&self) -> bool {
        // Default: classify common transient errors as retryable
        let msg = self.to_string().to_lowercase();
        msg.contains("timeout")
            || msg.contains("connection")
            || msg.contains("temporary")
            || msg.contains("unavailable")
            || msg.contains("busy")
            || msg.contains("retry")
            || msg.contains("network")
    }
}

/// Result of a retry attempt
#[derive(Debug, Clone)]
pub enum RetryResult<T, E> {
    /// Operation succeeded
    Ok(T),
    /// Operation failed but can be retried
    Retry(E),
    /// Operation failed and should not be retried
    Stop(E),
}

/// Action to take after an error
#[derive(Debug, Clone)]
pub enum RetryAction {
    /// Retry after the specified delay
    Retry { delay: Duration },
    /// Stop retrying
    Stop { reason: String },
}

/// Context for retry operations
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Name of the operation being retried
    pub operation_name: String,
    /// Current attempt number (0-indexed)
    pub attempt: u32,
    /// Time when the first attempt started
    pub started_at: Instant,
    /// Total time spent so far
    pub elapsed: Duration,
    /// Last error message (if any)
    pub last_error: Option<String>,
    /// Accumulated delay time
    pub total_delay: Duration,
}

impl RetryContext {
    /// Create a new retry context
    pub fn new(operation_name: &str) -> Self {
        Self {
            operation_name: operation_name.to_string(),
            attempt: 0,
            started_at: Instant::now(),
            elapsed: Duration::ZERO,
            last_error: None,
            total_delay: Duration::ZERO,
        }
    }

    /// Record an attempt
    pub fn record_attempt<E: std::error::Error>(&mut self, error: &E) {
        self.attempt += 1;
        self.elapsed = self.started_at.elapsed();
        self.last_error = Some(error.to_string());
    }

    /// Add delay to total
    pub fn add_delay(&mut self, delay: Duration) {
        self.total_delay += delay;
    }
}

/// Backoff strategy for calculating delay between retries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackoffStrategy {
    /// Constant delay between retries
    Constant { delay: Duration },

    /// Linear increase: delay = base * attempt
    Linear { base: Duration, max: Duration },

    /// Exponential increase: delay = base * 2^attempt
    Exponential { base: Duration, max: Duration },

    /// Fibonacci sequence: delay follows fib(attempt)
    Fibonacci { base: Duration, max: Duration },

    /// Decorrelated jitter (AWS-style)
    DecorrelatedJitter { base: Duration, max: Duration },
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(60),
        }
    }
}

impl BackoffStrategy {
    /// Calculate delay for a given attempt
    pub fn delay_for_attempt(&self, attempt: u32, last_delay: Option<Duration>) -> Duration {
        match self {
            BackoffStrategy::Constant { delay } => *delay,

            BackoffStrategy::Linear { base, max } => {
                let delay = *base * (attempt + 1);
                delay.min(*max)
            }

            BackoffStrategy::Exponential { base, max } => {
                // Prevent overflow by capping the exponent
                let exp = attempt.min(30);
                let multiplier = 2u64.saturating_pow(exp);
                let delay = Duration::from_millis(base.as_millis() as u64 * multiplier);
                delay.min(*max)
            }

            BackoffStrategy::Fibonacci { base, max } => {
                let fib = fibonacci(attempt);
                let delay = Duration::from_millis(base.as_millis() as u64 * fib);
                delay.min(*max)
            }

            BackoffStrategy::DecorrelatedJitter { base, max } => {
                // AWS decorrelated jitter: sleep = min(max, random_between(base, sleep * 3))
                let last = last_delay.unwrap_or(*base);
                let mut rng = rand::thread_rng();
                let sleep = rng.gen_range(base.as_millis()..=(last.as_millis() * 3));
                Duration::from_millis(sleep as u64).min(*max)
            }
        }
    }
}

/// Calculate fibonacci number (capped at reasonable value)
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

/// Retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,

    /// Backoff strategy for calculating delays
    pub backoff: BackoffStrategy,

    /// Maximum total time to spend retrying
    pub max_duration: Option<Duration>,

    /// Jitter factor (0.0 to 1.0) to add randomness to delays
    pub jitter: f64,

    /// List of error patterns that should not be retried
    #[serde(default)]
    pub non_retryable_patterns: Vec<String>,

    /// List of error patterns that should always be retried
    #[serde(default)]
    pub retryable_patterns: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff: BackoffStrategy::default(),
            max_duration: Some(Duration::from_secs(300)), // 5 minutes
            jitter: 0.1,                                  // 10% jitter
            non_retryable_patterns: vec![
                "permission denied".to_string(),
                "not found".to_string(),
                "invalid".to_string(),
                "unauthorized".to_string(),
                "forbidden".to_string(),
            ],
            retryable_patterns: vec![
                "timeout".to_string(),
                "connection".to_string(),
                "temporary".to_string(),
                "unavailable".to_string(),
                "busy".to_string(),
            ],
        }
    }
}

impl RetryPolicy {
    /// Create a simple retry policy with constant delay
    pub fn simple(max_retries: u32) -> Self {
        Self {
            max_retries,
            backoff: BackoffStrategy::Constant {
                delay: Duration::from_secs(1),
            },
            ..Default::default()
        }
    }

    /// Create an exponential backoff policy
    pub fn exponential_backoff(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            backoff: BackoffStrategy::Exponential {
                base: base_delay,
                max: Duration::from_secs(60),
            },
            ..Default::default()
        }
    }

    /// Create a linear backoff policy
    pub fn linear_backoff(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            backoff: BackoffStrategy::Linear {
                base: base_delay,
                max: Duration::from_secs(60),
            },
            ..Default::default()
        }
    }

    /// Create a builder for custom configuration
    pub fn builder() -> RetryPolicyBuilder {
        RetryPolicyBuilder::new()
    }

    /// Determine whether to retry based on context and error
    pub fn should_retry<E: RetryableError>(
        &self,
        context: &RetryContext,
        error: &E,
    ) -> RetryAction {
        // Check max retries
        if context.attempt >= self.max_retries {
            return RetryAction::Stop {
                reason: format!("Maximum retries ({}) exceeded", self.max_retries),
            };
        }

        // Check max duration
        if let Some(max_duration) = self.max_duration {
            if context.elapsed >= max_duration {
                return RetryAction::Stop {
                    reason: format!("Maximum duration ({:?}) exceeded", max_duration),
                };
            }
        }

        // Check error patterns
        let error_str = error.to_string().to_lowercase();

        // Check non-retryable patterns first
        for pattern in &self.non_retryable_patterns {
            if error_str.contains(&pattern.to_lowercase()) {
                return RetryAction::Stop {
                    reason: format!("Non-retryable error pattern: {}", pattern),
                };
            }
        }

        // Check if error is retryable
        let is_retryable = error.is_retryable()
            || self
                .retryable_patterns
                .iter()
                .any(|p| error_str.contains(&p.to_lowercase()));

        if !is_retryable {
            return RetryAction::Stop {
                reason: "Error is not retryable".to_string(),
            };
        }

        // Check for retry-after hint from error
        if let Some(retry_after) = error.retry_after() {
            return RetryAction::Retry { delay: retry_after };
        }

        // Calculate delay using backoff strategy
        let base_delay = self.backoff.delay_for_attempt(context.attempt, None);

        // Apply jitter
        let delay = if self.jitter > 0.0 {
            let mut rng = rand::thread_rng();
            let jitter_factor = 1.0 + rng.gen_range(-self.jitter..self.jitter);
            Duration::from_millis((base_delay.as_millis() as f64 * jitter_factor) as u64)
        } else {
            base_delay
        };

        RetryAction::Retry { delay }
    }
}

/// Builder for RetryPolicy
pub struct RetryPolicyBuilder {
    policy: RetryPolicy,
}

impl RetryPolicyBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            policy: RetryPolicy::default(),
        }
    }

    /// Set maximum retries
    pub fn max_retries(mut self, max: u32) -> Self {
        self.policy.max_retries = max;
        self
    }

    /// Set backoff strategy
    pub fn backoff(mut self, strategy: BackoffStrategy) -> Self {
        self.policy.backoff = strategy;
        self
    }

    /// Set maximum duration
    pub fn max_duration(mut self, duration: Duration) -> Self {
        self.policy.max_duration = Some(duration);
        self
    }

    /// Set jitter factor (0.0 to 1.0)
    pub fn jitter(mut self, jitter: f64) -> Self {
        self.policy.jitter = jitter.clamp(0.0, 1.0);
        self
    }

    /// Add a non-retryable pattern
    pub fn non_retryable(mut self, pattern: impl Into<String>) -> Self {
        self.policy.non_retryable_patterns.push(pattern.into());
        self
    }

    /// Add a retryable pattern
    pub fn retryable(mut self, pattern: impl Into<String>) -> Self {
        self.policy.retryable_patterns.push(pattern.into());
        self
    }

    /// Build the policy
    pub fn build(self) -> RetryPolicy {
        self.policy
    }
}

impl Default for RetryPolicyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for retry behavior (simplified alias)
pub type RetryConfig = RetryPolicy;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestError(String);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for TestError {}

    #[test]
    fn test_exponential_backoff() {
        let backoff = BackoffStrategy::Exponential {
            base: Duration::from_millis(100),
            max: Duration::from_secs(10),
        };

        let d0 = backoff.delay_for_attempt(0, None);
        let d1 = backoff.delay_for_attempt(1, None);
        let d2 = backoff.delay_for_attempt(2, None);

        assert_eq!(d0, Duration::from_millis(100));
        assert_eq!(d1, Duration::from_millis(200));
        assert_eq!(d2, Duration::from_millis(400));
    }

    #[test]
    fn test_exponential_backoff_max() {
        let backoff = BackoffStrategy::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(5),
        };

        let d10 = backoff.delay_for_attempt(10, None);
        assert_eq!(d10, Duration::from_secs(5));
    }

    #[test]
    fn test_linear_backoff() {
        let backoff = BackoffStrategy::Linear {
            base: Duration::from_millis(100),
            max: Duration::from_secs(10),
        };

        let d0 = backoff.delay_for_attempt(0, None);
        let d1 = backoff.delay_for_attempt(1, None);
        let d2 = backoff.delay_for_attempt(2, None);

        assert_eq!(d0, Duration::from_millis(100));
        assert_eq!(d1, Duration::from_millis(200));
        assert_eq!(d2, Duration::from_millis(300));
    }

    #[test]
    fn test_fibonacci_backoff() {
        let backoff = BackoffStrategy::Fibonacci {
            base: Duration::from_millis(100),
            max: Duration::from_secs(10),
        };

        let d0 = backoff.delay_for_attempt(0, None);
        let d1 = backoff.delay_for_attempt(1, None);
        let d2 = backoff.delay_for_attempt(2, None);
        let d3 = backoff.delay_for_attempt(3, None);

        assert_eq!(d0, Duration::from_millis(100)); // fib(0) = 1
        assert_eq!(d1, Duration::from_millis(100)); // fib(1) = 1
        assert_eq!(d2, Duration::from_millis(200)); // fib(2) = 2
        assert_eq!(d3, Duration::from_millis(300)); // fib(3) = 3
    }

    #[test]
    fn test_retry_policy_max_retries() {
        let policy = RetryPolicy::simple(3);
        let mut context = RetryContext::new("test");
        let error = TestError("timeout".to_string());

        // Should retry for first 3 attempts
        for _ in 0..3 {
            let action = policy.should_retry(&context, &error);
            assert!(matches!(action, RetryAction::Retry { .. }));
            context.record_attempt(&error);
        }

        // Should stop after 3 attempts
        let action = policy.should_retry(&context, &error);
        assert!(matches!(action, RetryAction::Stop { .. }));
    }

    #[test]
    fn test_retry_policy_non_retryable() {
        let policy = RetryPolicy::default();
        let context = RetryContext::new("test");
        let error = TestError("permission denied".to_string());

        let action = policy.should_retry(&context, &error);
        assert!(matches!(action, RetryAction::Stop { .. }));
    }

    #[test]
    fn test_retry_policy_retryable() {
        let policy = RetryPolicy::default();
        let context = RetryContext::new("test");
        let error = TestError("connection timeout".to_string());

        let action = policy.should_retry(&context, &error);
        assert!(matches!(action, RetryAction::Retry { .. }));
    }

    #[test]
    fn test_retry_policy_builder() {
        let policy = RetryPolicy::builder()
            .max_retries(10)
            .backoff(BackoffStrategy::Constant {
                delay: Duration::from_secs(5),
            })
            .jitter(0.5)
            .non_retryable("fatal")
            .build();

        assert_eq!(policy.max_retries, 10);
        assert!((policy.jitter - 0.5).abs() < f64::EPSILON);
        assert!(policy.non_retryable_patterns.contains(&"fatal".to_string()));
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
}
