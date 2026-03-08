//! Startup timing metrics for performance profiling.
//!
//! This module provides utilities for measuring and reporting
//! startup performance across different initialization phases.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Phases of application startup that can be timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StartupPhase {
    /// CLI argument parsing
    CliParsing,
    /// Logging/tracing initialization
    LoggingInit,
    /// Configuration file loading
    ConfigLoading,
    /// Module registry initialization
    ModuleRegistry,
    /// Callback plugin initialization
    CallbackInit,
    /// Inventory loading
    InventoryLoading,
    /// Playbook parsing
    PlaybookParsing,
    /// SSH/connection initialization
    ConnectionInit,
    /// Total application startup
    Total,
    /// Custom phase (use with custom name)
    Custom(&'static str),
}

impl std::fmt::Display for StartupPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupPhase::CliParsing => write!(f, "CLI Parsing"),
            StartupPhase::LoggingInit => write!(f, "Logging Init"),
            StartupPhase::ConfigLoading => write!(f, "Config Loading"),
            StartupPhase::ModuleRegistry => write!(f, "Module Registry"),
            StartupPhase::CallbackInit => write!(f, "Callback Init"),
            StartupPhase::InventoryLoading => write!(f, "Inventory Loading"),
            StartupPhase::PlaybookParsing => write!(f, "Playbook Parsing"),
            StartupPhase::ConnectionInit => write!(f, "Connection Init"),
            StartupPhase::Total => write!(f, "Total Startup"),
            StartupPhase::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Metrics for a single phase.
#[derive(Debug, Clone)]
pub struct PhaseMetrics {
    /// Start time of the phase
    start: Option<Instant>,
    /// Total duration of the phase
    duration: Duration,
    /// Whether the phase completed successfully
    completed: bool,
}

impl Default for PhaseMetrics {
    fn default() -> Self {
        Self {
            start: None,
            duration: Duration::ZERO,
            completed: false,
        }
    }
}

impl PhaseMetrics {
    /// Get the duration of this phase.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Check if this phase completed.
    pub fn is_completed(&self) -> bool {
        self.completed
    }
}

/// Startup metrics collector.
///
/// Tracks timing for various phases of application startup.
///
/// # Example
///
/// ```rust
/// use rustible::startup::{StartupMetrics, StartupPhase};
///
/// let mut metrics = StartupMetrics::new();
///
/// metrics.start_phase(StartupPhase::ConfigLoading);
/// // ... load config ...
/// metrics.end_phase(StartupPhase::ConfigLoading);
///
/// metrics.start_phase(StartupPhase::ModuleRegistry);
/// // ... init modules ...
/// metrics.end_phase(StartupPhase::ModuleRegistry);
///
/// // Print report
/// metrics.report();
/// ```
#[derive(Debug, Default)]
pub struct StartupMetrics {
    /// Per-phase metrics
    phases: HashMap<StartupPhase, PhaseMetrics>,
    /// When overall tracking started
    overall_start: Option<Instant>,
    /// When overall tracking ended
    overall_end: Option<Instant>,
}

impl StartupMetrics {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            phases: HashMap::new(),
            overall_start: Some(Instant::now()),
            overall_end: None,
        }
    }

    /// Start timing a phase.
    pub fn start_phase(&mut self, phase: StartupPhase) {
        let metrics = self.phases.entry(phase).or_default();
        metrics.start = Some(Instant::now());
        metrics.completed = false;
    }

    /// End timing a phase.
    pub fn end_phase(&mut self, phase: StartupPhase) {
        if let Some(metrics) = self.phases.get_mut(&phase) {
            if let Some(start) = metrics.start {
                metrics.duration = start.elapsed();
                metrics.completed = true;
            }
        }
    }

    /// Mark the end of overall startup.
    pub fn finish(&mut self) {
        self.overall_end = Some(Instant::now());
    }

    /// Get metrics for a specific phase.
    pub fn get_phase(&self, phase: &StartupPhase) -> Option<&PhaseMetrics> {
        self.phases.get(phase)
    }

    /// Get the total startup duration.
    pub fn total_duration(&self) -> Duration {
        match (self.overall_start, self.overall_end) {
            (Some(start), Some(end)) => end.duration_since(start),
            (Some(start), None) => start.elapsed(),
            _ => Duration::ZERO,
        }
    }

    /// Get all phase durations sorted by duration (descending).
    pub fn sorted_phases(&self) -> Vec<(StartupPhase, Duration)> {
        let mut phases: Vec<_> = self
            .phases
            .iter()
            .filter(|(_, m)| m.completed)
            .map(|(p, m)| (*p, m.duration))
            .collect();
        phases.sort_by(|a, b| b.1.cmp(&a.1));
        phases
    }

    /// Print a formatted report of startup timing.
    pub fn report(&self) {
        let total = self.total_duration();

        eprintln!("\n=== Startup Timing Report ===");
        eprintln!();

        for (phase, duration) in self.sorted_phases() {
            let pct = if total.as_nanos() > 0 {
                (duration.as_nanos() as f64 / total.as_nanos() as f64) * 100.0
            } else {
                0.0
            };

            eprintln!(
                "  {:20} {:>8.2}ms ({:>5.1}%)",
                phase.to_string(),
                duration.as_secs_f64() * 1000.0,
                pct
            );
        }

        eprintln!();
        eprintln!("  {:20} {:>8.2}ms", "TOTAL", total.as_secs_f64() * 1000.0);
        eprintln!();
    }

    /// Get a JSON representation of the metrics.
    pub fn to_json(&self) -> serde_json::Value {
        let phases: serde_json::Map<String, serde_json::Value> = self
            .phases
            .iter()
            .filter(|(_, m)| m.completed)
            .map(|(p, m)| {
                (
                    p.to_string(),
                    serde_json::json!({
                        "duration_ms": m.duration.as_secs_f64() * 1000.0,
                        "duration_us": m.duration.as_micros(),
                    }),
                )
            })
            .collect();

        serde_json::json!({
            "total_ms": self.total_duration().as_secs_f64() * 1000.0,
            "total_us": self.total_duration().as_micros(),
            "phases": phases,
        })
    }

    /// Measure a phase using a closure.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustible::startup::{StartupMetrics, StartupPhase};
    ///
    /// let mut metrics = StartupMetrics::new();
    ///
    /// let config = metrics.measure(StartupPhase::ConfigLoading, || {
    ///     // Load config
    ///     "loaded config"
    /// });
    /// ```
    pub fn measure<T, F>(&mut self, phase: StartupPhase, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.start_phase(phase);
        let result = f();
        self.end_phase(phase);
        result
    }

    /// Async version of measure.
    pub async fn measure_async<T, F, Fut>(&mut self, phase: StartupPhase, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        self.start_phase(phase);
        let result = f().await;
        self.end_phase(phase);
        result
    }
}

/// RAII guard for timing a phase.
///
/// Automatically ends the phase when dropped.
pub struct PhaseGuard<'a> {
    metrics: &'a mut StartupMetrics,
    phase: StartupPhase,
}

impl<'a> PhaseGuard<'a> {
    /// Create a new phase guard that starts timing immediately.
    pub fn new(metrics: &'a mut StartupMetrics, phase: StartupPhase) -> Self {
        metrics.start_phase(phase);
        Self { metrics, phase }
    }
}

impl<'a> Drop for PhaseGuard<'a> {
    fn drop(&mut self) {
        self.metrics.end_phase(self.phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_basic_timing() {
        let mut metrics = StartupMetrics::new();

        metrics.start_phase(StartupPhase::ConfigLoading);
        thread::sleep(Duration::from_millis(10));
        metrics.end_phase(StartupPhase::ConfigLoading);

        let phase = metrics.get_phase(&StartupPhase::ConfigLoading).unwrap();
        assert!(phase.is_completed());
        assert!(phase.duration() >= Duration::from_millis(10));
    }

    #[test]
    fn test_measure() {
        let mut metrics = StartupMetrics::new();

        let result = metrics.measure(StartupPhase::ModuleRegistry, || {
            thread::sleep(Duration::from_millis(5));
            42
        });

        assert_eq!(result, 42);

        let phase = metrics.get_phase(&StartupPhase::ModuleRegistry).unwrap();
        assert!(phase.is_completed());
        assert!(phase.duration() >= Duration::from_millis(5));
    }

    #[test]
    fn test_total_duration() {
        let mut metrics = StartupMetrics::new();

        thread::sleep(Duration::from_millis(5));
        metrics.finish();

        assert!(metrics.total_duration() >= Duration::from_millis(5));
    }

    #[test]
    fn test_sorted_phases() {
        let mut metrics = StartupMetrics::new();

        metrics.phases.insert(
            StartupPhase::ConfigLoading,
            PhaseMetrics {
                start: None,
                duration: Duration::from_millis(20),
                completed: true,
            },
        );
        metrics.phases.insert(
            StartupPhase::ModuleRegistry,
            PhaseMetrics {
                start: None,
                duration: Duration::from_millis(5),
                completed: true,
            },
        );

        let sorted = metrics.sorted_phases();
        assert_eq!(sorted.len(), 2);

        // ConfigLoading should be first (longer duration)
        assert_eq!(sorted[0].0, StartupPhase::ConfigLoading);
        assert_eq!(sorted[1].0, StartupPhase::ModuleRegistry);
    }

    #[test]
    fn test_to_json() {
        let mut metrics = StartupMetrics::new();

        metrics.start_phase(StartupPhase::ConfigLoading);
        thread::sleep(Duration::from_millis(5));
        metrics.end_phase(StartupPhase::ConfigLoading);
        metrics.finish();

        let json = metrics.to_json();

        assert!(json["total_ms"].as_f64().unwrap() > 0.0);
        assert!(
            json["phases"]["Config Loading"]["duration_ms"]
                .as_f64()
                .unwrap()
                > 0.0
        );
    }

    #[test]
    fn test_custom_phase() {
        let mut metrics = StartupMetrics::new();

        let custom = StartupPhase::Custom("MyCustomPhase");

        metrics.start_phase(custom);
        thread::sleep(Duration::from_millis(1));
        metrics.end_phase(custom);

        assert_eq!(custom.to_string(), "MyCustomPhase");

        let phase = metrics.get_phase(&custom).unwrap();
        assert!(phase.is_completed());
    }
}
