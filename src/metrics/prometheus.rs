//! Prometheus metrics export
//!
//! This module provides Prometheus-compatible metrics export in the
//! OpenMetrics text format, allowing integration with Prometheus,
//! Grafana, and other monitoring systems.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

use super::collector::MetricsCollector;
use super::types::{Counter, Gauge, Histogram, Labels};

// ============================================================================
// Prometheus Exporter
// ============================================================================

/// Prometheus metrics exporter
#[derive(Debug)]
pub struct PrometheusExporter {
    /// Metrics collector to export from
    collector: Arc<MetricsCollector>,
    /// Namespace prefix for all metrics
    namespace: String,
    /// Global labels to add to all metrics
    global_labels: Labels,
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter
    pub fn new(collector: Arc<MetricsCollector>) -> Self {
        Self {
            collector,
            namespace: "rustible".to_string(),
            global_labels: HashMap::new(),
        }
    }

    /// Create exporter with custom namespace
    pub fn with_namespace(collector: Arc<MetricsCollector>, namespace: impl Into<String>) -> Self {
        Self {
            collector,
            namespace: namespace.into(),
            global_labels: HashMap::new(),
        }
    }

    /// Add a global label to all exported metrics
    pub fn add_global_label(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.global_labels.insert(key.into(), value.into());
    }

    /// Export all metrics in Prometheus text format
    pub fn export(&self) -> String {
        let mut output = String::new();

        // Add header comments
        writeln!(output, "# Rustible Metrics Export").unwrap();
        writeln!(output, "# Generated at: {:?}", Instant::now()).unwrap();
        writeln!(output).unwrap();

        // Export connection metrics
        self.export_connection_metrics(&mut output);

        // Export pool metrics
        self.export_pool_metrics(&mut output);

        // Export command metrics
        self.export_command_metrics(&mut output);

        // Export per-host connection metrics
        self.export_per_host_connection_metrics(&mut output);

        // Export per-host pool metrics
        self.export_per_host_pool_metrics(&mut output);

        // Export per-module command metrics
        self.export_per_module_metrics(&mut output);

        // Export per-host command metrics
        self.export_per_host_command_metrics(&mut output);

        output
    }

    /// Export connection metrics
    fn export_connection_metrics(&self, output: &mut String) {
        let conn = &self.collector.connection;

        // Connection latency histogram
        self.export_histogram(
            output,
            "connection_latency_ms",
            "SSH connection establishment latency in milliseconds",
            &conn.connection_latency,
        );

        // Connection counters
        self.export_counter(
            output,
            "connection_attempts_total",
            "Total number of SSH connection attempts",
            &conn.connection_attempts,
        );

        self.export_counter(
            output,
            "connection_successes_total",
            "Total number of successful SSH connections",
            &conn.connection_successes,
        );

        self.export_counter(
            output,
            "connection_failures_total",
            "Total number of failed SSH connections",
            &conn.connection_failures,
        );

        // Active connections gauge
        self.export_gauge(
            output,
            "active_connections",
            "Current number of active SSH connections",
            &conn.active_connections,
        );

        self.export_counter(
            output,
            "connection_reuses_total",
            "Total number of connection reuses from pool",
            &conn.connection_reuses,
        );
    }

    /// Export pool metrics
    fn export_pool_metrics(&self, output: &mut String) {
        let pool = &self.collector.pool;

        self.export_gauge(
            output,
            "pool_size",
            "Current number of connections in pool",
            &pool.pool_size,
        );

        self.export_gauge(
            output,
            "pool_capacity",
            "Maximum pool capacity",
            &pool.pool_capacity,
        );

        self.export_gauge(
            output,
            "pool_available",
            "Number of available connections in pool",
            &pool.available_connections,
        );

        self.export_gauge(
            output,
            "pool_in_use",
            "Number of connections currently in use",
            &pool.in_use_connections,
        );

        self.export_gauge(
            output,
            "pool_utilization_percent",
            "Pool utilization percentage",
            &pool.utilization,
        );

        self.export_counter(
            output,
            "pool_connections_created_total",
            "Total connections created",
            &pool.connections_created,
        );

        self.export_counter(
            output,
            "pool_connections_destroyed_total",
            "Total connections destroyed",
            &pool.connections_destroyed,
        );

        self.export_histogram(
            output,
            "pool_wait_time_ms",
            "Time spent waiting for an available connection",
            &pool.wait_time,
        );

        self.export_counter(
            output,
            "pool_exhaustions_total",
            "Number of times pool was exhausted",
            &pool.pool_exhaustions,
        );

        self.export_counter(
            output,
            "pool_checkouts_total",
            "Total connection checkouts",
            &pool.checkouts,
        );

        self.export_counter(
            output,
            "pool_checkins_total",
            "Total connection checkins",
            &pool.checkins,
        );

        self.export_counter(
            output,
            "pool_health_checks_passed_total",
            "Total health checks passed",
            &pool.health_check_passes,
        );

        self.export_counter(
            output,
            "pool_health_checks_failed_total",
            "Total health checks failed",
            &pool.health_check_failures,
        );
    }

    /// Export command metrics
    fn export_command_metrics(&self, output: &mut String) {
        let cmd = &self.collector.command;

        self.export_histogram(
            output,
            "command_duration_ms",
            "Command execution duration in milliseconds",
            &cmd.execution_duration,
        );

        self.export_counter(
            output,
            "commands_executed_total",
            "Total number of commands executed",
            &cmd.commands_executed,
        );

        self.export_counter(
            output,
            "commands_succeeded_total",
            "Total number of successful commands",
            &cmd.commands_succeeded,
        );

        self.export_counter(
            output,
            "commands_failed_total",
            "Total number of failed commands",
            &cmd.commands_failed,
        );

        self.export_gauge(
            output,
            "commands_in_progress",
            "Number of commands currently executing",
            &cmd.commands_in_progress,
        );

        self.export_counter(
            output,
            "command_stdout_bytes_total",
            "Total bytes written to stdout",
            &cmd.bytes_stdout,
        );

        self.export_counter(
            output,
            "command_stderr_bytes_total",
            "Total bytes written to stderr",
            &cmd.bytes_stderr,
        );
    }

    /// Export per-host connection metrics
    fn export_per_host_connection_metrics(&self, output: &mut String) {
        let host_metrics = self.collector.connection.all_host_metrics();
        if host_metrics.is_empty() {
            return;
        }

        writeln!(output, "# Per-host connection metrics").unwrap();

        for host in host_metrics {
            let labels = format!("host=\"{}\"", escape_label_value(&host.host));

            writeln!(
                output,
                "{}_host_connection_attempts_total{{{}}}\t{}",
                self.namespace, labels, host.attempts
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_connection_successes_total{{{}}}\t{}",
                self.namespace, labels, host.successes
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_connection_failures_total{{{}}}\t{}",
                self.namespace, labels, host.failures
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_active_connections{{{}}}\t{}",
                self.namespace, labels, host.active
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_connection_latency_avg_ms{{{}}}\t{}",
                self.namespace, labels, host.avg_latency_ms
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_connection_latency_p95_ms{{{}}}\t{}",
                self.namespace, labels, host.p95_latency_ms
            )
            .unwrap();
        }
        writeln!(output).unwrap();
    }

    /// Export per-host pool metrics
    fn export_per_host_pool_metrics(&self, output: &mut String) {
        let host_metrics = self.collector.pool.all_host_metrics();
        if host_metrics.is_empty() {
            return;
        }

        writeln!(output, "# Per-host pool metrics").unwrap();

        for host in host_metrics {
            let labels = format!("host=\"{}\"", escape_label_value(&host.host));

            writeln!(
                output,
                "{}_host_pool_checkouts_total{{{}}}\t{}",
                self.namespace, labels, host.checkouts
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_pool_checkins_total{{{}}}\t{}",
                self.namespace, labels, host.checkins
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_pool_active{{{}}}\t{}",
                self.namespace, labels, host.active
            )
            .unwrap();
        }
        writeln!(output).unwrap();
    }

    /// Export per-module command metrics
    fn export_per_module_metrics(&self, output: &mut String) {
        let module_metrics = self.collector.command.all_module_metrics();
        if module_metrics.is_empty() {
            return;
        }

        writeln!(output, "# Per-module command metrics").unwrap();

        for module in module_metrics {
            let labels = format!("module=\"{}\"", escape_label_value(&module.module));

            writeln!(
                output,
                "{}_module_executed_total{{{}}}\t{}",
                self.namespace, labels, module.executed
            )
            .unwrap();

            writeln!(
                output,
                "{}_module_succeeded_total{{{}}}\t{}",
                self.namespace, labels, module.succeeded
            )
            .unwrap();

            writeln!(
                output,
                "{}_module_failed_total{{{}}}\t{}",
                self.namespace, labels, module.failed
            )
            .unwrap();

            writeln!(
                output,
                "{}_module_duration_avg_ms{{{}}}\t{}",
                self.namespace, labels, module.avg_duration_ms
            )
            .unwrap();

            writeln!(
                output,
                "{}_module_duration_p95_ms{{{}}}\t{}",
                self.namespace, labels, module.p95_duration_ms
            )
            .unwrap();
        }
        writeln!(output).unwrap();
    }

    /// Export per-host command metrics
    fn export_per_host_command_metrics(&self, output: &mut String) {
        let host_metrics = self.collector.command.all_host_metrics();
        if host_metrics.is_empty() {
            return;
        }

        writeln!(output, "# Per-host command metrics").unwrap();

        for host in host_metrics {
            let labels = format!("host=\"{}\"", escape_label_value(&host.host));

            writeln!(
                output,
                "{}_host_commands_executed_total{{{}}}\t{}",
                self.namespace, labels, host.executed
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_commands_succeeded_total{{{}}}\t{}",
                self.namespace, labels, host.succeeded
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_commands_failed_total{{{}}}\t{}",
                self.namespace, labels, host.failed
            )
            .unwrap();

            writeln!(
                output,
                "{}_host_command_duration_avg_ms{{{}}}\t{}",
                self.namespace, labels, host.avg_duration_ms
            )
            .unwrap();
        }
        writeln!(output).unwrap();
    }

    /// Export a counter metric
    fn export_counter(&self, output: &mut String, name: &str, help: &str, counter: &Counter) {
        let full_name = format!("{}_{}", self.namespace, name);
        writeln!(output, "# HELP {} {}", full_name, help).unwrap();
        writeln!(output, "# TYPE {} counter", full_name).unwrap();

        let labels = self.format_labels(counter.labels());
        writeln!(output, "{}{}\t{}", full_name, labels, counter.get()).unwrap();
        writeln!(output).unwrap();
    }

    /// Export a gauge metric
    fn export_gauge(&self, output: &mut String, name: &str, help: &str, gauge: &Gauge) {
        let full_name = format!("{}_{}", self.namespace, name);
        writeln!(output, "# HELP {} {}", full_name, help).unwrap();
        writeln!(output, "# TYPE {} gauge", full_name).unwrap();

        let labels = self.format_labels(gauge.labels());
        writeln!(output, "{}{}\t{}", full_name, labels, gauge.get()).unwrap();
        writeln!(output).unwrap();
    }

    /// Export a histogram metric
    fn export_histogram(&self, output: &mut String, name: &str, help: &str, histogram: &Histogram) {
        let full_name = format!("{}_{}", self.namespace, name);
        writeln!(output, "# HELP {} {}", full_name, help).unwrap();
        writeln!(output, "# TYPE {} histogram", full_name).unwrap();

        let base_labels = self.format_labels(histogram.labels());
        let buckets = histogram.buckets();
        let counts = histogram.bucket_counts();

        // Export bucket counts
        for (bucket, count) in buckets.iter().zip(counts.iter()) {
            let le_label = if base_labels.is_empty() {
                format!("{{le=\"{}\"}}", bucket)
            } else {
                format!(
                    "{{{},le=\"{}\"}}",
                    &base_labels[1..base_labels.len() - 1],
                    bucket
                )
            };
            writeln!(output, "{}_bucket{}\t{}", full_name, le_label, count).unwrap();
        }

        // Export +Inf bucket
        let inf_label = if base_labels.is_empty() {
            "{le=\"+Inf\"}".to_string()
        } else {
            format!("{{{},le=\"+Inf\"}}", &base_labels[1..base_labels.len() - 1])
        };
        writeln!(
            output,
            "{}_bucket{}\t{}",
            full_name,
            inf_label,
            histogram.count()
        )
        .unwrap();

        // Export sum and count
        writeln!(
            output,
            "{}_sum{}\t{}",
            full_name,
            base_labels,
            histogram.sum()
        )
        .unwrap();
        writeln!(
            output,
            "{}_count{}\t{}",
            full_name,
            base_labels,
            histogram.count()
        )
        .unwrap();
        writeln!(output).unwrap();
    }

    /// Format labels for Prometheus output
    fn format_labels(&self, metric_labels: &Labels) -> String {
        let mut all_labels = self.global_labels.clone();
        all_labels.extend(metric_labels.iter().map(|(k, v)| (k.clone(), v.clone())));

        if all_labels.is_empty() {
            return String::new();
        }

        let label_strs: Vec<String> = all_labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, escape_label_value(v)))
            .collect();

        format!("{{{}}}", label_strs.join(","))
    }
}

/// Escape special characters in label values
fn escape_label_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

// ============================================================================
// OpenMetrics Format (Prometheus 2.0+)
// ============================================================================

/// Export metrics in OpenMetrics format
pub fn export_openmetrics(_collector: &MetricsCollector) -> String {
    let exporter = PrometheusExporter::new(Arc::new(MetricsCollector::new()));
    let mut output = exporter.export();

    // Add OpenMetrics EOF marker
    output.push_str("# EOF\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_export() {
        let collector = Arc::new(MetricsCollector::new());
        let exporter = PrometheusExporter::new(collector);
        let output = exporter.export();

        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
        assert!(output.contains("rustible_"));
    }

    #[test]
    fn test_escape_label_value() {
        assert_eq!(escape_label_value("simple"), "simple");
        assert_eq!(escape_label_value("with\"quote"), "with\\\"quote");
        assert_eq!(escape_label_value("with\\backslash"), "with\\\\backslash");
        assert_eq!(escape_label_value("with\nnewline"), "with\\nnewline");
    }

    #[test]
    fn test_histogram_export() {
        let collector = Arc::new(MetricsCollector::new());

        // Add some histogram data
        collector.connection.connection_latency.observe(0.1);
        collector.connection.connection_latency.observe(0.5);
        collector.connection.connection_latency.observe(1.0);

        let exporter = PrometheusExporter::new(collector);
        let output = exporter.export();

        assert!(output.contains("_bucket"));
        assert!(output.contains("le=\"+Inf\""));
        assert!(output.contains("_sum"));
        assert!(output.contains("_count"));
    }
}
