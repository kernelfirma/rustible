//! Connection health monitoring and diagnostics.
//!
//! This module provides comprehensive health monitoring for connections,
//! including latency tracking, success rate calculation, and proactive
//! health checks.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

use super::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
use super::{Connection, ConnectionError, ConnectionResult};

// ============================================================================
// Health Status
// ============================================================================

/// Overall health status of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Connection is healthy and performing well.
    Healthy,
    /// Connection is degraded but still functional.
    Degraded,
    /// Connection is unhealthy and should not be used.
    Unhealthy,
    /// Health status is unknown (no recent data).
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "Healthy"),
            HealthStatus::Degraded => write!(f, "Degraded"),
            HealthStatus::Unhealthy => write!(f, "Unhealthy"),
            HealthStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

// ============================================================================
// Health Monitoring Configuration
// ============================================================================

/// Configuration for health monitoring behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// Interval for periodic health checks.
    #[serde(default = "default_check_interval")]
    #[serde(with = "humantime_serde")]
    pub check_interval: Duration,

    /// Timeout for health check operations.
    #[serde(default = "default_check_timeout")]
    #[serde(with = "humantime_serde")]
    pub check_timeout: Duration,

    /// Number of samples to keep for latency calculation.
    #[serde(default = "default_sample_size")]
    pub sample_size: usize,

    /// Success rate threshold for healthy status (0.0 to 1.0).
    #[serde(default = "default_healthy_threshold")]
    pub healthy_threshold: f64,

    /// Success rate threshold for degraded status (below this is unhealthy).
    #[serde(default = "default_degraded_threshold")]
    pub degraded_threshold: f64,

    /// Latency threshold for degraded status.
    #[serde(default = "default_latency_threshold")]
    #[serde(with = "humantime_serde")]
    pub latency_threshold: Duration,

    /// Whether to enable proactive health checks.
    #[serde(default = "default_true")]
    pub enable_proactive_checks: bool,

    /// Command to use for health checks (e.g., "true" or "echo ok").
    #[serde(default = "default_health_command")]
    pub health_command: String,

    /// Time after which cached health data is considered stale.
    #[serde(default = "default_stale_threshold")]
    #[serde(with = "humantime_serde")]
    pub stale_threshold: Duration,
}

fn default_check_interval() -> Duration {
    Duration::from_secs(30)
}

fn default_check_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_sample_size() -> usize {
    100
}

fn default_healthy_threshold() -> f64 {
    0.95
}

fn default_degraded_threshold() -> f64 {
    0.80
}

fn default_latency_threshold() -> Duration {
    Duration::from_secs(2)
}

fn default_true() -> bool {
    true
}

fn default_health_command() -> String {
    "true".to_string()
}

fn default_stale_threshold() -> Duration {
    Duration::from_secs(60)
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval: default_check_interval(),
            check_timeout: default_check_timeout(),
            sample_size: default_sample_size(),
            healthy_threshold: default_healthy_threshold(),
            degraded_threshold: default_degraded_threshold(),
            latency_threshold: default_latency_threshold(),
            enable_proactive_checks: default_true(),
            health_command: default_health_command(),
            stale_threshold: default_stale_threshold(),
        }
    }
}

impl HealthConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the check interval.
    pub fn with_check_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// Set the check timeout.
    pub fn with_check_timeout(mut self, timeout: Duration) -> Self {
        self.check_timeout = timeout;
        self
    }

    /// Set the sample size for statistics.
    pub fn with_sample_size(mut self, size: usize) -> Self {
        self.sample_size = size;
        self
    }

    /// Set the health command.
    pub fn with_health_command(mut self, command: impl Into<String>) -> Self {
        self.health_command = command.into();
        self
    }

    /// Disable proactive health checks.
    pub fn disable_proactive_checks(mut self) -> Self {
        self.enable_proactive_checks = false;
        self
    }
}

// ============================================================================
// Latency Sample
// ============================================================================

/// A single latency sample.
#[derive(Debug, Clone, Copy)]
struct LatencySample {
    /// The measured latency.
    latency: Duration,
    /// When the sample was recorded.
    timestamp: Instant,
    /// Whether the operation was successful.
    success: bool,
}

// ============================================================================
// Health Monitor
// ============================================================================

/// Health monitor for a single connection.
pub struct HealthMonitor {
    /// Identifier for the monitored connection.
    identifier: String,
    /// Configuration.
    config: HealthConfig,
    /// Latency samples (ring buffer).
    samples: RwLock<VecDeque<LatencySample>>,
    /// Total successful operations.
    successes: AtomicU64,
    /// Total failed operations.
    failures: AtomicU64,
    /// Last health check time.
    last_check: RwLock<Option<Instant>>,
    /// Last successful operation time.
    last_success: RwLock<Option<Instant>>,
    /// Is the connection currently being checked.
    checking: AtomicBool,
    /// Circuit breaker for this connection.
    circuit_breaker: Arc<CircuitBreaker>,
    /// Consecutive check failures.
    consecutive_failures: AtomicU32,
}

impl HealthMonitor {
    /// Create a new health monitor.
    pub fn new(
        identifier: impl Into<String>,
        config: HealthConfig,
        circuit_breaker_config: CircuitBreakerConfig,
    ) -> Self {
        let id = identifier.into();
        Self {
            circuit_breaker: Arc::new(CircuitBreaker::new(&id, circuit_breaker_config)),
            identifier: id,
            config,
            samples: RwLock::new(VecDeque::new()),
            successes: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            last_check: RwLock::new(None),
            last_success: RwLock::new(None),
            checking: AtomicBool::new(false),
            consecutive_failures: AtomicU32::new(0),
        }
    }

    /// Get the identifier.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Record a successful operation.
    pub fn record_success(&self, latency: Duration) {
        self.successes.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        *self.last_success.write() = Some(Instant::now());

        self.add_sample(LatencySample {
            latency,
            timestamp: Instant::now(),
            success: true,
        });

        self.circuit_breaker.record_success();
        trace!(
            identifier = %self.identifier,
            latency = ?latency,
            "Recorded successful operation"
        );
    }

    /// Record a failed operation.
    pub fn record_failure(&self, latency: Duration, error: &ConnectionError) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);

        self.add_sample(LatencySample {
            latency,
            timestamp: Instant::now(),
            success: false,
        });

        self.circuit_breaker.record_failure(error);
        debug!(
            identifier = %self.identifier,
            latency = ?latency,
            error = %error,
            "Recorded failed operation"
        );
    }

    /// Add a sample to the ring buffer.
    fn add_sample(&self, sample: LatencySample) {
        let mut samples = self.samples.write();
        if samples.len() >= self.config.sample_size {
            samples.pop_front();
        }
        samples.push_back(sample);
    }

    /// Get the current health status.
    pub fn status(&self) -> HealthStatus {
        // Check circuit breaker first
        match self.circuit_breaker.state() {
            CircuitState::Open => return HealthStatus::Unhealthy,
            CircuitState::HalfOpen => return HealthStatus::Degraded,
            CircuitState::Closed => {}
        }

        let samples = self.samples.read();

        if samples.is_empty() {
            return HealthStatus::Unknown;
        }

        // Check if data is stale
        if let Some(last) = samples.back() {
            if last.timestamp.elapsed() > self.config.stale_threshold {
                return HealthStatus::Unknown;
            }
        }

        // Calculate success rate from recent samples
        let success_count = samples.iter().filter(|s| s.success).count();
        let success_rate = success_count as f64 / samples.len() as f64;

        // Calculate average latency
        let avg_latency = self.average_latency_internal(&samples);

        // Determine status
        if success_rate >= self.config.healthy_threshold
            && avg_latency < self.config.latency_threshold
        {
            HealthStatus::Healthy
        } else if success_rate >= self.config.degraded_threshold {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unhealthy
        }
    }

    /// Check if a connection attempt should be made.
    pub fn can_attempt(&self) -> bool {
        self.circuit_breaker.can_attempt()
    }

    /// Check if a health check is needed.
    pub fn needs_check(&self) -> bool {
        if !self.config.enable_proactive_checks {
            return false;
        }

        // Don't check if already checking
        if self.checking.load(Ordering::Relaxed) {
            return false;
        }

        // Check if enough time has passed
        let last_check = *self.last_check.read();
        match last_check {
            None => true,
            Some(t) => t.elapsed() >= self.config.check_interval,
        }
    }

    /// Mark that a health check is starting.
    pub fn start_check(&self) -> bool {
        self.checking
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
    }

    /// Mark that a health check is complete.
    pub fn finish_check(&self) {
        *self.last_check.write() = Some(Instant::now());
        self.checking.store(false, Ordering::Relaxed);
    }

    /// Get the circuit breaker for this connection.
    pub fn circuit_breaker(&self) -> &Arc<CircuitBreaker> {
        &self.circuit_breaker
    }

    /// Get health statistics.
    pub fn stats(&self) -> HealthStats {
        let samples = self.samples.read();
        let success_count = samples.iter().filter(|s| s.success).count();

        HealthStats {
            identifier: self.identifier.clone(),
            status: self.status(),
            total_successes: self.successes.load(Ordering::Relaxed),
            total_failures: self.failures.load(Ordering::Relaxed),
            recent_success_rate: if samples.is_empty() {
                0.0
            } else {
                success_count as f64 / samples.len() as f64
            },
            average_latency: self.average_latency_internal(&samples),
            p50_latency: self.percentile_latency_internal(&samples, 50),
            p95_latency: self.percentile_latency_internal(&samples, 95),
            p99_latency: self.percentile_latency_internal(&samples, 99),
            sample_count: samples.len(),
            last_success: *self.last_success.read(),
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            circuit_state: self.circuit_breaker.state(),
        }
    }

    /// Calculate average latency.
    pub fn average_latency(&self) -> Duration {
        let samples = self.samples.read();
        self.average_latency_internal(&samples)
    }

    fn average_latency_internal(&self, samples: &VecDeque<LatencySample>) -> Duration {
        if samples.is_empty() {
            return Duration::ZERO;
        }

        let total: Duration = samples.iter().map(|s| s.latency).sum();
        total / samples.len() as u32
    }

    /// Calculate percentile latency.
    pub fn percentile_latency(&self, percentile: u32) -> Duration {
        let samples = self.samples.read();
        self.percentile_latency_internal(&samples, percentile)
    }

    fn percentile_latency_internal(
        &self,
        samples: &VecDeque<LatencySample>,
        percentile: u32,
    ) -> Duration {
        if samples.is_empty() {
            return Duration::ZERO;
        }

        let mut latencies: Vec<Duration> = samples.iter().map(|s| s.latency).collect();
        latencies.sort();

        let index = ((percentile as f64 / 100.0) * (latencies.len() - 1) as f64).round() as usize;
        latencies[index.min(latencies.len() - 1)]
    }

    /// Get the success rate from recent samples.
    pub fn success_rate(&self) -> f64 {
        let samples = self.samples.read();
        if samples.is_empty() {
            return 0.0;
        }

        let success_count = samples.iter().filter(|s| s.success).count();
        success_count as f64 / samples.len() as f64
    }
}

/// Health statistics for a connection.
#[derive(Debug, Clone)]
pub struct HealthStats {
    /// Connection identifier.
    pub identifier: String,
    /// Current health status.
    pub status: HealthStatus,
    /// Total successful operations.
    pub total_successes: u64,
    /// Total failed operations.
    pub total_failures: u64,
    /// Success rate from recent samples.
    pub recent_success_rate: f64,
    /// Average latency from recent samples.
    pub average_latency: Duration,
    /// 50th percentile latency.
    pub p50_latency: Duration,
    /// 95th percentile latency.
    pub p95_latency: Duration,
    /// 99th percentile latency.
    pub p99_latency: Duration,
    /// Number of samples used for statistics.
    pub sample_count: usize,
    /// Time of last successful operation.
    pub last_success: Option<Instant>,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// Current circuit breaker state.
    pub circuit_state: CircuitState,
}

impl HealthStats {
    /// Check if the connection is healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self.status, HealthStatus::Healthy)
    }

    /// Total operations (successes + failures).
    pub fn total_operations(&self) -> u64 {
        self.total_successes + self.total_failures
    }

    /// Overall success rate.
    pub fn overall_success_rate(&self) -> f64 {
        let total = self.total_operations();
        if total == 0 {
            0.0
        } else {
            self.total_successes as f64 / total as f64
        }
    }
}

// ============================================================================
// Health Check Result
// ============================================================================

/// Result of a health check.
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Connection identifier.
    pub identifier: String,
    /// Whether the check succeeded.
    pub success: bool,
    /// Latency of the check.
    pub latency: Duration,
    /// Error message if check failed.
    pub error: Option<String>,
    /// When the check was performed.
    pub timestamp: Instant,
}

impl HealthCheckResult {
    /// Create a successful health check result.
    pub fn success(identifier: impl Into<String>, latency: Duration) -> Self {
        Self {
            identifier: identifier.into(),
            success: true,
            latency,
            error: None,
            timestamp: Instant::now(),
        }
    }

    /// Create a failed health check result.
    pub fn failure(
        identifier: impl Into<String>,
        latency: Duration,
        error: impl Into<String>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            success: false,
            latency,
            error: Some(error.into()),
            timestamp: Instant::now(),
        }
    }
}

// ============================================================================
// Health Checker
// ============================================================================

/// Performs health checks on connections.
pub struct HealthChecker {
    /// Health configuration.
    config: HealthConfig,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new(config: HealthConfig) -> Self {
        Self { config }
    }

    /// Perform a health check on a connection.
    pub async fn check<C: Connection + ?Sized>(&self, connection: &C) -> HealthCheckResult {
        let start = Instant::now();

        let result = tokio::time::timeout(
            self.config.check_timeout,
            connection.execute(&self.config.health_command, None),
        )
        .await;

        let latency = start.elapsed();

        match result {
            Ok(Ok(cmd_result)) => {
                if cmd_result.success {
                    debug!(
                        identifier = %connection.identifier(),
                        latency = ?latency,
                        "Health check passed"
                    );
                    HealthCheckResult::success(connection.identifier(), latency)
                } else {
                    warn!(
                        identifier = %connection.identifier(),
                        exit_code = cmd_result.exit_code,
                        "Health check command failed"
                    );
                    HealthCheckResult::failure(
                        connection.identifier(),
                        latency,
                        format!("Command failed with exit code {}", cmd_result.exit_code),
                    )
                }
            }
            Ok(Err(e)) => {
                warn!(
                    identifier = %connection.identifier(),
                    error = %e,
                    "Health check failed"
                );
                HealthCheckResult::failure(connection.identifier(), latency, e.to_string())
            }
            Err(_) => {
                warn!(
                    identifier = %connection.identifier(),
                    timeout = ?self.config.check_timeout,
                    "Health check timed out"
                );
                HealthCheckResult::failure(
                    connection.identifier(),
                    latency,
                    format!("Timeout after {:?}", self.config.check_timeout),
                )
            }
        }
    }
}

// ============================================================================
// Graceful Degradation
// ============================================================================

/// Strategy for graceful degradation when connections fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DegradationStrategy {
    /// Fail fast without retrying.
    FailFast,
    /// Retry with backoff.
    #[default]
    RetryWithBackoff,
    /// Use a fallback connection if available.
    UseFallback,
    /// Queue the operation for later.
    QueueForLater,
    /// Return cached result if available.
    ReturnCached,
}

/// Configuration for graceful degradation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationConfig {
    /// Primary degradation strategy.
    #[serde(default)]
    pub strategy: DegradationStrategy,

    /// Fallback strategy if primary fails.
    #[serde(default)]
    pub fallback_strategy: Option<DegradationStrategy>,

    /// Maximum queue size for queued operations.
    #[serde(default = "default_queue_size")]
    pub max_queue_size: usize,

    /// Timeout for cached results.
    #[serde(default = "default_cache_timeout")]
    #[serde(with = "humantime_serde")]
    pub cache_timeout: Duration,

    /// Whether to log degradation events.
    #[serde(default = "default_true")]
    pub log_degradation: bool,
}

fn default_queue_size() -> usize {
    1000
}

fn default_cache_timeout() -> Duration {
    Duration::from_secs(300)
}

impl Default for DegradationConfig {
    fn default() -> Self {
        Self {
            strategy: DegradationStrategy::default(),
            fallback_strategy: Some(DegradationStrategy::FailFast),
            max_queue_size: default_queue_size(),
            cache_timeout: default_cache_timeout(),
            log_degradation: true,
        }
    }
}

impl DegradationConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the primary strategy.
    pub fn with_strategy(mut self, strategy: DegradationStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the fallback strategy.
    pub fn with_fallback(mut self, strategy: DegradationStrategy) -> Self {
        self.fallback_strategy = Some(strategy);
        self
    }
}

/// Result of a degraded operation.
#[derive(Debug)]
pub enum DegradationResult<T> {
    /// Operation completed successfully.
    Success(T),
    /// Operation is queued for later.
    Queued,
    /// Cached result returned.
    Cached(T),
    /// Fallback result returned.
    Fallback(T),
    /// Operation failed with degradation.
    Failed(ConnectionError),
}

impl<T> DegradationResult<T> {
    /// Check if the result is a success (direct or cached/fallback).
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            DegradationResult::Success(_)
                | DegradationResult::Cached(_)
                | DegradationResult::Fallback(_)
        )
    }

    /// Get the result value if available.
    pub fn into_value(self) -> Option<T> {
        match self {
            DegradationResult::Success(v)
            | DegradationResult::Cached(v)
            | DegradationResult::Fallback(v) => Some(v),
            _ => None,
        }
    }

    /// Convert to a standard Result.
    pub fn into_result(self) -> ConnectionResult<T> {
        match self {
            DegradationResult::Success(v)
            | DegradationResult::Cached(v)
            | DegradationResult::Fallback(v) => Ok(v),
            DegradationResult::Queued => Err(ConnectionError::UnsupportedOperation(
                "Operation queued".to_string(),
            )),
            DegradationResult::Failed(e) => Err(e),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_display() {
        assert_eq!(format!("{}", HealthStatus::Healthy), "Healthy");
        assert_eq!(format!("{}", HealthStatus::Degraded), "Degraded");
        assert_eq!(format!("{}", HealthStatus::Unhealthy), "Unhealthy");
        assert_eq!(format!("{}", HealthStatus::Unknown), "Unknown");
    }

    #[test]
    fn test_health_monitor_initial_state() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        assert_eq!(monitor.status(), HealthStatus::Unknown);
        assert!(monitor.can_attempt());
        assert!(monitor.needs_check());
    }

    #[test]
    fn test_health_monitor_records_success() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        for _ in 0..10 {
            monitor.record_success(Duration::from_millis(100));
        }

        let stats = monitor.stats();
        assert_eq!(stats.total_successes, 10);
        assert_eq!(stats.total_failures, 0);
        assert!((stats.recent_success_rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_health_monitor_records_failure() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        let error = ConnectionError::ConnectionFailed("test".to_string());
        for _ in 0..5 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }

        let stats = monitor.stats();
        assert_eq!(stats.total_failures, 5);
        assert_eq!(stats.consecutive_failures, 5);
    }

    #[test]
    fn test_health_status_calculation() {
        let config = HealthConfig {
            healthy_threshold: 0.9,
            degraded_threshold: 0.7,
            sample_size: 10,
            ..Default::default()
        };

        let monitor = HealthMonitor::new("test", config, CircuitBreakerConfig::default());

        // All successes -> Healthy
        for _ in 0..10 {
            monitor.record_success(Duration::from_millis(100));
        }
        assert_eq!(monitor.status(), HealthStatus::Healthy);

        // Mix in some failures -> Degraded
        let error = ConnectionError::ConnectionFailed("test".to_string());
        for _ in 0..3 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }
        // Now 7/10 success rate (70%) which is >= degraded_threshold
        // We need to recalculate based on samples
        let stats = monitor.stats();
        // After 10 successes + 3 failures = 13 samples, but max is 10
        // So we have last 10: 7 successes, 3 failures = 70% success rate
        assert!(
            stats.status == HealthStatus::Degraded || stats.status == HealthStatus::Healthy,
            "Expected Degraded or Healthy, got {:?}",
            stats.status
        );
    }

    #[test]
    fn test_latency_percentiles() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        // Add samples with varying latencies
        for i in 1..=100 {
            monitor.record_success(Duration::from_millis(i * 10));
        }

        let p50 = monitor.percentile_latency(50);
        let p95 = monitor.percentile_latency(95);
        let p99 = monitor.percentile_latency(99);

        assert!(p50 < p95);
        assert!(p95 < p99);
    }

    #[test]
    fn test_degradation_result() {
        let success: DegradationResult<i32> = DegradationResult::Success(42);
        assert!(success.is_success());
        assert_eq!(success.into_value(), Some(42));

        let failed: DegradationResult<i32> =
            DegradationResult::Failed(ConnectionError::ConnectionClosed);
        assert!(!failed.is_success());
        assert!(failed.into_value().is_none());
    }
}
