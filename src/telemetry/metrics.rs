//! Metrics collection and export for observability.
//!
//! This module provides Prometheus-compatible metrics including counters,
//! gauges, and histograms for monitoring Rustible execution.

use crate::telemetry::config::MetricsConfig;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A metric counter that can only increase.
#[derive(Debug)]
pub struct Counter {
    /// Counter name
    name: String,
    /// Counter help text
    help: String,
    /// Counter value
    value: AtomicU64,
    /// Labels for this counter
    labels: HashMap<String, String>,
}

impl Counter {
    /// Create a new counter.
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            help: help.into(),
            value: AtomicU64::new(0),
            labels: HashMap::new(),
        }
    }

    /// Create a counter with labels.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    /// Increment the counter by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the counter by a given amount.
    pub fn inc_by(&self, amount: u64) {
        self.value.fetch_add(amount, Ordering::Relaxed);
    }

    /// Get the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Get the counter name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the help text.
    pub fn help(&self) -> &str {
        &self.help
    }

    /// Get the labels.
    pub fn labels(&self) -> &HashMap<String, String> {
        &self.labels
    }
}

/// A metric gauge that can go up and down.
#[derive(Debug)]
pub struct Gauge {
    /// Gauge name
    name: String,
    /// Gauge help text
    help: String,
    /// Gauge value (stored as i64 bits for atomicity)
    value: AtomicI64,
    /// Labels for this gauge
    labels: HashMap<String, String>,
}

impl Gauge {
    /// Create a new gauge.
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            help: help.into(),
            value: AtomicI64::new(0),
            labels: HashMap::new(),
        }
    }

    /// Create a gauge with labels.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    /// Set the gauge to a value.
    pub fn set(&self, value: i64) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Set the gauge to a floating-point value.
    pub fn set_f64(&self, value: f64) {
        self.value.store(value.to_bits() as i64, Ordering::Relaxed);
    }

    /// Increment the gauge by 1.
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the gauge by 1.
    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Add to the gauge.
    pub fn add(&self, amount: i64) {
        self.value.fetch_add(amount, Ordering::Relaxed);
    }

    /// Subtract from the gauge.
    pub fn sub(&self, amount: i64) {
        self.value.fetch_sub(amount, Ordering::Relaxed);
    }

    /// Get the current value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Get the current value as f64.
    pub fn get_f64(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed) as u64)
    }

    /// Get the gauge name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the help text.
    pub fn help(&self) -> &str {
        &self.help
    }

    /// Get the labels.
    pub fn labels(&self) -> &HashMap<String, String> {
        &self.labels
    }
}

/// A histogram for measuring distributions.
#[derive(Debug)]
pub struct Histogram {
    /// Histogram name
    name: String,
    /// Histogram help text
    help: String,
    /// Bucket boundaries
    buckets: Vec<f64>,
    /// Bucket counts (atomic)
    bucket_counts: Vec<AtomicU64>,
    /// Sum of all observed values
    sum: AtomicU64,
    /// Total count of observations
    count: AtomicU64,
    /// Labels for this histogram
    labels: HashMap<String, String>,
}

impl Histogram {
    /// Create a new histogram with default buckets.
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self::with_buckets(
            name,
            help,
            vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        )
    }

    /// Create a histogram with custom buckets.
    pub fn with_buckets(
        name: impl Into<String>,
        help: impl Into<String>,
        buckets: Vec<f64>,
    ) -> Self {
        let bucket_counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            name: name.into(),
            help: help.into(),
            buckets,
            bucket_counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
            labels: HashMap::new(),
        }
    }

    /// Create a histogram with labels.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    /// Observe a value.
    pub fn observe(&self, value: f64) {
        // Update sum and count
        self.sum.fetch_add(value.to_bits(), Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        // Update buckets
        for (i, &boundary) in self.buckets.iter().enumerate() {
            if value <= boundary {
                self.bucket_counts[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Observe a duration (in seconds).
    pub fn observe_duration(&self, duration: Duration) {
        self.observe(duration.as_secs_f64());
    }

    /// Start a timer that records duration on drop.
    pub fn start_timer(&self) -> HistogramTimer<'_> {
        HistogramTimer {
            histogram: self,
            start: Instant::now(),
        }
    }

    /// Get the total count.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get the sum (as bits).
    pub fn sum_bits(&self) -> u64 {
        self.sum.load(Ordering::Relaxed)
    }

    /// Get bucket values.
    pub fn bucket_values(&self) -> Vec<(f64, u64)> {
        self.buckets
            .iter()
            .zip(self.bucket_counts.iter())
            .map(|(&boundary, count)| (boundary, count.load(Ordering::Relaxed)))
            .collect()
    }

    /// Get the histogram name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the help text.
    pub fn help(&self) -> &str {
        &self.help
    }

    /// Get the labels.
    pub fn labels(&self) -> &HashMap<String, String> {
        &self.labels
    }
}

/// Timer for histogram observations.
pub struct HistogramTimer<'a> {
    histogram: &'a Histogram,
    start: Instant,
}

impl<'a> HistogramTimer<'a> {
    /// Stop the timer and record the duration.
    pub fn stop(self) -> f64 {
        let duration = self.start.elapsed();
        let secs = duration.as_secs_f64();
        self.histogram.observe(secs);
        secs
    }
}

impl<'a> Drop for HistogramTimer<'a> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        self.histogram.observe(duration.as_secs_f64());
    }
}

/// Registry for collecting and managing metrics.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    /// Registered counters
    counters: RwLock<HashMap<String, Arc<Counter>>>,
    /// Registered gauges
    gauges: RwLock<HashMap<String, Arc<Gauge>>>,
    /// Registered histograms
    histograms: RwLock<HashMap<String, Arc<Histogram>>>,
    /// Metric prefix
    prefix: Option<String>,
}

impl MetricsRegistry {
    /// Create a new metrics registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with a prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            ..Default::default()
        }
    }

    /// Create a registry from configuration.
    pub fn from_config(config: &MetricsConfig) -> Self {
        Self {
            prefix: config.prefix.clone(),
            ..Default::default()
        }
    }

    fn prefixed_name(&self, name: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}_{}", prefix, name),
            None => name.to_string(),
        }
    }

    /// Register or get a counter.
    pub fn counter(&self, name: &str, help: &str) -> Arc<Counter> {
        let prefixed = self.prefixed_name(name);
        let mut counters = self.counters.write();
        counters
            .entry(prefixed.clone())
            .or_insert_with(|| Arc::new(Counter::new(prefixed, help)))
            .clone()
    }

    /// Register or get a counter with labels.
    pub fn counter_with_labels(
        &self,
        name: &str,
        help: &str,
        labels: HashMap<String, String>,
    ) -> Arc<Counter> {
        let prefixed = self.prefixed_name(name);
        let key = format!(
            "{}{{{}}}",
            prefixed,
            labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut counters = self.counters.write();
        counters
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Counter::new(prefixed, help).with_labels(labels)))
            .clone()
    }

    /// Register or get a gauge.
    pub fn gauge(&self, name: &str, help: &str) -> Arc<Gauge> {
        let prefixed = self.prefixed_name(name);
        let mut gauges = self.gauges.write();
        gauges
            .entry(prefixed.clone())
            .or_insert_with(|| Arc::new(Gauge::new(prefixed, help)))
            .clone()
    }

    /// Register or get a gauge with labels.
    pub fn gauge_with_labels(
        &self,
        name: &str,
        help: &str,
        labels: HashMap<String, String>,
    ) -> Arc<Gauge> {
        let prefixed = self.prefixed_name(name);
        let key = format!(
            "{}{{{}}}",
            prefixed,
            labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut gauges = self.gauges.write();
        gauges
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Gauge::new(prefixed, help).with_labels(labels)))
            .clone()
    }

    /// Register or get a histogram.
    pub fn histogram(&self, name: &str, help: &str) -> Arc<Histogram> {
        let prefixed = self.prefixed_name(name);
        let mut histograms = self.histograms.write();
        histograms
            .entry(prefixed.clone())
            .or_insert_with(|| Arc::new(Histogram::new(prefixed, help)))
            .clone()
    }

    /// Register or get a histogram with custom buckets.
    pub fn histogram_with_buckets(
        &self,
        name: &str,
        help: &str,
        buckets: Vec<f64>,
    ) -> Arc<Histogram> {
        let prefixed = self.prefixed_name(name);
        let mut histograms = self.histograms.write();
        histograms
            .entry(prefixed.clone())
            .or_insert_with(|| Arc::new(Histogram::with_buckets(prefixed, help, buckets)))
            .clone()
    }

    /// Get all counters.
    pub fn get_counters(&self) -> Vec<Arc<Counter>> {
        self.counters.read().values().cloned().collect()
    }

    /// Get all gauges.
    pub fn get_gauges(&self) -> Vec<Arc<Gauge>> {
        self.gauges.read().values().cloned().collect()
    }

    /// Get all histograms.
    pub fn get_histograms(&self) -> Vec<Arc<Histogram>> {
        self.histograms.read().values().cloned().collect()
    }
}

/// Trait for recording metrics.
pub trait MetricsRecorder: Send + Sync {
    /// Increment a counter.
    fn inc_counter(&self, name: &str, labels: &[(&str, &str)]);

    /// Set a gauge value.
    fn set_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]);

    /// Observe a histogram value.
    fn observe_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]);
}

/// Trait for exporting metrics.
pub trait MetricsExporter: Send + Sync {
    /// Export metrics in the target format.
    fn export(&self, registry: &MetricsRegistry) -> String;
}

/// Prometheus text format exporter.
#[derive(Debug, Default)]
pub struct PrometheusExporter;

impl PrometheusExporter {
    /// Create a new Prometheus exporter.
    pub fn new() -> Self {
        Self
    }

    fn format_labels(labels: &HashMap<String, String>) -> String {
        if labels.is_empty() {
            String::new()
        } else {
            let pairs: Vec<_> = labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, escape_label_value(v)))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
    }
}

impl MetricsExporter for PrometheusExporter {
    fn export(&self, registry: &MetricsRegistry) -> String {
        let mut output = String::new();

        // Export counters
        for counter in registry.get_counters() {
            output.push_str(&format!("# HELP {} {}\n", counter.name(), counter.help()));
            output.push_str(&format!("# TYPE {} counter\n", counter.name()));
            let labels = Self::format_labels(counter.labels());
            output.push_str(&format!("{}{} {}\n", counter.name(), labels, counter.get()));
        }

        // Export gauges
        for gauge in registry.get_gauges() {
            output.push_str(&format!("# HELP {} {}\n", gauge.name(), gauge.help()));
            output.push_str(&format!("# TYPE {} gauge\n", gauge.name()));
            let labels = Self::format_labels(gauge.labels());
            output.push_str(&format!("{}{} {}\n", gauge.name(), labels, gauge.get()));
        }

        // Export histograms
        for histogram in registry.get_histograms() {
            output.push_str(&format!(
                "# HELP {} {}\n",
                histogram.name(),
                histogram.help()
            ));
            output.push_str(&format!("# TYPE {} histogram\n", histogram.name()));

            let base_labels = Self::format_labels(histogram.labels());

            // Bucket values
            let mut cumulative = 0u64;
            for (boundary, count) in histogram.bucket_values() {
                cumulative += count;
                let bucket_label = if histogram.labels().is_empty() {
                    format!("{{le=\"{}\"}}", boundary)
                } else {
                    format!(
                        "{{{},le=\"{}\"}}",
                        histogram
                            .labels()
                            .iter()
                            .map(|(k, v)| format!("{}=\"{}\"", k, escape_label_value(v)))
                            .collect::<Vec<_>>()
                            .join(","),
                        boundary
                    )
                };
                output.push_str(&format!(
                    "{}_bucket{} {}\n",
                    histogram.name(),
                    bucket_label,
                    cumulative
                ));
            }

            // +Inf bucket
            let inf_label = if histogram.labels().is_empty() {
                "{le=\"+Inf\"}".to_string()
            } else {
                format!(
                    "{{{},le=\"+Inf\"}}",
                    histogram
                        .labels()
                        .iter()
                        .map(|(k, v)| format!("{}=\"{}\"", k, escape_label_value(v)))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            output.push_str(&format!(
                "{}_bucket{} {}\n",
                histogram.name(),
                inf_label,
                histogram.count()
            ));

            // Sum and count
            output.push_str(&format!(
                "{}_sum{} {}\n",
                histogram.name(),
                base_labels,
                f64::from_bits(histogram.sum_bits())
            ));
            output.push_str(&format!(
                "{}_count{} {}\n",
                histogram.name(),
                base_labels,
                histogram.count()
            ));
        }

        output
    }
}

/// Escape special characters in label values for Prometheus format.
fn escape_label_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Pre-defined metrics for Rustible.
pub struct RustibleMetrics {
    /// Registry for all metrics
    pub registry: Arc<MetricsRegistry>,

    // Playbook metrics
    /// Total playbooks executed
    pub playbooks_total: Arc<Counter>,
    /// Playbooks currently running
    pub playbooks_running: Arc<Gauge>,
    /// Playbook execution duration
    pub playbook_duration_seconds: Arc<Histogram>,

    // Task metrics
    /// Total tasks executed
    pub tasks_total: Arc<Counter>,
    /// Tasks by status (ok, changed, failed, skipped)
    pub tasks_by_status: HashMap<String, Arc<Counter>>,
    /// Task execution duration
    pub task_duration_seconds: Arc<Histogram>,

    // Connection metrics
    /// Total connections made
    pub connections_total: Arc<Counter>,
    /// Active connections
    pub connections_active: Arc<Gauge>,
    /// Connection errors
    pub connection_errors_total: Arc<Counter>,
    /// Connection duration
    pub connection_duration_seconds: Arc<Histogram>,

    // Host metrics
    /// Hosts processed
    pub hosts_total: Arc<Counter>,
    /// Hosts by status
    pub hosts_by_status: HashMap<String, Arc<Counter>>,
}

impl RustibleMetrics {
    /// Create a new set of Rustible metrics.
    pub fn new() -> Self {
        let registry = Arc::new(MetricsRegistry::with_prefix("rustible"));

        let playbooks_total =
            registry.counter("playbooks_total", "Total number of playbooks executed");
        let playbooks_running =
            registry.gauge("playbooks_running", "Number of playbooks currently running");
        let playbook_duration_seconds = registry.histogram(
            "playbook_duration_seconds",
            "Duration of playbook execution in seconds",
        );

        let tasks_total = registry.counter("tasks_total", "Total number of tasks executed");
        let task_duration_seconds = registry.histogram(
            "task_duration_seconds",
            "Duration of task execution in seconds",
        );

        let mut tasks_by_status = HashMap::new();
        for status in &["ok", "changed", "failed", "skipped", "unreachable"] {
            let mut labels = HashMap::new();
            labels.insert("status".to_string(), (*status).to_string());
            tasks_by_status.insert(
                (*status).to_string(),
                registry.counter_with_labels(
                    "tasks_by_status",
                    "Tasks by execution status",
                    labels,
                ),
            );
        }

        let connections_total =
            registry.counter("connections_total", "Total number of connections made");
        let connections_active =
            registry.gauge("connections_active", "Number of active connections");
        let connection_errors_total = registry.counter(
            "connection_errors_total",
            "Total number of connection errors",
        );
        let connection_duration_seconds = registry.histogram(
            "connection_duration_seconds",
            "Duration of connection establishment in seconds",
        );

        let hosts_total = registry.counter("hosts_total", "Total number of hosts processed");
        let mut hosts_by_status = HashMap::new();
        for status in &["ok", "changed", "failed", "unreachable"] {
            let mut labels = HashMap::new();
            labels.insert("status".to_string(), (*status).to_string());
            hosts_by_status.insert(
                (*status).to_string(),
                registry.counter_with_labels("hosts_by_status", "Hosts by final status", labels),
            );
        }

        Self {
            registry,
            playbooks_total,
            playbooks_running,
            playbook_duration_seconds,
            tasks_total,
            tasks_by_status,
            task_duration_seconds,
            connections_total,
            connections_active,
            connection_errors_total,
            connection_duration_seconds,
            hosts_total,
            hosts_by_status,
        }
    }

    /// Record a task completion.
    pub fn record_task(&self, status: &str, duration: Duration) {
        self.tasks_total.inc();
        if let Some(counter) = self.tasks_by_status.get(status) {
            counter.inc();
        }
        self.task_duration_seconds.observe_duration(duration);
    }

    /// Record a playbook start.
    pub fn record_playbook_start(&self) {
        self.playbooks_total.inc();
        self.playbooks_running.inc();
    }

    /// Record a playbook completion.
    pub fn record_playbook_complete(&self, duration: Duration) {
        self.playbooks_running.dec();
        self.playbook_duration_seconds.observe_duration(duration);
    }

    /// Record a connection.
    pub fn record_connection(&self, duration: Duration, success: bool) {
        self.connections_total.inc();
        if !success {
            self.connection_errors_total.inc();
        }
        self.connection_duration_seconds.observe_duration(duration);
    }

    /// Export metrics in Prometheus format.
    pub fn export_prometheus(&self) -> String {
        let exporter = PrometheusExporter::new();
        exporter.export(&self.registry)
    }
}

impl Default for RustibleMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new("test_counter", "A test counter");
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.inc_by(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new("test_gauge", "A test gauge");
        assert_eq!(gauge.get(), 0);

        gauge.set(42);
        assert_eq!(gauge.get(), 42);

        gauge.inc();
        assert_eq!(gauge.get(), 43);

        gauge.dec();
        assert_eq!(gauge.get(), 42);
    }

    #[test]
    fn test_histogram() {
        let histogram = Histogram::new("test_histogram", "A test histogram");

        histogram.observe(0.1);
        histogram.observe(0.5);
        histogram.observe(1.5);

        assert_eq!(histogram.count(), 3);
    }

    #[test]
    fn test_registry() {
        let registry = MetricsRegistry::with_prefix("test");

        let counter = registry.counter("requests", "Total requests");
        counter.inc();

        let gauge = registry.gauge("active", "Active requests");
        gauge.set(5);

        let histogram = registry.histogram("duration", "Request duration");
        histogram.observe(0.1);

        assert_eq!(registry.get_counters().len(), 1);
        assert_eq!(registry.get_gauges().len(), 1);
        assert_eq!(registry.get_histograms().len(), 1);
    }

    #[test]
    fn test_prometheus_export() {
        let registry = MetricsRegistry::new();

        let counter = registry.counter("http_requests_total", "Total HTTP requests");
        counter.inc_by(100);

        let exporter = PrometheusExporter::new();
        let output = exporter.export(&registry);

        assert!(output.contains("http_requests_total"));
        assert!(output.contains("100"));
    }

    #[test]
    fn test_rustible_metrics() {
        let metrics = RustibleMetrics::new();

        metrics.record_playbook_start();
        assert_eq!(metrics.playbooks_total.get(), 1);
        assert_eq!(metrics.playbooks_running.get(), 1);

        metrics.record_task("ok", Duration::from_millis(100));
        assert_eq!(metrics.tasks_total.get(), 1);

        metrics.record_playbook_complete(Duration::from_secs(1));
        assert_eq!(metrics.playbooks_running.get(), 0);
    }
}
