//! Circuit breaker pattern for connection resilience.
//!
//! The circuit breaker pattern prevents cascading failures by detecting
//! repeated failures and temporarily stopping attempts to connect to
//! unhealthy hosts. This reduces load on struggling services and allows
//! them time to recover.
//!
//! # States
//!
//! - **Closed**: Normal operation, requests flow through
//! - **Open**: Circuit is tripped, requests fail immediately
//! - **Half-Open**: Testing if the service has recovered
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::connection::ConnectionResult;
//! use rustible::connection::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
//! # async fn connection_attempt() -> ConnectionResult<()> {
//! #     Ok(())
//! # }
//!
//! let config = CircuitBreakerConfig::default();
//! let breaker = CircuitBreaker::new("host1", config);
//!
//! // Before each connection attempt
//! if breaker.can_attempt() {
//!     match connection_attempt().await {
//!         Ok(_) => breaker.record_success(),
//!         Err(e) => breaker.record_failure(&e),
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::ConnectionError;

// ============================================================================
// Circuit Breaker State
// ============================================================================

/// The current state of a circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow through normally.
    Closed,
    /// Circuit is open, requests fail immediately without attempting.
    Open,
    /// Circuit is testing if the service has recovered.
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "Closed"),
            CircuitState::Open => write!(f, "Open"),
            CircuitState::HalfOpen => write!(f, "Half-Open"),
        }
    }
}

/// Internal numeric representation of circuit state for atomic operations.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum CircuitStateRaw {
    Closed = 0,
    Open = 1,
    HalfOpen = 2,
}

impl From<u32> for CircuitStateRaw {
    fn from(value: u32) -> Self {
        match value {
            0 => CircuitStateRaw::Closed,
            1 => CircuitStateRaw::Open,
            _ => CircuitStateRaw::HalfOpen,
        }
    }
}

impl From<CircuitStateRaw> for CircuitState {
    fn from(value: CircuitStateRaw) -> Self {
        match value {
            CircuitStateRaw::Closed => CircuitState::Closed,
            CircuitStateRaw::Open => CircuitState::Open,
            CircuitStateRaw::HalfOpen => CircuitState::HalfOpen,
        }
    }
}

// ============================================================================
// Circuit Breaker Configuration
// ============================================================================

/// Configuration for circuit breaker behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures to trip the circuit.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,

    /// Number of consecutive successes to close the circuit from half-open.
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,

    /// Time to wait before testing a tripped circuit.
    #[serde(default = "default_reset_timeout")]
    #[serde(with = "humantime_serde")]
    pub reset_timeout: Duration,

    /// Time window for counting failures (sliding window).
    #[serde(default = "default_failure_window")]
    #[serde(with = "humantime_serde")]
    pub failure_window: Duration,

    /// Whether to count slow responses as failures.
    #[serde(default)]
    pub count_slow_as_failure: bool,

    /// Threshold for considering a response "slow".
    #[serde(default = "default_slow_threshold")]
    #[serde(with = "humantime_serde")]
    pub slow_threshold: Duration,

    /// Maximum number of test requests in half-open state.
    #[serde(default = "default_half_open_max_requests")]
    pub half_open_max_requests: u32,

    /// Percentage of failures to trip the circuit (alternative to consecutive failures).
    #[serde(default)]
    pub failure_rate_threshold: Option<f64>,

    /// Minimum number of requests before failure rate is evaluated.
    #[serde(default = "default_min_requests")]
    pub min_requests_for_rate: u32,
}

fn default_failure_threshold() -> u32 {
    5
}

fn default_success_threshold() -> u32 {
    3
}

fn default_reset_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_failure_window() -> Duration {
    Duration::from_secs(60)
}

fn default_slow_threshold() -> Duration {
    Duration::from_secs(10)
}

fn default_half_open_max_requests() -> u32 {
    3
}

fn default_min_requests() -> u32 {
    10
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: default_failure_threshold(),
            success_threshold: default_success_threshold(),
            reset_timeout: default_reset_timeout(),
            failure_window: default_failure_window(),
            count_slow_as_failure: false,
            slow_threshold: default_slow_threshold(),
            half_open_max_requests: default_half_open_max_requests(),
            failure_rate_threshold: None,
            min_requests_for_rate: default_min_requests(),
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a sensitive configuration that trips quickly.
    pub fn sensitive() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 2,
            reset_timeout: Duration::from_secs(60),
            ..Default::default()
        }
    }

    /// Create a relaxed configuration that's more tolerant of failures.
    pub fn relaxed() -> Self {
        Self {
            failure_threshold: 10,
            success_threshold: 1,
            reset_timeout: Duration::from_secs(15),
            ..Default::default()
        }
    }

    /// Set the failure threshold.
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Set the success threshold for recovery.
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }

    /// Set the reset timeout.
    pub fn with_reset_timeout(mut self, timeout: Duration) -> Self {
        self.reset_timeout = timeout;
        self
    }

    /// Enable counting slow responses as failures.
    pub fn count_slow_responses(mut self, slow_threshold: Duration) -> Self {
        self.count_slow_as_failure = true;
        self.slow_threshold = slow_threshold;
        self
    }

    /// Set failure rate threshold (0.0 to 1.0).
    pub fn with_failure_rate_threshold(mut self, rate: f64) -> Self {
        self.failure_rate_threshold = Some(rate.clamp(0.0, 1.0));
        self
    }
}

// ============================================================================
// Circuit Breaker Implementation
// ============================================================================

/// A circuit breaker for a single connection endpoint.
pub struct CircuitBreaker {
    /// Identifier for this circuit breaker (e.g., host address).
    identifier: String,
    /// Configuration.
    config: CircuitBreakerConfig,
    /// Current state (atomic for lock-free reads).
    state: AtomicU32,
    /// Consecutive failure count.
    failure_count: AtomicU32,
    /// Consecutive success count (for half-open recovery).
    success_count: AtomicU32,
    /// Total requests in current window.
    total_requests: AtomicU32,
    /// Failed requests in current window.
    failed_requests: AtomicU32,
    /// Time when circuit was opened (nanos since epoch).
    opened_at: AtomicU64,
    /// Time of last state transition.
    last_transition: RwLock<Instant>,
    /// Number of requests in half-open state.
    half_open_requests: AtomicU32,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    pub fn new(identifier: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            identifier: identifier.into(),
            config,
            state: AtomicU32::new(CircuitStateRaw::Closed as u32),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            total_requests: AtomicU32::new(0),
            failed_requests: AtomicU32::new(0),
            opened_at: AtomicU64::new(0),
            last_transition: RwLock::new(Instant::now()),
            half_open_requests: AtomicU32::new(0),
        }
    }

    /// Get the identifier for this circuit breaker.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Get the current circuit state.
    pub fn state(&self) -> CircuitState {
        let raw = CircuitStateRaw::from(self.state.load(Ordering::SeqCst));
        self.maybe_transition_to_half_open(raw)
    }

    /// Check if an attempt can be made through this circuit.
    pub fn can_attempt(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                let current = self.half_open_requests.load(Ordering::Relaxed);
                current < self.config.half_open_max_requests
            }
        }
    }

    /// Record a successful operation.
    pub fn record_success(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        match self.state() {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                self.half_open_requests.fetch_add(1, Ordering::Relaxed);
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;

                if successes >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
                debug!(
                    identifier = %self.identifier,
                    "Unexpected success recorded while circuit is open"
                );
            }
        }
    }

    /// Record a failed operation.
    pub fn record_failure(&self, _error: &ConnectionError) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);

        match self.state() {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

                // Check consecutive failure threshold
                if failures >= self.config.failure_threshold {
                    self.transition_to_open();
                    return;
                }

                // Check failure rate threshold
                if let Some(rate_threshold) = self.config.failure_rate_threshold {
                    let total = self.total_requests.load(Ordering::Relaxed);
                    if total >= self.config.min_requests_for_rate {
                        let failed = self.failed_requests.load(Ordering::Relaxed);
                        let rate = failed as f64 / total as f64;
                        if rate >= rate_threshold {
                            self.transition_to_open();
                        }
                    }
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state trips the circuit again
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Circuit is already open, nothing to do
            }
        }
    }

    /// Record a slow response (optionally counts as failure).
    pub fn record_slow(&self, duration: Duration) {
        if self.config.count_slow_as_failure && duration >= self.config.slow_threshold {
            self.record_failure(&ConnectionError::Timeout(duration.as_secs()));
        }
    }

    /// Manually trip the circuit breaker.
    pub fn trip(&self) {
        self.transition_to_open();
    }

    /// Manually reset the circuit breaker.
    pub fn reset(&self) {
        self.transition_to_closed();
    }

    /// Get statistics about this circuit breaker.
    pub fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            identifier: self.identifier.clone(),
            state: self.state(),
            failure_count: self.failure_count.load(Ordering::Relaxed),
            success_count: self.success_count.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            last_transition: *self.last_transition.read(),
        }
    }

    /// Transition to open state.
    fn transition_to_open(&self) {
        let was = CircuitStateRaw::from(
            self.state
                .swap(CircuitStateRaw::Open as u32, Ordering::SeqCst),
        );

        if !matches!(was, CircuitStateRaw::Open) {
            let now_nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;

            self.opened_at.store(now_nanos, Ordering::SeqCst);
            self.success_count.store(0, Ordering::Relaxed);
            self.half_open_requests.store(0, Ordering::Relaxed);
            *self.last_transition.write() = Instant::now();

            warn!(
                identifier = %self.identifier,
                failures = self.failure_count.load(Ordering::Relaxed),
                reset_timeout = ?self.config.reset_timeout,
                "Circuit breaker OPENED"
            );
        }
    }

    /// Transition to closed state.
    fn transition_to_closed(&self) {
        let was = CircuitStateRaw::from(
            self.state
                .swap(CircuitStateRaw::Closed as u32, Ordering::SeqCst),
        );

        if !matches!(was, CircuitStateRaw::Closed) {
            self.failure_count.store(0, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);
            self.total_requests.store(0, Ordering::Relaxed);
            self.failed_requests.store(0, Ordering::Relaxed);
            self.half_open_requests.store(0, Ordering::Relaxed);
            *self.last_transition.write() = Instant::now();

            info!(
                identifier = %self.identifier,
                "Circuit breaker CLOSED"
            );
        }
    }

    /// Check if we should transition from open to half-open.
    fn maybe_transition_to_half_open(&self, current: CircuitStateRaw) -> CircuitState {
        if !matches!(current, CircuitStateRaw::Open) {
            return current.into();
        }

        let opened_at_nanos = self.opened_at.load(Ordering::SeqCst);
        if opened_at_nanos == 0 {
            return CircuitState::Open;
        }

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let elapsed_nanos = now_nanos.saturating_sub(opened_at_nanos);
        let elapsed = Duration::from_nanos(elapsed_nanos);

        if elapsed >= self.config.reset_timeout {
            // Transition to half-open
            if self
                .state
                .compare_exchange(
                    CircuitStateRaw::Open as u32,
                    CircuitStateRaw::HalfOpen as u32,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                self.half_open_requests.store(0, Ordering::Relaxed);
                self.success_count.store(0, Ordering::Relaxed);
                *self.last_transition.write() = Instant::now();

                debug!(
                    identifier = %self.identifier,
                    elapsed = ?elapsed,
                    "Circuit breaker entering HALF-OPEN state"
                );
            }
            return CircuitState::HalfOpen;
        }

        CircuitState::Open
    }
}

/// Statistics about a circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    /// Identifier for this circuit breaker.
    pub identifier: String,
    /// Current state.
    pub state: CircuitState,
    /// Current consecutive failure count.
    pub failure_count: u32,
    /// Current consecutive success count.
    pub success_count: u32,
    /// Total requests in current window.
    pub total_requests: u32,
    /// Failed requests in current window.
    pub failed_requests: u32,
    /// Time of last state transition.
    pub last_transition: Instant,
}

impl CircuitBreakerStats {
    /// Calculate the failure rate.
    pub fn failure_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.failed_requests as f64 / self.total_requests as f64
        }
    }

    /// Check if the circuit is healthy (closed).
    pub fn is_healthy(&self) -> bool {
        matches!(self.state, CircuitState::Closed)
    }
}

// ============================================================================
// Circuit Breaker Registry
// ============================================================================

/// A registry for managing multiple circuit breakers.
pub struct CircuitBreakerRegistry {
    /// Circuit breakers by identifier.
    breakers: RwLock<HashMap<String, Arc<CircuitBreaker>>>,
    /// Default configuration for new breakers.
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    /// Create a new registry with default configuration.
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: RwLock::new(HashMap::new()),
            default_config,
        }
    }

    /// Get or create a circuit breaker for an identifier.
    pub fn get_or_create(&self, identifier: &str) -> Arc<CircuitBreaker> {
        // Try read lock first
        {
            let breakers = self.breakers.read();
            if let Some(breaker) = breakers.get(identifier) {
                return breaker.clone();
            }
        }

        // Need to create a new breaker
        let mut breakers = self.breakers.write();
        breakers
            .entry(identifier.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(identifier, self.default_config.clone()))
            })
            .clone()
    }

    /// Get a circuit breaker by identifier.
    pub fn get(&self, identifier: &str) -> Option<Arc<CircuitBreaker>> {
        self.breakers.read().get(identifier).cloned()
    }

    /// Remove a circuit breaker.
    pub fn remove(&self, identifier: &str) -> Option<Arc<CircuitBreaker>> {
        self.breakers.write().remove(identifier)
    }

    /// Reset all circuit breakers.
    pub fn reset_all(&self) {
        for breaker in self.breakers.read().values() {
            breaker.reset();
        }
    }

    /// Get statistics for all circuit breakers.
    pub fn all_stats(&self) -> Vec<CircuitBreakerStats> {
        self.breakers.read().values().map(|b| b.stats()).collect()
    }

    /// Get identifiers of all open circuits.
    pub fn open_circuits(&self) -> Vec<String> {
        self.breakers
            .read()
            .iter()
            .filter(|(_, b)| matches!(b.state(), CircuitState::Open))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get the number of circuit breakers.
    pub fn len(&self) -> usize {
        self.breakers.read().len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.breakers.read().is_empty()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

// ============================================================================
// Connection Error Extension
// ============================================================================

/// Error returned when circuit breaker is open.
#[derive(Debug)]
pub struct CircuitBreakerOpenError {
    /// Identifier of the circuit breaker.
    pub identifier: String,
    /// Time until the circuit may close.
    pub time_until_retry: Duration,
}

impl std::fmt::Display for CircuitBreakerOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Circuit breaker '{}' is open, retry in {:?}",
            self.identifier, self.time_until_retry
        )
    }
}

impl std::error::Error for CircuitBreakerOpenError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());
    }

    #[test]
    fn test_circuit_trips_after_threshold() {
        let config = CircuitBreakerConfig::new().with_failure_threshold(3);
        let breaker = CircuitBreaker::new("test", config);

        let error = ConnectionError::ConnectionFailed("test".to_string());

        // First two failures don't trip
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Closed);
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Third failure trips the circuit
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_attempt());
    }

    #[test]
    fn test_success_resets_failure_count() {
        let config = CircuitBreakerConfig::new().with_failure_threshold(3);
        let breaker = CircuitBreaker::new("test", config);

        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Two failures
        breaker.record_failure(&error);
        breaker.record_failure(&error);
        assert_eq!(breaker.failure_count.load(Ordering::Relaxed), 2);

        // Success resets count
        breaker.record_success();
        assert_eq!(breaker.failure_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_manual_trip_and_reset() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());

        breaker.trip();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_attempt());

        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());
    }

    #[test]
    fn test_registry() {
        let registry = CircuitBreakerRegistry::default();

        let b1 = registry.get_or_create("host1");
        let b2 = registry.get_or_create("host2");
        let b1_again = registry.get_or_create("host1");

        // Same breaker returned for same identifier
        assert!(Arc::ptr_eq(&b1, &b1_again));
        assert!(!Arc::ptr_eq(&b1, &b2));

        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_failure_rate_threshold() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(100) // High threshold, won't trip
            .with_failure_rate_threshold(0.5); // 50% failure rate

        let config = CircuitBreakerConfig {
            min_requests_for_rate: 4,
            ..config
        };

        let breaker = CircuitBreaker::new("test", config);
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // 2 successes, 2 failures = 50% failure rate
        breaker.record_success();
        breaker.record_success();
        breaker.record_failure(&error);
        breaker.record_failure(&error); // Should trip at 50%

        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_stats() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        let error = ConnectionError::ConnectionFailed("test".to_string());

        breaker.record_success();
        breaker.record_success();
        breaker.record_failure(&error);

        let stats = breaker.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.failed_requests, 1);
        assert!((stats.failure_rate() - 0.333).abs() < 0.01);
    }
}
