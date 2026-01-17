//! Rate limiting utilities for remote operations
//!
//! Provides configurable rate limiting to prevent:
//! - API rate limit exhaustion
//! - DoS on remote systems
//! - Resource exhaustion
//!
//! Uses a token bucket algorithm for smooth rate limiting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::warn;
use std::time::{Duration, Instant};

use super::{SecurityError, SecurityResult};

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum requests per second
    pub requests_per_second: f64,
    /// Maximum burst size (tokens in bucket)
    pub burst_size: u32,
    /// Whether to block or reject when rate limited
    pub blocking: bool,
    /// Maximum time to wait when blocking
    pub max_wait: Duration,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10.0,
            burst_size: 20,
            blocking: true,
            max_wait: Duration::from_secs(30),
        }
    }
}

impl RateLimiterConfig {
    /// Create a config for high-throughput operations
    pub fn high_throughput() -> Self {
        Self {
            requests_per_second: 100.0,
            burst_size: 200,
            blocking: true,
            max_wait: Duration::from_secs(10),
        }
    }

    /// Create a config for API-rate-limited services
    pub fn api_limited() -> Self {
        Self {
            requests_per_second: 1.0,
            burst_size: 5,
            blocking: true,
            max_wait: Duration::from_secs(60),
        }
    }

    /// Create a config for aggressive rate limiting
    pub fn conservative() -> Self {
        Self {
            requests_per_second: 0.5,
            burst_size: 2,
            blocking: true,
            max_wait: Duration::from_secs(120),
        }
    }
}

/// Token bucket implementation for rate limiting
#[derive(Debug)]
struct TokenBucket {
    /// Current number of tokens available
    tokens: f64,
    /// Maximum tokens (burst size)
    max_tokens: f64,
    /// Tokens added per second
    refill_rate: f64,
    /// Last time tokens were refilled
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to consume a token, returning true if successful
    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Time until the next token is available
    fn time_until_available(&mut self) -> Duration {
        self.refill();
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            let needed = 1.0 - self.tokens;
            Duration::from_secs_f64(needed / self.refill_rate)
        }
    }
}

/// Thread-safe rate limiter supporting multiple named limiters
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Map of limiter name to token bucket
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    /// Default configuration
    default_config: RateLimiterConfig,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimiterConfig::default())
    }
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            default_config: config,
        }
    }

    /// Create a global rate limiter (singleton pattern)
    pub fn global() -> &'static RateLimiter {
        use once_cell::sync::Lazy;
        static GLOBAL: Lazy<RateLimiter> = Lazy::new(|| RateLimiter::default());
        &GLOBAL
    }

    fn lock_buckets(&self) -> std::sync::MutexGuard<'_, HashMap<String, TokenBucket>> {
        match self.buckets.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("Rate limiter bucket lock poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }

    /// Get or create a token bucket for the given key
    fn get_or_create_bucket(&self, key: &str) -> TokenBucket {
        let mut buckets = self.lock_buckets();
        if !buckets.contains_key(key) {
            buckets.insert(
                key.to_string(),
                TokenBucket::new(
                    self.default_config.burst_size as f64,
                    self.default_config.requests_per_second,
                ),
            );
        }
        // Clone the bucket state for returning
        let bucket = buckets.get(key).unwrap();
        TokenBucket {
            tokens: bucket.tokens,
            max_tokens: bucket.max_tokens,
            refill_rate: bucket.refill_rate,
            last_refill: bucket.last_refill,
        }
    }

    /// Acquire a permit to make a request, potentially blocking
    ///
    /// # Arguments
    ///
    /// * `key` - Identifier for the rate limit bucket (e.g., host name, API endpoint)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Permit acquired, proceed with request
    /// * `Err(SecurityError::RateLimitExceeded)` - Rate limit exceeded
    pub fn acquire(&self, key: &str) -> SecurityResult<()> {
        let start = Instant::now();

        loop {
            let mut sleep_for = Duration::from_millis(50);
            {
                let mut buckets = self.lock_buckets();

                // Create bucket if it doesn't exist
                if !buckets.contains_key(key) {
                    buckets.insert(
                        key.to_string(),
                        TokenBucket::new(
                            self.default_config.burst_size as f64,
                            self.default_config.requests_per_second,
                        ),
                    );
                }

                let bucket = buckets.get_mut(key).unwrap();

                // Try to consume a token
                if bucket.try_consume() {
                    return Ok(());
                }

                // Check if we should block or reject
                if !self.default_config.blocking {
                    return Err(SecurityError::RateLimitExceeded(format!(
                        "Rate limit exceeded for '{}'",
                        key
                    )));
                }

                // Check if we've exceeded max wait time
                if start.elapsed() >= self.default_config.max_wait {
                    return Err(SecurityError::RateLimitExceeded(format!(
                        "Rate limit wait timeout for '{}'",
                        key
                    )));
                }

                let next = bucket.time_until_available();
                if next > Duration::from_millis(1) {
                    sleep_for = next;
                }
                if sleep_for > Duration::from_millis(50) {
                    sleep_for = Duration::from_millis(50);
                }
            }

            // Wait a bit before retrying
            std::thread::sleep(sleep_for);
        }
    }

    /// Try to acquire a permit without blocking
    ///
    /// # Arguments
    ///
    /// * `key` - Identifier for the rate limit bucket
    ///
    /// # Returns
    ///
    /// * `true` - Permit acquired
    /// * `false` - Rate limited, try again later
    pub fn try_acquire(&self, key: &str) -> bool {
        let mut buckets = self.lock_buckets();

        if !buckets.contains_key(key) {
            buckets.insert(
                key.to_string(),
                TokenBucket::new(
                    self.default_config.burst_size as f64,
                    self.default_config.requests_per_second,
                ),
            );
        }

        buckets.get_mut(key).unwrap().try_consume()
    }

    /// Get the time until the next request is allowed
    pub fn time_until_available(&self, key: &str) -> Duration {
        let mut buckets = self.lock_buckets();

        if !buckets.contains_key(key) {
            return Duration::ZERO;
        }

        buckets.get_mut(key).unwrap().time_until_available()
    }

    /// Reset the rate limiter for a specific key
    pub fn reset(&self, key: &str) {
        let mut buckets = self.lock_buckets();
        buckets.remove(key);
    }

    /// Reset all rate limiters
    pub fn reset_all(&self) {
        let mut buckets = self.lock_buckets();
        buckets.clear();
    }

    /// Get current token count for a key (for monitoring)
    pub fn available_permits(&self, key: &str) -> f64 {
        let mut buckets = self.lock_buckets();

        if !buckets.contains_key(key) {
            return self.default_config.burst_size as f64;
        }

        let bucket = buckets.get_mut(key).unwrap();
        bucket.refill();
        bucket.tokens
    }
}

/// Rate limit guard that automatically releases on drop
pub struct RateLimitGuard<'a> {
    limiter: &'a RateLimiter,
    key: String,
    acquired: bool,
}

impl<'a> RateLimitGuard<'a> {
    /// Create a new rate limit guard
    pub fn new(limiter: &'a RateLimiter, key: impl Into<String>) -> SecurityResult<Self> {
        let key = key.into();
        limiter.acquire(&key)?;
        Ok(Self {
            limiter,
            key,
            acquired: true,
        })
    }
}

impl Drop for RateLimitGuard<'_> {
    fn drop(&mut self) {
        // Nothing to release since we use token bucket
        // But we could add metrics here
        let _ = (self.limiter, &self.key, self.acquired);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_basic() {
        let mut bucket = TokenBucket::new(5.0, 1.0);

        // Should be able to consume 5 tokens immediately
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());

        // Should be empty now
        assert!(!bucket.try_consume());
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(2.0, 10.0); // 10 tokens per second

        // Consume all tokens
        assert!(bucket.try_consume());
        assert!(bucket.try_consume());
        assert!(!bucket.try_consume());

        // Wait for refill
        std::thread::sleep(Duration::from_millis(200));

        // Should have ~2 tokens now
        assert!(bucket.try_consume());
    }

    #[test]
    fn test_rate_limiter_try_acquire() {
        let config = RateLimiterConfig {
            requests_per_second: 100.0,
            burst_size: 2,
            blocking: false,
            max_wait: Duration::from_millis(100),
        };
        let limiter = RateLimiter::new(config);

        // Should acquire first two
        assert!(limiter.try_acquire("test"));
        assert!(limiter.try_acquire("test"));

        // Third should fail (non-blocking)
        assert!(!limiter.try_acquire("test"));
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[test]
    fn test_rate_limiter_blocking() {
        let config = RateLimiterConfig {
            requests_per_second: 100.0,
            burst_size: 1,
            blocking: true,
            max_wait: Duration::from_secs(1),
        };
        let limiter = RateLimiter::new(config);

        // First should succeed immediately
        assert!(limiter.acquire("test").is_ok());

        // Second should block briefly then succeed
        let start = Instant::now();
        assert!(limiter.acquire("test").is_ok());
        assert!(start.elapsed() > Duration::from_millis(5));
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[test]
    fn test_rate_limiter_different_keys() {
        let config = RateLimiterConfig {
            requests_per_second: 100.0,
            burst_size: 1,
            blocking: false,
            max_wait: Duration::from_millis(100),
        };
        let limiter = RateLimiter::new(config);

        // Different keys have separate limits
        assert!(limiter.try_acquire("host1"));
        assert!(limiter.try_acquire("host2"));
        assert!(limiter.try_acquire("host3"));

        // Each key is exhausted separately
        assert!(!limiter.try_acquire("host1"));
        assert!(!limiter.try_acquire("host2"));
        assert!(!limiter.try_acquire("host3"));
    }

    #[test]
    fn test_rate_limiter_reset() {
        let config = RateLimiterConfig {
            requests_per_second: 100.0,
            burst_size: 1,
            blocking: false,
            max_wait: Duration::from_millis(100),
        };
        let limiter = RateLimiter::new(config);

        assert!(limiter.try_acquire("test"));
        assert!(!limiter.try_acquire("test"));

        // Reset should restore tokens
        limiter.reset("test");
        assert!(limiter.try_acquire("test"));
    }
}
