//! Metrics and observability for Rustible
//!
//! This module provides comprehensive metrics collection and export capabilities
//! for monitoring Rustible's performance and behavior:
//!
//! - **Connection Metrics**: Track SSH connection latency, success rates, and per-host statistics
//! - **Pool Metrics**: Monitor connection pool utilization, availability, and efficiency
//! - **Command Metrics**: Measure command execution times, success rates, and per-module statistics
//! - **Prometheus Export**: Export metrics in Prometheus-compatible format
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::metrics::{global, MetricsCollector, PrometheusExporter};
//! use std::sync::Arc;
//!
//! // Use the global metrics collector
//! let collector = global();
//!
//! // Track a connection
//! let timer = collector.connection.start_connection("host1.example.com");
//! // ... perform connection ...
//! collector.connection.record_connection_success(timer);
//!
//! // Get a snapshot of all metrics
//! let snapshot = collector.snapshot();
//! println!("Success rate: {:.2}%", snapshot.connection.success_rate());
//!
//! // Export for Prometheus
//! let exporter = PrometheusExporter::new(collector);
//! let prometheus_output = exporter.export();
//! # Ok(())
//! # }
//! ```

mod collector;
mod command;
mod connection;
mod pool;
mod prometheus;
mod types;

// Re-export main types
pub use collector::{
    global, init_global, HealthStatus, MetricsCollector, MetricsConfig, MetricsSnapshot,
};
pub use command::{
    CommandMetrics, CommandMetricsSummary, CommandTimer, HostCommandMetrics,
    HostCommandMetricsSnapshot, ModuleMetrics, ModuleMetricsSnapshot,
};
pub use connection::{
    ConnectionMetrics, ConnectionMetricsSummary, ConnectionTimer, HostConnectionMetrics,
    HostConnectionMetricsSnapshot,
};
pub use pool::{HostPoolMetrics, HostPoolMetricsSnapshot, PoolMetrics, PoolMetricsSummary};
pub use prometheus::{export_openmetrics, PrometheusExporter};
pub use types::{
    Counter, CounterSnapshot, Gauge, GaugeSnapshot, Histogram, HistogramSnapshot, Labeled, Labels,
    TimerGuard, DEFAULT_BUCKETS, LATENCY_BUCKETS_MS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_integration() {
        let collector = MetricsCollector::new();

        // Simulate some activity
        let timer = collector.connection.start_connection("test-host");
        collector.connection.record_connection_success(timer);

        let cmd_timer = collector.command.start_command("test-host", "shell");
        collector.command.record_success(cmd_timer, 100, 0);

        // Verify snapshot
        let snapshot = collector.snapshot();
        assert_eq!(snapshot.connection.total_attempts, 1);
        assert_eq!(snapshot.connection.total_successes, 1);
        assert_eq!(snapshot.command.total_executed, 1);
        assert_eq!(snapshot.command.total_succeeded, 1);
    }

    #[test]
    fn test_prometheus_export_integration() {
        use std::sync::Arc;

        let collector = Arc::new(MetricsCollector::new());

        // Add some metrics
        let timer = collector.connection.start_connection("host1");
        collector.connection.record_connection_success(timer);

        // Export to Prometheus format
        let exporter = PrometheusExporter::new(collector);
        let output = exporter.export();

        assert!(output.contains("rustible_connection_"));
        assert!(output.contains("# TYPE"));
        assert!(output.contains("# HELP"));
    }
}
