//! Comprehensive tests for connection enhancement features.
//!
//! This module tests advanced connection resilience features:
//! - Circuit breaker pattern for connection failures
//! - Health check system with latency tracking
//! - Jump host (bastion) support for multi-hop SSH
//! - Retry logic with exponential backoff
//!
//! These tests validate the connection layer's ability to handle
//! failures gracefully and maintain reliable connections.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustible::connection::{
    // Retry types
    BackoffStrategy,
    // Circuit breaker types
    CircuitBreaker,
    CircuitBreakerConfig,
    CircuitBreakerRegistry,
    CircuitState,
    // Base types
    ConnectionConfig,
    ConnectionError,
    // Health monitoring types
    DegradationConfig,
    DegradationResult,
    DegradationStrategy,
    HealthConfig,
    HealthMonitor,
    HealthStatus,
    HostConfig,
    // Jump host types
    JumpHostChain,
    JumpHostConfig,
    JumpHostResolver,
    RetryPolicy,
    RetryStats,
    MAX_JUMP_DEPTH,
};

// ============================================================================
// Circuit Breaker Tests
// ============================================================================

mod circuit_breaker_tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_initial_state() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new("test-host", config);

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());
        assert_eq!(breaker.identifier(), "test-host");
    }

    #[test]
    fn test_circuit_breaker_default_config() {
        let config = CircuitBreakerConfig::default();

        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
        assert_eq!(config.reset_timeout, Duration::from_secs(30));
        assert_eq!(config.failure_window, Duration::from_secs(60));
        assert!(!config.count_slow_as_failure);
        assert_eq!(config.half_open_max_requests, 3);
        assert!(config.failure_rate_threshold.is_none());
    }

    #[test]
    fn test_circuit_breaker_sensitive_config() {
        let config = CircuitBreakerConfig::sensitive();

        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.success_threshold, 2);
        assert_eq!(config.reset_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_circuit_breaker_relaxed_config() {
        let config = CircuitBreakerConfig::relaxed();

        assert_eq!(config.failure_threshold, 10);
        assert_eq!(config.success_threshold, 1);
        assert_eq!(config.reset_timeout, Duration::from_secs(15));
    }

    #[test]
    fn test_circuit_breaker_config_builder() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(7)
            .with_success_threshold(5)
            .with_reset_timeout(Duration::from_secs(45));

        assert_eq!(config.failure_threshold, 7);
        assert_eq!(config.success_threshold, 5);
        assert_eq!(config.reset_timeout, Duration::from_secs(45));
    }

    #[test]
    fn test_circuit_breaker_config_slow_responses() {
        let config = CircuitBreakerConfig::new().count_slow_responses(Duration::from_secs(5));

        assert!(config.count_slow_as_failure);
        assert_eq!(config.slow_threshold, Duration::from_secs(5));
    }

    #[test]
    fn test_circuit_breaker_config_failure_rate() {
        let config = CircuitBreakerConfig::new().with_failure_rate_threshold(0.5);

        assert_eq!(config.failure_rate_threshold, Some(0.5));
    }

    #[test]
    fn test_circuit_breaker_config_failure_rate_clamping() {
        let config_high = CircuitBreakerConfig::new().with_failure_rate_threshold(1.5);
        assert_eq!(config_high.failure_rate_threshold, Some(1.0));

        let config_low = CircuitBreakerConfig::new().with_failure_rate_threshold(-0.5);
        assert_eq!(config_low.failure_rate_threshold, Some(0.0));
    }

    #[test]
    fn test_circuit_breaker_trips_after_failures() {
        let config = CircuitBreakerConfig::new().with_failure_threshold(3);
        let breaker = CircuitBreaker::new("test", config);
        let error = ConnectionError::ConnectionFailed("test failure".to_string());

        // First two failures should not trip the circuit
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());

        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());

        // Third failure should trip the circuit
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_attempt());
    }

    #[test]
    fn test_circuit_breaker_success_resets_failure_count() {
        let config = CircuitBreakerConfig::new().with_failure_threshold(3);
        let breaker = CircuitBreaker::new("test", config);
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Two failures
        breaker.record_failure(&error);
        breaker.record_failure(&error);

        // Success should reset the counter
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Two more failures should not trip (we're at 2, not 4)
        breaker.record_failure(&error);
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Third failure after success should not trip
        // Because success reset the count, we need 3 more failures
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_manual_trip() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());

        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.trip();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_attempt());
    }

    #[test]
    fn test_circuit_breaker_manual_reset() {
        let config = CircuitBreakerConfig::new().with_failure_threshold(2);
        let breaker = CircuitBreaker::new("test", config);
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Trip the circuit
        breaker.record_failure(&error);
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Open);

        // Manual reset
        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_attempt());
    }

    #[test]
    fn test_circuit_breaker_failure_rate_threshold() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(100) // High threshold, won't trigger
            .with_failure_rate_threshold(0.5);

        // Lower min_requests for easier testing
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
        breaker.record_failure(&error);

        // Should trip due to failure rate
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_stats() {
        let breaker = CircuitBreaker::new("test-stats", CircuitBreakerConfig::default());
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Record some operations
        breaker.record_success();
        breaker.record_success();
        breaker.record_failure(&error);

        let stats = breaker.stats();
        assert_eq!(stats.identifier, "test-stats");
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.failed_requests, 1);
        assert!(stats.is_healthy());

        // Verify failure rate calculation
        let rate = stats.failure_rate();
        assert!((rate - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_circuit_breaker_stats_failure_rate_zero_requests() {
        let breaker = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        let stats = breaker.stats();

        assert_eq!(stats.failure_rate(), 0.0);
    }

    #[test]
    fn test_circuit_breaker_slow_response_as_failure() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(2)
            .count_slow_responses(Duration::from_millis(100));

        let breaker = CircuitBreaker::new("test", config);

        // Record slow responses that exceed threshold
        breaker.record_slow(Duration::from_millis(150));
        breaker.record_slow(Duration::from_millis(200));

        // Should trip due to slow responses counting as failures
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_slow_response_under_threshold() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(2)
            .count_slow_responses(Duration::from_millis(100));

        let breaker = CircuitBreaker::new("test", config);

        // Record fast responses under threshold
        breaker.record_slow(Duration::from_millis(50));
        breaker.record_slow(Duration::from_millis(80));

        // Should not trip
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(format!("{}", CircuitState::Closed), "Closed");
        assert_eq!(format!("{}", CircuitState::Open), "Open");
        assert_eq!(format!("{}", CircuitState::HalfOpen), "Half-Open");
    }

    // Registry tests
    #[test]
    fn test_circuit_breaker_registry_creation() {
        let registry = CircuitBreakerRegistry::default();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_circuit_breaker_registry_get_or_create() {
        let registry = CircuitBreakerRegistry::default();

        let breaker1 = registry.get_or_create("host1");
        let breaker2 = registry.get_or_create("host2");
        let breaker1_again = registry.get_or_create("host1");

        // Same breaker should be returned for same identifier
        assert!(Arc::ptr_eq(&breaker1, &breaker1_again));
        assert!(!Arc::ptr_eq(&breaker1, &breaker2));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_circuit_breaker_registry_get() {
        let registry = CircuitBreakerRegistry::default();

        // Create a breaker
        let _ = registry.get_or_create("host1");

        // Get should find it
        let breaker = registry.get("host1");
        assert!(breaker.is_some());

        // Get for non-existent should return None
        let missing = registry.get("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_circuit_breaker_registry_remove() {
        let registry = CircuitBreakerRegistry::default();

        let _ = registry.get_or_create("host1");
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("host1");
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_circuit_breaker_registry_reset_all() {
        let registry =
            CircuitBreakerRegistry::new(CircuitBreakerConfig::new().with_failure_threshold(2));

        let breaker1 = registry.get_or_create("host1");
        let breaker2 = registry.get_or_create("host2");
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Trip both breakers
        breaker1.record_failure(&error);
        breaker1.record_failure(&error);
        breaker2.record_failure(&error);
        breaker2.record_failure(&error);

        assert_eq!(breaker1.state(), CircuitState::Open);
        assert_eq!(breaker2.state(), CircuitState::Open);

        // Reset all
        registry.reset_all();

        assert_eq!(breaker1.state(), CircuitState::Closed);
        assert_eq!(breaker2.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_registry_all_stats() {
        let registry = CircuitBreakerRegistry::default();

        let _ = registry.get_or_create("host1");
        let _ = registry.get_or_create("host2");

        let stats = registry.all_stats();
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_circuit_breaker_registry_open_circuits() {
        let registry =
            CircuitBreakerRegistry::new(CircuitBreakerConfig::new().with_failure_threshold(1));

        let breaker1 = registry.get_or_create("host1");
        let _ = registry.get_or_create("host2");
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Trip only breaker1
        breaker1.record_failure(&error);

        let open_circuits = registry.open_circuits();
        assert_eq!(open_circuits.len(), 1);
        assert!(open_circuits.contains(&"host1".to_string()));
    }
}

// ============================================================================
// Health Check System Tests
// ============================================================================

mod health_check_tests {
    use super::*;
    use rustible::connection::circuit_breaker::CircuitBreakerConfig;

    #[test]
    fn test_health_config_default() {
        let config = HealthConfig::default();

        assert_eq!(config.check_interval, Duration::from_secs(30));
        assert_eq!(config.check_timeout, Duration::from_secs(5));
        assert_eq!(config.sample_size, 100);
        assert_eq!(config.healthy_threshold, 0.95);
        assert_eq!(config.degraded_threshold, 0.80);
        assert_eq!(config.latency_threshold, Duration::from_secs(2));
        assert!(config.enable_proactive_checks);
        assert_eq!(config.health_command, "true");
        assert_eq!(config.stale_threshold, Duration::from_secs(60));
    }

    #[test]
    fn test_health_config_builder() {
        let config = HealthConfig::new()
            .with_check_interval(Duration::from_secs(60))
            .with_check_timeout(Duration::from_secs(10))
            .with_sample_size(50)
            .with_health_command("echo ok");

        assert_eq!(config.check_interval, Duration::from_secs(60));
        assert_eq!(config.check_timeout, Duration::from_secs(10));
        assert_eq!(config.sample_size, 50);
        assert_eq!(config.health_command, "echo ok");
    }

    #[test]
    fn test_health_config_disable_proactive_checks() {
        let config = HealthConfig::new().disable_proactive_checks();

        assert!(!config.enable_proactive_checks);
    }

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
            "test-host",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        assert_eq!(monitor.identifier(), "test-host");
        assert_eq!(monitor.status(), HealthStatus::Unknown);
        assert!(monitor.can_attempt());
        assert!(monitor.needs_check());
    }

    #[test]
    fn test_health_monitor_record_success() {
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
        assert_eq!(stats.consecutive_failures, 0);
    }

    #[test]
    fn test_health_monitor_record_failure() {
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
    fn test_health_monitor_status_calculation() {
        let config = HealthConfig {
            healthy_threshold: 0.9,
            degraded_threshold: 0.7,
            sample_size: 10,
            latency_threshold: Duration::from_secs(5),
            ..HealthConfig::default()
        };

        let monitor = HealthMonitor::new("test", config, CircuitBreakerConfig::default());

        // All successes -> Healthy
        for _ in 0..10 {
            monitor.record_success(Duration::from_millis(100));
        }
        assert_eq!(monitor.status(), HealthStatus::Healthy);
    }

    #[test]
    fn test_health_monitor_success_rate() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // 7 successes, 3 failures = 70% success rate
        for _ in 0..7 {
            monitor.record_success(Duration::from_millis(100));
        }
        for _ in 0..3 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }

        let rate = monitor.success_rate();
        assert!((rate - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_health_monitor_average_latency() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        // Record operations with varying latencies: 100, 200, 300ms
        monitor.record_success(Duration::from_millis(100));
        monitor.record_success(Duration::from_millis(200));
        monitor.record_success(Duration::from_millis(300));

        let avg = monitor.average_latency();
        // Average should be 200ms
        assert!((avg.as_millis() as i64 - 200).abs() < 5);
    }

    #[test]
    fn test_health_monitor_percentile_latency() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        // Add 100 samples with increasing latency
        for i in 1..=100 {
            monitor.record_success(Duration::from_millis(i * 10));
        }

        let p50 = monitor.percentile_latency(50);
        let p95 = monitor.percentile_latency(95);
        let p99 = monitor.percentile_latency(99);

        // Verify ordering
        assert!(p50 < p95);
        assert!(p95 < p99);
    }

    #[test]
    fn test_health_monitor_check_state() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::new().with_check_interval(Duration::from_millis(100)),
            CircuitBreakerConfig::default(),
        );

        // Initially needs check
        assert!(monitor.needs_check());

        // Start check
        assert!(monitor.start_check());

        // Should not start another check
        assert!(!monitor.start_check());

        // Finish check
        monitor.finish_check();

        // Just after finishing, should not need check (interval not elapsed)
        assert!(!monitor.needs_check());
    }

    #[test]
    fn test_health_monitor_circuit_breaker_integration() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::new().with_failure_threshold(3),
        );
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Before tripping, can attempt
        assert!(monitor.can_attempt());

        // Record failures to trip circuit breaker
        for _ in 0..3 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }

        // After tripping, cannot attempt
        assert!(!monitor.can_attempt());
        assert_eq!(monitor.status(), HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_stats_is_healthy() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        // Record enough successes
        for _ in 0..100 {
            monitor.record_success(Duration::from_millis(50));
        }

        let stats = monitor.stats();
        assert!(stats.is_healthy());
    }

    #[test]
    fn test_health_stats_total_operations() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );
        let error = ConnectionError::ConnectionFailed("test".to_string());

        monitor.record_success(Duration::from_millis(100));
        monitor.record_success(Duration::from_millis(100));
        monitor.record_failure(Duration::from_millis(100), &error);

        let stats = monitor.stats();
        assert_eq!(stats.total_operations(), 3);
    }

    #[test]
    fn test_health_stats_overall_success_rate() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // 8 successes, 2 failures = 80% success rate
        for _ in 0..8 {
            monitor.record_success(Duration::from_millis(100));
        }
        for _ in 0..2 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }

        let stats = monitor.stats();
        assert!((stats.overall_success_rate() - 0.8).abs() < 0.01);
    }

    // Degradation tests
    #[test]
    fn test_degradation_strategy_default() {
        let strategy = DegradationStrategy::default();
        assert!(matches!(strategy, DegradationStrategy::RetryWithBackoff));
    }

    #[test]
    fn test_degradation_config_default() {
        let config = DegradationConfig::default();

        assert!(matches!(
            config.strategy,
            DegradationStrategy::RetryWithBackoff
        ));
        assert!(matches!(
            config.fallback_strategy,
            Some(DegradationStrategy::FailFast)
        ));
        assert_eq!(config.max_queue_size, 1000);
        assert_eq!(config.cache_timeout, Duration::from_secs(300));
        assert!(config.log_degradation);
    }

    #[test]
    fn test_degradation_config_builder() {
        let config = DegradationConfig::new()
            .with_strategy(DegradationStrategy::UseFallback)
            .with_fallback(DegradationStrategy::QueueForLater);

        assert!(matches!(config.strategy, DegradationStrategy::UseFallback));
        assert!(matches!(
            config.fallback_strategy,
            Some(DegradationStrategy::QueueForLater)
        ));
    }

    #[test]
    fn test_degradation_result_success() {
        let result: DegradationResult<i32> = DegradationResult::Success(42);

        assert!(result.is_success());
        assert_eq!(result.into_value(), Some(42));
    }

    #[test]
    fn test_degradation_result_cached() {
        let result: DegradationResult<i32> = DegradationResult::Cached(42);

        assert!(result.is_success());
        assert_eq!(result.into_value(), Some(42));
    }

    #[test]
    fn test_degradation_result_fallback() {
        let result: DegradationResult<i32> = DegradationResult::Fallback(42);

        assert!(result.is_success());
        assert_eq!(result.into_value(), Some(42));
    }

    #[test]
    fn test_degradation_result_queued() {
        let result: DegradationResult<i32> = DegradationResult::Queued;

        assert!(!result.is_success());
        assert!(result.into_value().is_none());
    }

    #[test]
    fn test_degradation_result_failed() {
        let result: DegradationResult<i32> =
            DegradationResult::Failed(ConnectionError::ConnectionClosed);

        assert!(!result.is_success());
        assert!(result.into_value().is_none());
    }

    #[test]
    fn test_degradation_result_into_result() {
        let success: DegradationResult<i32> = DegradationResult::Success(42);
        assert!(success.into_result().is_ok());

        let failed: DegradationResult<i32> =
            DegradationResult::Failed(ConnectionError::ConnectionClosed);
        assert!(failed.into_result().is_err());
    }
}

// ============================================================================
// Jump Host Tests
// ============================================================================

mod jump_host_tests {
    use super::*;

    #[test]
    fn test_jump_host_config_new() {
        let config = JumpHostConfig::new("bastion.example.com");

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.port, 22);
        assert!(config.user.is_none());
        assert!(config.identity_file.is_none());
    }

    #[test]
    fn test_jump_host_config_builder() {
        let config = JumpHostConfig::new("bastion.example.com")
            .port(2222)
            .user("admin")
            .identity_file("~/.ssh/bastion_key");

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.port, 2222);
        assert_eq!(config.user, Some("admin".to_string()));
        assert_eq!(config.identity_file, Some("~/.ssh/bastion_key".to_string()));
    }

    #[test]
    fn test_jump_host_config_parse_simple() {
        let config = JumpHostConfig::parse("bastion.example.com").unwrap();

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.port, 22);
        assert!(config.user.is_none());
    }

    #[test]
    fn test_jump_host_config_parse_with_port() {
        let config = JumpHostConfig::parse("bastion.example.com:2222").unwrap();

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_jump_host_config_parse_with_user() {
        let config = JumpHostConfig::parse("admin@bastion.example.com").unwrap();

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.user, Some("admin".to_string()));
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_jump_host_config_parse_full() {
        let config = JumpHostConfig::parse("admin@bastion.example.com:2222").unwrap();

        assert_eq!(config.host, "bastion.example.com");
        assert_eq!(config.user, Some("admin".to_string()));
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_jump_host_config_parse_ipv6() {
        let config = JumpHostConfig::parse("[::1]:2222").unwrap();

        assert_eq!(config.host, "::1");
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_jump_host_config_parse_ipv6_no_port() {
        let config = JumpHostConfig::parse("[::1]").unwrap();

        assert_eq!(config.host, "::1");
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_jump_host_config_parse_empty() {
        let result = JumpHostConfig::parse("");

        assert!(result.is_err());
        match result {
            Err(ConnectionError::InvalidConfig(msg)) => {
                assert!(msg.contains("Empty"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[test]
    fn test_jump_host_config_parse_invalid_port() {
        let result = JumpHostConfig::parse("host:notaport");

        assert!(result.is_err());
    }

    #[test]
    fn test_jump_host_config_display() {
        let config = JumpHostConfig::new("bastion.example.com")
            .port(2222)
            .user("admin");

        assert_eq!(config.to_string(), "admin@bastion.example.com:2222");
    }

    #[test]
    fn test_jump_host_config_display_simple() {
        let config = JumpHostConfig::new("bastion.example.com");

        assert_eq!(config.to_string(), "bastion.example.com");
    }

    #[test]
    fn test_jump_host_config_effective_user() {
        let config_with_user = JumpHostConfig::new("host").user("specific");
        assert_eq!(config_with_user.effective_user("default"), "specific");

        let config_without_user = JumpHostConfig::new("host");
        assert_eq!(config_without_user.effective_user("default"), "default");
    }

    #[test]
    fn test_jump_host_config_to_host_config() {
        let jump_config = JumpHostConfig::new("bastion.example.com")
            .port(2222)
            .user("admin")
            .identity_file("/path/to/key");

        let host_config = jump_config.to_host_config();

        assert_eq!(
            host_config.hostname,
            Some("bastion.example.com".to_string())
        );
        assert_eq!(host_config.port, Some(2222));
        assert_eq!(host_config.user, Some("admin".to_string()));
        assert_eq!(host_config.identity_file, Some("/path/to/key".to_string()));
    }

    // Jump Host Chain tests
    #[test]
    fn test_jump_host_chain_new() {
        let chain = JumpHostChain::new();

        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_jump_host_chain_single() {
        let chain = JumpHostChain::single(JumpHostConfig::new("bastion"));

        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
    }

    #[test]
    fn test_jump_host_chain_add_jump() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2"))
            .add_jump(JumpHostConfig::new("jump3"));

        assert_eq!(chain.len(), 3);

        let hosts: Vec<_> = chain.iter().map(|j| j.host.as_str()).collect();
        assert_eq!(hosts, vec!["jump1", "jump2", "jump3"]);
    }

    #[test]
    fn test_jump_host_chain_push() {
        let mut chain = JumpHostChain::new();
        chain.push(JumpHostConfig::new("jump1"));
        chain.push(JumpHostConfig::new("jump2"));

        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_jump_host_chain_parse() {
        let chain = JumpHostChain::parse("jump1,user@jump2:2222,jump3").unwrap();

        assert_eq!(chain.len(), 3);

        let jumps: Vec<_> = chain.iter().collect();
        assert_eq!(jumps[0].host, "jump1");
        assert_eq!(jumps[1].host, "jump2");
        assert_eq!(jumps[1].user, Some("user".to_string()));
        assert_eq!(jumps[1].port, 2222);
        assert_eq!(jumps[2].host, "jump3");
    }

    #[test]
    fn test_jump_host_chain_parse_empty() {
        let chain = JumpHostChain::parse("").unwrap();
        assert!(chain.is_empty());
    }

    #[test]
    fn test_jump_host_chain_parse_none() {
        let chain = JumpHostChain::parse("none").unwrap();
        assert!(chain.is_empty());

        let chain2 = JumpHostChain::parse("NONE").unwrap();
        assert!(chain2.is_empty());
    }

    #[test]
    fn test_jump_host_chain_parse_too_deep() {
        // Create a chain that exceeds MAX_JUMP_DEPTH
        let spec = (0..=MAX_JUMP_DEPTH)
            .map(|i| format!("jump{}", i))
            .collect::<Vec<_>>()
            .join(",");

        let result = JumpHostChain::parse(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_jump_host_chain_validate_loop() {
        let mut chain = JumpHostChain::new();
        chain.push(JumpHostConfig::new("jump1"));
        chain.push(JumpHostConfig::new("jump2"));
        chain.push(JumpHostConfig::new("jump1")); // Loop!

        let result = chain.validate();
        assert!(result.is_err());
        match result {
            Err(ConnectionError::InvalidConfig(msg)) => {
                assert!(msg.contains("Loop"));
            }
            _ => panic!("Expected InvalidConfig error about loop"),
        }
    }

    #[test]
    fn test_jump_host_chain_validate_depth() {
        let mut chain = JumpHostChain::new();
        for i in 0..=MAX_JUMP_DEPTH {
            chain.push(JumpHostConfig::new(format!("jump{}", i)));
        }

        let result = chain.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_jump_host_chain_display() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2").user("admin").port(2222));

        assert_eq!(chain.to_string(), "jump1,admin@jump2:2222");
    }

    #[test]
    fn test_jump_host_chain_as_slice() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2"));

        let slice = chain.as_slice();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].host, "jump1");
    }

    #[test]
    fn test_jump_host_chain_into_iter() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2"));

        let hosts: Vec<_> = chain.into_iter().map(|j| j.host).collect();
        assert_eq!(hosts, vec!["jump1", "jump2"]);
    }

    #[test]
    fn test_max_jump_depth_constant() {
        // Verify the constant is reasonable (compile-time checks)
        const _: () = assert!(MAX_JUMP_DEPTH >= 5);
        const _: () = assert!(MAX_JUMP_DEPTH <= 20);
        let _ = MAX_JUMP_DEPTH;
    }

    // JumpHostResolver tests
    #[test]
    fn test_jump_host_resolver_no_config() {
        let config = ConnectionConfig::default();
        let mut resolver = JumpHostResolver::new(&config);

        let chain = resolver.resolve("unknown-host").unwrap();
        assert!(chain.is_empty());
    }

    #[test]
    fn test_jump_host_resolver_simple() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                proxy_jump: Some("bastion".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();

        assert_eq!(chain.len(), 1);
        assert_eq!(chain.as_slice()[0].host, "bastion");
    }

    #[test]
    fn test_jump_host_resolver_recursive() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                proxy_jump: Some("jump2".to_string()),
                ..Default::default()
            },
        );
        config.hosts.insert(
            "jump2".to_string(),
            HostConfig {
                proxy_jump: Some("jump1".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();

        assert_eq!(chain.len(), 2);
        let hosts: Vec<_> = chain.iter().map(|j| j.host.as_str()).collect();
        assert_eq!(hosts, vec!["jump1", "jump2"]);
    }

    #[test]
    fn test_jump_host_resolver_circular() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "host1".to_string(),
            HostConfig {
                proxy_jump: Some("host2".to_string()),
                ..Default::default()
            },
        );
        config.hosts.insert(
            "host2".to_string(),
            HostConfig {
                proxy_jump: Some("host1".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let result = resolver.resolve("host1");

        assert!(result.is_err());
        match result {
            Err(ConnectionError::InvalidConfig(msg)) => {
                assert!(msg.contains("Circular") || msg.contains("circular"));
            }
            _ => panic!("Expected circular reference error"),
        }
    }

    #[test]
    fn test_jump_host_resolver_no_proxy_jump() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                hostname: Some("target.example.com".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();

        assert!(chain.is_empty());
    }

    #[test]
    fn test_jump_host_resolver_proxy_jump_none() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                proxy_jump: Some("none".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();

        assert!(chain.is_empty());
    }
}

// ============================================================================
// Retry Logic Tests
// ============================================================================

mod retry_tests {
    use super::*;
    use rustible::connection::retry::{retry, retry_simple};

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();

        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.initial_delay, Duration::from_millis(500));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
        assert!(matches!(
            policy.strategy,
            BackoffStrategy::ExponentialWithJitter
        ));
        assert!((policy.multiplier - 2.0).abs() < 0.001);
        assert!((policy.jitter - 0.25).abs() < 0.001);
        assert!(policy.attempt_timeout.is_none());
        assert!(policy.total_timeout.is_none());
        assert!(!policy.retry_on_auth_failure);
        assert!(policy.retry_on_timeout);
    }

    #[test]
    fn test_retry_policy_no_retry() {
        let policy = RetryPolicy::no_retry();

        assert_eq!(policy.max_retries, 0);
    }

    #[test]
    fn test_retry_policy_aggressive() {
        let policy = RetryPolicy::aggressive();

        assert_eq!(policy.max_retries, 5);
        assert_eq!(policy.initial_delay, Duration::from_millis(100));
        assert_eq!(policy.max_delay, Duration::from_secs(10));
        assert!((policy.multiplier - 1.5).abs() < 0.001);
        assert!((policy.jitter - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_retry_policy_conservative() {
        let policy = RetryPolicy::conservative();

        assert_eq!(policy.max_retries, 2);
        assert_eq!(policy.initial_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
        assert!((policy.multiplier - 2.0).abs() < 0.001);
        assert!((policy.jitter - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_retry_policy_builder() {
        let policy = RetryPolicy::new()
            .with_max_retries(5)
            .with_initial_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(60))
            .with_strategy(BackoffStrategy::Linear)
            .with_multiplier(1.5)
            .with_jitter(0.5);

        assert_eq!(policy.max_retries, 5);
        assert_eq!(policy.initial_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(60));
        assert!(matches!(policy.strategy, BackoffStrategy::Linear));
        assert!((policy.multiplier - 1.5).abs() < 0.001);
        assert!((policy.jitter - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_retry_policy_with_timeouts() {
        let policy = RetryPolicy::new()
            .with_attempt_timeout(Duration::from_secs(5))
            .with_total_timeout(Duration::from_secs(30));

        assert_eq!(policy.attempt_timeout, Some(Duration::from_secs(5)));
        assert_eq!(policy.total_timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_retry_policy_retry_auth_failures() {
        let policy = RetryPolicy::new().retry_auth_failures(true);

        assert!(policy.retry_on_auth_failure);
    }

    #[test]
    fn test_retry_policy_jitter_clamping() {
        let policy_high = RetryPolicy::new().with_jitter(1.5);
        assert!((policy_high.jitter - 1.0).abs() < 0.001);

        let policy_low = RetryPolicy::new().with_jitter(-0.5);
        assert!((policy_low.jitter - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_backoff_strategy_default() {
        let strategy = BackoffStrategy::default();
        assert!(matches!(strategy, BackoffStrategy::ExponentialWithJitter));
    }

    #[test]
    fn test_delay_calculation_fixed() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Fixed)
            .with_initial_delay(Duration::from_secs(1))
            .with_jitter(0.0);

        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(10), Duration::from_secs(1));
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
    fn test_delay_calculation_linear() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Linear)
            .with_initial_delay(Duration::from_secs(1))
            .with_multiplier(2.0)
            .with_max_delay(Duration::from_secs(100))
            .with_jitter(0.0);

        let delay0 = policy.delay_for_attempt(0);
        let delay1 = policy.delay_for_attempt(1);
        let delay2 = policy.delay_for_attempt(2);

        // Linear: delay * (1 + attempt * (multiplier - 1))
        // attempt 0: 1 * (1 + 0 * 1) = 1
        // attempt 1: 1 * (1 + 1 * 1) = 2
        // attempt 2: 1 * (1 + 2 * 1) = 3
        assert_eq!(delay0, Duration::from_secs(1));
        assert_eq!(delay1, Duration::from_secs(2));
        assert_eq!(delay2, Duration::from_secs(3));
    }

    #[test]
    fn test_delay_calculation_fibonacci() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Fibonacci)
            .with_initial_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(100))
            .with_jitter(0.0);

        // Fibonacci: 1, 1, 2, 3, 5, 8, ...
        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(3));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(5));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(8));
    }

    #[test]
    fn test_delay_caps_at_max() {
        let policy = RetryPolicy::new()
            .with_strategy(BackoffStrategy::Exponential)
            .with_initial_delay(Duration::from_secs(1))
            .with_multiplier(2.0)
            .with_max_delay(Duration::from_secs(5))
            .with_jitter(0.0);

        // At attempt 10, exponential would be 1024 seconds
        // But should be capped at max_delay (5 seconds)
        assert_eq!(policy.delay_for_attempt(10), Duration::from_secs(5));
    }

    #[test]
    fn test_is_retryable_connection_failed() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::ConnectionFailed("test".to_string());

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_retryable_timeout() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::Timeout(30);

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_retryable_connection_closed() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::ConnectionClosed;

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_retryable_pool_exhausted() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::PoolExhausted;

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_retryable_ssh_error() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::SshError("test".to_string());

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_not_retryable_auth_failure_default() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::AuthenticationFailed("bad password".to_string());

        assert!(!policy.is_retryable(&error));
    }

    #[test]
    fn test_is_retryable_auth_failure_when_enabled() {
        let policy = RetryPolicy::default().retry_auth_failures(true);
        let error = ConnectionError::AuthenticationFailed("bad password".to_string());

        assert!(policy.is_retryable(&error));
    }

    #[test]
    fn test_is_not_retryable_host_not_found() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::HostNotFound("unknown".to_string());

        assert!(!policy.is_retryable(&error));
    }

    #[test]
    fn test_is_not_retryable_invalid_config() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::InvalidConfig("bad config".to_string());

        assert!(!policy.is_retryable(&error));
    }

    #[test]
    fn test_is_not_retryable_unsupported_operation() {
        let policy = RetryPolicy::default();
        let error = ConnectionError::UnsupportedOperation("not supported".to_string());

        assert!(!policy.is_retryable(&error));
    }

    #[test]
    fn test_retry_stats_new() {
        let stats = RetryStats::new();

        assert_eq!(stats.total_attempts, 0);
        assert_eq!(stats.successful_attempts, 0);
        assert_eq!(stats.failed_attempts, 0);
        assert_eq!(stats.total_duration, Duration::ZERO);
        assert_eq!(stats.wait_duration, Duration::ZERO);
        assert!(stats.errors.is_empty());
    }

    #[test]
    fn test_retry_stats_record_success() {
        let mut stats = RetryStats::new();
        stats.record_success(Duration::from_millis(100));

        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.successful_attempts, 1);
        assert_eq!(stats.failed_attempts, 0);
        assert_eq!(stats.total_duration, Duration::from_millis(100));
        assert!(stats.succeeded());
    }

    #[test]
    fn test_retry_stats_record_failure() {
        let mut stats = RetryStats::new();
        stats.record_failure("test error", Duration::from_millis(50));

        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.successful_attempts, 0);
        assert_eq!(stats.failed_attempts, 1);
        assert_eq!(stats.errors.len(), 1);
        assert_eq!(stats.errors[0], "test error");
        assert!(!stats.succeeded());
    }

    #[test]
    fn test_retry_stats_record_wait() {
        let mut stats = RetryStats::new();
        stats.record_wait(Duration::from_millis(500));

        assert_eq!(stats.wait_duration, Duration::from_millis(500));
        assert_eq!(stats.total_duration, Duration::from_millis(500));
    }

    // Async retry tests
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
        assert_eq!(result.stats.errors.len(), 3);
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

    #[tokio::test]
    async fn test_retry_simple() {
        let policy = RetryPolicy::new().with_max_retries(3);

        let result = retry_simple(&policy, || async { Ok::<_, ConnectionError>(42) }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_with_attempt_timeout() {
        let policy = RetryPolicy::new()
            .with_max_retries(1)
            .with_attempt_timeout(Duration::from_millis(100));

        let start = Instant::now();
        let result = retry(&policy, || async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok::<_, ConnectionError>(42)
        })
        .await;

        // Should timeout quickly
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_retry_result_into_result() {
        let policy = RetryPolicy::new();

        let success = retry(&policy, || async { Ok::<_, ConnectionError>(42) }).await;
        assert!(success.into_result().is_ok());

        let failure = retry(&policy, || async {
            Err::<i32, _>(ConnectionError::InvalidConfig("test".to_string()))
        })
        .await;
        assert!(failure.into_result().is_err());
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

mod integration_tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_with_health_monitor() {
        // Create a health monitor that shares circuit breaker state
        let health_config = HealthConfig::default();
        let cb_config = CircuitBreakerConfig::new().with_failure_threshold(3);

        let monitor = HealthMonitor::new("test-host", health_config, cb_config);
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Verify initial state
        assert!(monitor.can_attempt());
        assert_eq!(monitor.status(), HealthStatus::Unknown);

        // Record failures to trip circuit breaker
        for _ in 0..3 {
            monitor.record_failure(Duration::from_millis(100), &error);
        }

        // Verify circuit is open
        assert!(!monitor.can_attempt());
        assert_eq!(monitor.status(), HealthStatus::Unhealthy);

        // Verify circuit breaker is tripped
        let stats = monitor.circuit_breaker().stats();
        assert_eq!(stats.state, CircuitState::Open);
    }

    #[test]
    fn test_jump_host_chain_with_connection_config() {
        let mut config = ConnectionConfig::default();

        // Set up a multi-hop scenario
        config.hosts.insert(
            "internal-server".to_string(),
            HostConfig {
                hostname: Some("10.0.0.100".to_string()),
                proxy_jump: Some("bastion".to_string()),
                user: Some("internal".to_string()),
                ..Default::default()
            },
        );

        config.hosts.insert(
            "bastion".to_string(),
            HostConfig {
                hostname: Some("bastion.example.com".to_string()),
                user: Some("admin".to_string()),
                port: Some(2222),
                ..Default::default()
            },
        );

        // Resolve jump chain
        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("internal-server").unwrap();

        assert_eq!(chain.len(), 1);
        assert_eq!(chain.as_slice()[0].host, "bastion");
    }

    #[tokio::test]
    async fn test_retry_with_circuit_breaker() {
        let cb_config = CircuitBreakerConfig::new().with_failure_threshold(2);
        let breaker = Arc::new(CircuitBreaker::new("test", cb_config));

        let retry_policy = RetryPolicy::new()
            .with_max_retries(5)
            .with_initial_delay(Duration::from_millis(10))
            .with_jitter(0.0);

        let breaker_clone = breaker.clone();

        // This simulates using retry with circuit breaker
        let result = rustible::connection::retry::retry(&retry_policy, || {
            let breaker = breaker_clone.clone();
            async move {
                if !breaker.can_attempt() {
                    return Err(ConnectionError::ConnectionFailed(
                        "Circuit breaker open".to_string(),
                    ));
                }

                // Simulate failure
                let error = ConnectionError::ConnectionFailed("test".to_string());
                breaker.record_failure(&error);
                Err::<i32, _>(error)
            }
        })
        .await;

        // After 2 failures, circuit should be open
        assert!(!result.is_success());
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_registry_with_custom_config() {
        let custom_config = CircuitBreakerConfig::new()
            .with_failure_threshold(10)
            .with_reset_timeout(Duration::from_secs(120));

        let registry = CircuitBreakerRegistry::new(custom_config);

        let breaker = registry.get_or_create("test-host");
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Verify custom config is used
        for _ in 0..9 {
            breaker.record_failure(&error);
        }
        assert_eq!(breaker.state(), CircuitState::Closed);

        // 10th failure should trip
        breaker.record_failure(&error);
        assert_eq!(breaker.state(), CircuitState::Open);
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_empty_identifier() {
        let breaker = CircuitBreaker::new("", CircuitBreakerConfig::default());
        assert_eq!(breaker.identifier(), "");
    }

    #[test]
    fn test_jump_host_parse_whitespace() {
        let config = JumpHostConfig::parse("  bastion.example.com  ").unwrap();
        assert_eq!(config.host, "bastion.example.com");
    }

    #[test]
    fn test_jump_host_chain_empty_display() {
        let chain = JumpHostChain::new();
        assert_eq!(chain.to_string(), "");
    }

    #[test]
    fn test_health_monitor_no_samples_latency() {
        let monitor = HealthMonitor::new(
            "test",
            HealthConfig::default(),
            CircuitBreakerConfig::default(),
        );

        assert_eq!(monitor.average_latency(), Duration::ZERO);
        assert_eq!(monitor.percentile_latency(50), Duration::ZERO);
        assert_eq!(monitor.success_rate(), 0.0);
    }

    #[test]
    fn test_retry_policy_zero_retries() {
        let policy = RetryPolicy::new().with_max_retries(0);
        assert_eq!(policy.max_retries, 0);
    }

    #[test]
    fn test_retry_policy_very_small_delay() {
        let policy = RetryPolicy::new()
            .with_initial_delay(Duration::from_nanos(1))
            .with_jitter(0.0);

        // Should not panic with very small delay
        let delay = policy.delay_for_attempt(0);
        assert!(delay >= Duration::from_nanos(0));
    }

    #[test]
    fn test_circuit_breaker_rapid_transitions() {
        let config = CircuitBreakerConfig::new()
            .with_failure_threshold(1)
            .with_success_threshold(1);
        let breaker = CircuitBreaker::new("test", config);
        let error = ConnectionError::ConnectionFailed("test".to_string());

        // Rapid trip and reset
        for _ in 0..10 {
            breaker.record_failure(&error);
            assert_eq!(breaker.state(), CircuitState::Open);
            breaker.reset();
            assert_eq!(breaker.state(), CircuitState::Closed);
        }
    }

    #[test]
    fn test_health_sample_ring_buffer_overflow() {
        let config = HealthConfig {
            sample_size: 5,
            ..HealthConfig::default()
        };

        let monitor = HealthMonitor::new("test", config, CircuitBreakerConfig::default());

        // Add more samples than buffer size
        for i in 0..10 {
            monitor.record_success(Duration::from_millis(i * 100));
        }

        let stats = monitor.stats();
        assert_eq!(stats.sample_count, 5); // Should be capped at sample_size
    }

    #[tokio::test]
    async fn test_retry_with_immediate_success() {
        let policy = RetryPolicy::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = rustible::connection::retry::retry(&policy, || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, ConnectionError>(()) }
        })
        .await;

        assert!(result.is_success());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
