//! Adaptive parallelism for host responsiveness
//!
//! This module implements adaptive execution strategies that dynamically adjust
//! parallelism based on host responsiveness, network conditions, and system load.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::connection::ConnectionError;

/// Adaptive parallelism configuration
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    /// Initial parallelism level (number of concurrent hosts)
    pub initial_parallelism: usize,
    /// Minimum parallelism level
    pub min_parallelism: usize,
    /// Maximum parallelism level
    pub max_parallelism: usize,
    /// Response time threshold for scaling up (in milliseconds)
    pub response_threshold_up_ms: u64,
    /// Response time threshold for scaling down (in milliseconds)
    pub response_threshold_down_ms: u64,
    /// Error rate threshold for scaling down (0.0 to 1.0)
    pub error_threshold: f64,
    /// How often to re-evaluate parallelism
    pub evaluation_interval: Duration,
    /// Weight for recent performance (0.0 to 1.0)
    pub performance_weight: f64,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            initial_parallelism: 10,
            min_parallelism: 1,
            max_parallelism: 50,
            response_threshold_up_ms: 100,
            response_threshold_down_ms: 5000,
            error_threshold: 0.1,
            evaluation_interval: Duration::from_secs(5),
            performance_weight: 0.7,
        }
    }
}

impl AdaptiveConfig {
    /// Create config for high-performance networks
    pub fn high_performance() -> Self {
        Self {
            initial_parallelism: 25,
            min_parallelism: 5,
            max_parallelism: 100,
            response_threshold_up_ms: 50,
            response_threshold_down_ms: 2000,
            error_threshold: 0.05,
            evaluation_interval: Duration::from_secs(3),
            performance_weight: 0.8,
        }
    }

    /// Create config for slow/unreliable networks
    pub fn conservative() -> Self {
        Self {
            initial_parallelism: 5,
            min_parallelism: 1,
            max_parallelism: 15,
            response_threshold_up_ms: 500,
            response_threshold_down_ms: 10000,
            error_threshold: 0.2,
            evaluation_interval: Duration::from_secs(10),
            performance_weight: 0.5,
        }
    }
}

/// Host performance metrics
#[derive(Debug, Clone)]
pub struct HostMetrics {
    /// Host identifier
    pub host: String,
    /// Average response time
    pub avg_response_time: Duration,
    /// Response time samples (for calculating average)
    pub response_samples: Vec<Duration>,
    /// Number of successful operations
    pub success_count: usize,
    /// Number of failed operations
    pub failure_count: usize,
    /// Last operation time
    pub last_operation: Option<Instant>,
    /// Whether host is considered slow
    pub is_slow: bool,
    /// Whether host is considered unreliable
    pub is_unreliable: bool,
}

impl HostMetrics {
    /// Create new host metrics
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            avg_response_time: Duration::ZERO,
            response_samples: Vec::new(),
            success_count: 0,
            failure_count: 0,
            last_operation: None,
            is_slow: false,
            is_unreliable: false,
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self, response_time: Duration) {
        self.success_count += 1;
        self.response_samples.push(response_time);
        self.last_operation = Some(Instant::now());

        // Keep only last 50 samples
        if self.response_samples.len() > 50 {
            self.response_samples.remove(0);
        }

        // Recalculate average
        self.recalculate_average();
    }

    /// Record a failed operation
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_operation = Some(Instant::now());

        // Update reliability status
        let total_ops = self.success_count + self.failure_count;
        if total_ops > 5 {
            let error_rate = self.failure_count as f64 / total_ops as f64;
            self.is_unreliable = error_rate > 0.2;
        }
    }

    /// Get error rate
    pub fn error_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.failure_count as f64 / total as f64
        }
    }

    /// Get total operations
    pub fn total_operations(&self) -> usize {
        self.success_count + self.failure_count
    }

    /// Recalculate average response time
    fn recalculate_average(&mut self) {
        if self.response_samples.is_empty() {
            self.avg_response_time = Duration::ZERO;
            return;
        }

        let total: Duration = self.response_samples.iter().sum();
        self.avg_response_time = total / self.response_samples.len() as u32;

        // Update slow status
        self.is_slow = self.avg_response_time.as_millis() > 2000;
    }
}

/// Adaptive parallelism controller
pub struct AdaptiveController {
    /// Configuration
    config: AdaptiveConfig,
    /// Current parallelism level
    current_parallelism: usize,
    /// Host performance metrics
    host_metrics: HashMap<String, HostMetrics>,
    /// Semaphore for controlling concurrency
    semaphore: Arc<Semaphore>,
    /// Last evaluation time
    last_evaluation: Instant,
    /// Overall success rate
    overall_success_rate: f64,
    /// Overall average response time
    overall_avg_response_time: Duration,
}

impl AdaptiveController {
    /// Create a new adaptive controller
    pub fn new(config: AdaptiveConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.initial_parallelism));
        
        Self {
            config,
            current_parallelism: config.initial_parallelism,
            host_metrics: HashMap::new(),
            semaphore,
            last_evaluation: Instant::now(),
            overall_success_rate: 1.0,
            overall_avg_response_time: Duration::ZERO,
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(AdaptiveConfig::default())
    }

    /// Get current parallelism level
    pub fn current_parallelism(&self) -> usize {
        self.current_parallelism
    }

    /// Get statistics
    pub fn stats(&self) -> AdaptiveStats {
        AdaptiveStats {
            current_parallelism: self.current_parallelism,
            total_hosts: self.host_metrics.len(),
            overall_success_rate: self.overall_success_rate,
            overall_avg_response_time: self.overall_avg_response_time,
            slow_hosts: self.host_metrics.values().filter(|m| m.is_slow).count(),
            unreliable_hosts: self.host_metrics.values().filter(|m| m.is_unreliable).count(),
        }
    }
}

impl Default for AdaptiveController {
    fn default() -> Self {
        Self::new(AdaptiveConfig::default())
    }
}

/// Adaptive controller statistics
#[derive(Debug, Clone)]
pub struct AdaptiveStats {
    /// Current parallelism level
    pub current_parallelism: usize,
    /// Total number of hosts being tracked
    pub total_hosts: usize,
    /// Overall success rate
    pub overall_success_rate: f64,
    /// Overall average response time
    pub overall_avg_response_time: Duration,
    /// Number of slow hosts
    pub slow_hosts: usize,
    /// Number of unreliable hosts
    pub unreliable_hosts: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_config() {
        let config = AdaptiveConfig::high_performance();
        assert_eq!(config.initial_parallelism, 25);
        assert_eq!(config.max_parallelism, 100);
    }

    #[test]
    fn test_host_metrics() {
        let mut metrics = HostMetrics::new("test-host");
        assert_eq!(metrics.total_operations(), 0);
        
        metrics.record_success(Duration::from_millis(100));
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.total_operations(), 1);
    }
}
