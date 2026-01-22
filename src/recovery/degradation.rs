//! Graceful Degradation Module
//!
//! Provides patterns for handling partial failures and service degradation:
//!
//! - **Circuit Breaker**: Prevent cascading failures by cutting off failing services
//! - **Degradation Levels**: Progressive reduction of functionality under stress
//! - **Fallback Actions**: Alternative behaviors when primary path fails
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::recovery::degradation::{CircuitBreaker, CircuitBreakerConfig, GracefulDegradation};
//! # async fn connect_to_host() -> std::io::Result<()> { Ok(()) }
//! # fn use_connection(_conn: ()) {}
//! # fn use_fallback() {}
//! # fn handle_error(_err: std::io::Error) {}
//!
//! let breaker = CircuitBreaker::new("ssh-connection", CircuitBreakerConfig::default());
//!
//! // Execute with circuit breaker protection
//! if breaker.allow_request().await {
//!     match connect_to_host().await {
//!         Ok(conn) => {
//!             breaker.record_success().await;
//!             use_connection(conn);
//!         }
//!         Err(e) => {
//!             breaker.record_failure().await;
//!             if breaker.is_open().await {
//!                 use_fallback();
//!             } else {
//!                 handle_error(e);
//!             }
//!         }
//!     }
//! } else {
//!     use_fallback();
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Level of degradation for the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum DegradationLevel {
    /// Normal operation, all features available
    #[default]
    Normal,
    /// Minor degradation, non-critical features may be limited
    Minor,
    /// Moderate degradation, some features disabled
    Moderate,
    /// Severe degradation, only critical operations available
    Severe,
    /// Critical degradation, minimal functionality
    Critical,
}

/// State of a circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    #[default]
    Closed,
    /// Circuit is open, requests are rejected immediately
    Open,
    /// Circuit is half-open, testing if service recovered
    HalfOpen,
}

/// Configuration for circuit breaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: u32,

    /// Number of successes in half-open state before closing
    pub success_threshold: u32,

    /// Time to wait before transitioning from open to half-open
    pub reset_timeout: Duration,

    /// Time window for counting failures
    pub failure_window: Duration,

    /// Maximum number of requests allowed in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
            half_open_max_requests: 3,
        }
    }
}

/// Circuit breaker for protecting against cascading failures
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    name: String,
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
}

#[derive(Debug)]
struct CircuitBreakerState {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    last_state_change: Instant,
    half_open_requests: u32,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            last_state_change: Instant::now(),
            half_open_requests: 0,
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::default())),
        }
    }

    /// Get the current state
    pub async fn state(&self) -> CircuitState {
        let state = self.state.read().await;
        self.effective_state(&state)
    }

    /// Check if the circuit is open
    pub async fn is_open(&self) -> bool {
        self.state().await == CircuitState::Open
    }

    /// Check if a request should be allowed
    pub async fn allow_request(&self) -> bool {
        let mut state = self.state.write().await;
        let effective = self.effective_state(&state);

        match effective {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                if state.half_open_requests < self.config.half_open_max_requests {
                    state.half_open_requests += 1;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self) {
        let mut state = self.state.write().await;

        // First, check if we need to transition from Open to HalfOpen based on timeout
        if state.state == CircuitState::Open {
            let elapsed = state.last_state_change.elapsed();
            if elapsed >= self.config.reset_timeout {
                // Transition to HalfOpen
                state.state = CircuitState::HalfOpen;
                state.success_count = 0;
                state.half_open_requests = 0;
                state.last_state_change = Instant::now();
            }
        }

        match state.state {
            CircuitState::Closed => {
                // Reset failure count on success
                state.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.config.success_threshold {
                    info!(
                        "Circuit breaker '{}' closing after {} successful requests",
                        self.name, state.success_count
                    );
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                    state.success_count = 0;
                    state.half_open_requests = 0;
                    state.last_state_change = Instant::now();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen after the check above, but handle gracefully
            }
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        let mut state = self.state.write().await;
        let now = Instant::now();

        // Reset count if outside failure window
        if let Some(last_failure) = state.last_failure_time {
            if now.duration_since(last_failure) > self.config.failure_window {
                state.failure_count = 0;
            }
        }

        state.failure_count += 1;
        state.last_failure_time = Some(now);

        match state.state {
            CircuitState::Closed => {
                if state.failure_count >= self.config.failure_threshold {
                    warn!(
                        "Circuit breaker '{}' opening after {} failures",
                        self.name, state.failure_count
                    );
                    state.state = CircuitState::Open;
                    state.last_state_change = Instant::now();
                }
            }
            CircuitState::HalfOpen => {
                warn!(
                    "Circuit breaker '{}' reopening after failure in half-open state",
                    self.name
                );
                state.state = CircuitState::Open;
                state.success_count = 0;
                state.half_open_requests = 0;
                state.last_state_change = Instant::now();
            }
            CircuitState::Open => {
                // Already open
            }
        }
    }

    /// Reset the circuit breaker
    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        *state = CircuitBreakerState::default();
        info!("Circuit breaker '{}' reset", self.name);
    }

    /// Get effective state considering timeout
    fn effective_state(&self, state: &CircuitBreakerState) -> CircuitState {
        if state.state == CircuitState::Open {
            let elapsed = state.last_state_change.elapsed();
            if elapsed >= self.config.reset_timeout {
                return CircuitState::HalfOpen;
            }
        }
        state.state
    }

    /// Get the circuit breaker name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Action to take when degraded
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FallbackAction {
    /// Return a cached result
    UseCached { max_age_secs: u64 },

    /// Return a default value
    UseDefault { value: serde_json::Value },

    /// Skip the operation
    Skip,

    /// Fail immediately
    Fail { message: String },

    /// Try an alternative operation
    Alternative { operation: String },

    /// Queue for later execution
    QueueForLater,
}

/// Policy for graceful degradation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationPolicy {
    /// Thresholds for degradation levels (failure rate percentage)
    pub level_thresholds: Vec<(DegradationLevel, f64)>,

    /// Fallback actions for each degradation level
    pub fallback_actions: HashMap<String, Vec<(DegradationLevel, FallbackAction)>>,

    /// Time window for calculating failure rate
    pub measurement_window: Duration,

    /// Minimum samples before calculating failure rate
    pub min_samples: u32,

    /// How often to recalculate degradation level
    pub recalculation_interval: Duration,
}

impl Default for DegradationPolicy {
    fn default() -> Self {
        Self {
            level_thresholds: vec![
                (DegradationLevel::Minor, 0.1),     // 10% failure rate
                (DegradationLevel::Moderate, 0.25), // 25% failure rate
                (DegradationLevel::Severe, 0.5),    // 50% failure rate
                (DegradationLevel::Critical, 0.75), // 75% failure rate
            ],
            fallback_actions: HashMap::new(),
            measurement_window: Duration::from_secs(60),
            min_samples: 10,
            recalculation_interval: Duration::from_secs(5),
        }
    }
}

/// Manager for graceful degradation
pub struct GracefulDegradation {
    policy: DegradationPolicy,
    metrics: Arc<RwLock<DegradationMetrics>>,
}

#[derive(Debug, Default)]
struct DegradationMetrics {
    /// Failure counts per service
    failures: HashMap<String, Vec<Instant>>,
    /// Success counts per service
    successes: HashMap<String, Vec<Instant>>,
    /// Current degradation level
    current_level: DegradationLevel,
    /// Last recalculation time
    last_recalculation: Option<Instant>,
}

impl GracefulDegradation {
    /// Create a new graceful degradation manager
    pub fn new(policy: DegradationPolicy) -> Self {
        Self {
            policy,
            metrics: Arc::new(RwLock::new(DegradationMetrics::default())),
        }
    }

    /// Report a failure for a service
    pub async fn report_failure(&self, service: &str) {
        let mut metrics = self.metrics.write().await;
        let failures = metrics.failures.entry(service.to_string()).or_default();
        failures.push(Instant::now());

        // Cleanup old entries
        let cutoff = Instant::now() - self.policy.measurement_window;
        failures.retain(|t| *t > cutoff);

        debug!(
            "Recorded failure for service '{}', total in window: {}",
            service,
            failures.len()
        );
    }

    /// Report a success for a service
    pub async fn report_success(&self, service: &str) {
        let mut metrics = self.metrics.write().await;
        let successes = metrics.successes.entry(service.to_string()).or_default();
        successes.push(Instant::now());

        // Cleanup old entries
        let cutoff = Instant::now() - self.policy.measurement_window;
        successes.retain(|t| *t > cutoff);
    }

    /// Get the current degradation level for an operation
    pub async fn current_level(&self, _operation_criticality: u8) -> DegradationLevel {
        let mut metrics = self.metrics.write().await;

        // Check if we need to recalculate
        let should_recalculate = metrics
            .last_recalculation
            .map(|t| t.elapsed() >= self.policy.recalculation_interval)
            .unwrap_or(true);

        if should_recalculate {
            let failure_rate = self.calculate_failure_rate(&metrics);
            metrics.current_level = self.level_for_failure_rate(failure_rate);
            metrics.last_recalculation = Some(Instant::now());
        }

        metrics.current_level
    }

    /// Calculate overall failure rate
    fn calculate_failure_rate(&self, metrics: &DegradationMetrics) -> f64 {
        let cutoff = Instant::now() - self.policy.measurement_window;

        let total_failures: usize = metrics
            .failures
            .values()
            .map(|v| v.iter().filter(|t| **t > cutoff).count())
            .sum();

        let total_successes: usize = metrics
            .successes
            .values()
            .map(|v| v.iter().filter(|t| **t > cutoff).count())
            .sum();

        let total = total_failures + total_successes;

        if total < self.policy.min_samples as usize {
            return 0.0; // Not enough data
        }

        total_failures as f64 / total as f64
    }

    /// Get degradation level for a failure rate
    fn level_for_failure_rate(&self, rate: f64) -> DegradationLevel {
        for (level, threshold) in self.policy.level_thresholds.iter().rev() {
            if rate >= *threshold {
                return *level;
            }
        }
        DegradationLevel::Normal
    }

    /// Get the fallback action for an operation at current degradation level
    pub async fn get_fallback(&self, operation: &str) -> Option<FallbackAction> {
        let metrics = self.metrics.read().await;
        let level = metrics.current_level;

        self.policy
            .fallback_actions
            .get(operation)
            .and_then(|actions| {
                actions
                    .iter()
                    .filter(|(l, _)| *l <= level)
                    .max_by_key(|(l, _)| *l)
                    .map(|(_, action)| action.clone())
            })
    }

    /// Get the current policy
    pub fn policy(&self) -> &DegradationPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());

        assert_eq!(breaker.state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new("test", config);

        // Record failures
        for _ in 0..3 {
            breaker.record_failure().await;
        }

        assert_eq!(breaker.state().await, CircuitState::Open);
        assert!(!breaker.allow_request().await);
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[tokio::test]
    async fn test_circuit_breaker_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let breaker = CircuitBreaker::new("test", config);

        breaker.record_failure().await;
        assert_eq!(breaker.state().await, CircuitState::Open);

        // Wait for reset timeout
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(breaker.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let breaker = CircuitBreaker::new("test", config);

        breaker.record_failure().await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Verify we're in half-open state before proceeding
        assert_eq!(breaker.state().await, CircuitState::HalfOpen);

        // Record successes in half-open state
        breaker.record_success().await;
        breaker.record_success().await;

        assert_eq!(breaker.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_degradation_levels() {
        let degradation = GracefulDegradation::new(DegradationPolicy::default());

        // Report many failures
        for _ in 0..20 {
            degradation.report_failure("service1").await;
        }

        let level = degradation.current_level(5).await;
        assert!(level >= DegradationLevel::Severe);
    }

    #[test]
    fn test_degradation_level_ordering() {
        assert!(DegradationLevel::Normal < DegradationLevel::Minor);
        assert!(DegradationLevel::Minor < DegradationLevel::Moderate);
        assert!(DegradationLevel::Moderate < DegradationLevel::Severe);
        assert!(DegradationLevel::Severe < DegradationLevel::Critical);
    }
}
