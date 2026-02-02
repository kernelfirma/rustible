//! Statistics Aggregator callback plugin for Rustible.
//!
//! This plugin collects comprehensive execution statistics including:
//! - Task success/failure rates per host and overall
//! - Timing metrics per module type
//! - Memory/resource usage tracking
//! - Export capabilities in multiple formats (JSON, Prometheus)
//!
//! # Features
//!
//! - **Comprehensive Metrics**: Track all task results with detailed timing
//! - **Module Classification**: Group stats by module type (LocalLogic, NativeTransport, etc.)
//! - **Export Formats**: JSON for analysis, Prometheus for monitoring integration
//! - **Thread-safe**: Safe for concurrent access across parallel task execution
//! - **Histogram Support**: Distribution of execution times for percentile analysis
//!
//! # Example Output (JSON)
//!
//! ```json
//! {
//!   "playbook": "deploy.yml",
//!   "duration_secs": 45.23,
//!   "hosts": {
//!     "webserver1": {
//!       "ok": 15, "changed": 5, "failed": 0, "skipped": 2
//!     }
//!   },
//!   "module_stats": {
//!     "apt": { "count": 10, "avg_duration_ms": 1250.5, "failures": 0 }
//!   }
//! }
//! ```
//!
//! # Prometheus Format
//!
//! ```text
//! # HELP rustible_tasks_total Total number of tasks executed
//! # TYPE rustible_tasks_total counter
//! rustible_tasks_total{status="ok"} 50
//! rustible_tasks_total{status="changed"} 20
//! rustible_tasks_total{status="failed"} 2
//! ```
//!
//! # Example Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{StatsCallback, StatsConfig};
//!
//! // Create with default config
//! let callback = StatsCallback::new();
//!
//! // Or with custom config
//! let config = StatsConfig {
//!     enable_histograms: true,
//!     per_host_module_stats: true,
//!     track_memory: true,
//!     ..Default::default()
//! };
//! let callback = StatsCallback::with_config(config);
//!
//! // After execution, export stats
//! let json = callback.export_json();
//! let prometheus = callback.export_prometheus();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

/// Configuration options for the stats callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsConfig {
    /// Whether to track timing histograms (slightly more overhead)
    pub enable_histograms: bool,
    /// Whether to track per-host module breakdowns
    pub per_host_module_stats: bool,
    /// Whether to track memory usage (requires platform support)
    pub track_memory: bool,
    /// Histogram bucket boundaries in milliseconds
    pub histogram_buckets_ms: Vec<u64>,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            enable_histograms: true,
            per_host_module_stats: false,
            track_memory: false,
            // Default Prometheus-style buckets: 10ms, 50ms, 100ms, 250ms, 500ms, 1s, 2.5s, 5s, 10s
            histogram_buckets_ms: vec![10, 50, 100, 250, 500, 1000, 2500, 5000, 10000],
        }
    }
}

/// Module classification tiers for grouping statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleClassification {
    /// Tier 1: Logic modules that run on the control node
    LocalLogic,
    /// Tier 2: File/transport modules with native Rust implementation
    NativeTransport,
    /// Tier 3: Remote command execution modules
    RemoteCommand,
    /// Tier 4: Python fallback for Ansible compatibility
    PythonFallback,
}

impl std::fmt::Display for ModuleClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleClassification::LocalLogic => write!(f, "local_logic"),
            ModuleClassification::NativeTransport => write!(f, "native_transport"),
            ModuleClassification::RemoteCommand => write!(f, "remote_command"),
            ModuleClassification::PythonFallback => write!(f, "python_fallback"),
        }
    }
}

/// Per-host execution statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostStats {
    /// Number of tasks that completed successfully without changes
    pub ok: u64,
    /// Number of tasks that made changes
    pub changed: u64,
    /// Number of tasks that failed
    pub failed: u64,
    /// Number of tasks that were skipped
    pub skipped: u64,
    /// Number of unreachable attempts
    pub unreachable: u64,
    /// Total execution time in milliseconds for this host
    pub total_duration_ms: u64,
}

impl HostStats {
    /// Create new empty host stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a task result.
    pub fn record(&mut self, result: &ModuleResult, duration: Duration) {
        let duration_ms = duration.as_millis() as u64;
        self.total_duration_ms += duration_ms;

        if result.skipped {
            self.skipped += 1;
        } else if !result.success {
            self.failed += 1;
        } else if result.changed {
            self.changed += 1;
        } else {
            self.ok += 1;
        }
    }

    /// Get total task count.
    pub fn total(&self) -> u64 {
        self.ok + self.changed + self.failed + self.skipped + self.unreachable
    }

    /// Calculate success rate as a percentage.
    pub fn success_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            100.0
        } else {
            let successful = self.ok + self.changed + self.skipped;
            (successful as f64 / total as f64) * 100.0
        }
    }
}

/// Statistics for a specific module type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleStats {
    /// Total number of executions
    pub count: u64,
    /// Number of successful executions (ok + changed)
    pub successes: u64,
    /// Number of failures
    pub failures: u64,
    /// Number of tasks that made changes
    pub changed: u64,
    /// Number of skipped tasks
    pub skipped: u64,
    /// Total execution time in milliseconds
    pub total_duration_ms: u64,
    /// Minimum execution time in milliseconds
    pub min_duration_ms: Option<u64>,
    /// Maximum execution time in milliseconds
    pub max_duration_ms: Option<u64>,
    /// Module classification tier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<ModuleClassification>,
}

impl ModuleStats {
    /// Create new empty module stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create module stats with a classification.
    pub fn with_classification(classification: ModuleClassification) -> Self {
        Self {
            classification: Some(classification),
            ..Default::default()
        }
    }

    /// Record a task execution result.
    pub fn record(&mut self, result: &ModuleResult, duration_ms: u64) {
        self.count += 1;

        if result.skipped {
            self.skipped += 1;
        } else if !result.success {
            self.failures += 1;
        } else {
            self.successes += 1;
            if result.changed {
                self.changed += 1;
            }
        }

        self.total_duration_ms += duration_ms;
        self.min_duration_ms = Some(
            self.min_duration_ms
                .map(|m| m.min(duration_ms))
                .unwrap_or(duration_ms),
        );
        self.max_duration_ms = Some(
            self.max_duration_ms
                .map(|m| m.max(duration_ms))
                .unwrap_or(duration_ms),
        );
    }

    /// Calculate average duration in milliseconds.
    pub fn avg_duration_ms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / self.count as f64
        }
    }

    /// Calculate success rate as a percentage.
    pub fn success_rate(&self) -> f64 {
        if self.count == 0 {
            100.0
        } else {
            (self.successes as f64 / self.count as f64) * 100.0
        }
    }

    /// Calculate change rate as a percentage of successful executions.
    pub fn change_rate(&self) -> f64 {
        if self.successes == 0 {
            0.0
        } else {
            (self.changed as f64 / self.successes as f64) * 100.0
        }
    }
}

/// Histogram for tracking duration distributions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationHistogram {
    /// Bucket boundaries in milliseconds
    pub buckets: Vec<u64>,
    /// Count per bucket (bucket[i] = count of durations <= buckets[i])
    pub counts: Vec<u64>,
    /// Count of values above the highest bucket
    pub overflow: u64,
    /// Sum of all values for calculating mean
    pub sum_ms: u64,
    /// Total count
    pub total: u64,
}

impl DurationHistogram {
    /// Create a new histogram with the given bucket boundaries.
    pub fn new(buckets: Vec<u64>) -> Self {
        let counts = vec![0; buckets.len()];
        Self {
            buckets,
            counts,
            overflow: 0,
            sum_ms: 0,
            total: 0,
        }
    }

    /// Record a duration value.
    pub fn record(&mut self, duration_ms: u64) {
        self.sum_ms += duration_ms;
        self.total += 1;

        // Find the appropriate bucket
        for (i, &boundary) in self.buckets.iter().enumerate() {
            if duration_ms <= boundary {
                self.counts[i] += 1;
                return;
            }
        }
        // Value exceeds all buckets
        self.overflow += 1;
    }

    /// Calculate the approximate percentile value.
    pub fn percentile(&self, p: f64) -> Option<u64> {
        if self.total == 0 {
            return None;
        }

        let target = (self.total as f64 * p / 100.0).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, &count) in self.counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return Some(self.buckets[i]);
            }
        }

        // Return highest bucket if we're in overflow
        self.buckets.last().copied()
    }

    /// Calculate mean duration.
    pub fn mean(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sum_ms as f64 / self.total as f64
        }
    }
}

/// Memory usage snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Timestamp in milliseconds since epoch
    pub timestamp_ms: u64,
    /// Resident set size in bytes (if available)
    pub rss_bytes: Option<u64>,
    /// Virtual memory size in bytes (if available)
    pub vms_bytes: Option<u64>,
    /// Heap allocated bytes (if available)
    pub heap_bytes: Option<u64>,
}

impl MemorySnapshot {
    /// Take a memory snapshot (platform-dependent).
    #[cfg(target_os = "linux")]
    pub fn take() -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Try to read /proc/self/statm for memory info
        let (rss_bytes, vms_bytes) = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|content| {
                let parts: Vec<&str> = content.split_whitespace().collect();
                if parts.len() >= 2 {
                    let page_size = 4096u64; // Typical page size
                    let vms = parts[0].parse::<u64>().ok().map(|v| v * page_size);
                    let rss = parts[1].parse::<u64>().ok().map(|v| v * page_size);
                    Some((rss, vms))
                } else {
                    None
                }
            })
            .unwrap_or((None, None));

        Self {
            timestamp_ms,
            rss_bytes,
            vms_bytes,
            heap_bytes: None,
        }
    }

    /// Take a memory snapshot (non-Linux platforms).
    #[cfg(not(target_os = "linux"))]
    pub fn take() -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            timestamp_ms,
            rss_bytes: None,
            vms_bytes: None,
            heap_bytes: None,
        }
    }
}

/// Statistics for a single play within a playbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayStats {
    /// Play name
    pub name: String,
    /// Target hosts
    pub hosts: Vec<String>,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Task count
    pub task_count: u64,
    /// Start timestamp
    pub start_time_ms: u64,
    /// Whether the play succeeded
    pub success: bool,
}

/// Aggregated statistics for an entire playbook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookStats {
    /// Playbook file name
    pub playbook: String,
    /// Start timestamp in milliseconds since epoch
    pub start_time_ms: u64,
    /// End timestamp in milliseconds since epoch (if completed)
    pub end_time_ms: Option<u64>,
    /// Total duration in seconds
    pub duration_secs: Option<f64>,
    /// Per-host statistics
    pub hosts: HashMap<String, HostStats>,
    /// Per-module statistics
    pub module_stats: HashMap<String, ModuleStats>,
    /// Statistics grouped by module classification
    pub classification_stats: HashMap<String, ModuleStats>,
    /// Overall task counts
    pub total_tasks: u64,
    pub total_ok: u64,
    pub total_changed: u64,
    pub total_failed: u64,
    pub total_skipped: u64,
    pub total_unreachable: u64,
    /// Duration histogram (if enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_histogram: Option<DurationHistogram>,
    /// Memory snapshots (if enabled)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub memory_snapshots: Vec<MemorySnapshot>,
    /// Current play name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_play: Option<String>,
    /// Play statistics
    pub plays: Vec<PlayStats>,
    /// Whether the playbook succeeded
    pub success: bool,
}

impl PlaybookStats {
    /// Create new stats for a playbook.
    pub fn new(playbook: &str, config: &StatsConfig) -> Self {
        let histogram = if config.enable_histograms {
            Some(DurationHistogram::new(config.histogram_buckets_ms.clone()))
        } else {
            None
        };

        Self {
            playbook: playbook.to_string(),
            start_time_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            end_time_ms: None,
            duration_secs: None,
            hosts: HashMap::new(),
            module_stats: HashMap::new(),
            classification_stats: HashMap::new(),
            total_tasks: 0,
            total_ok: 0,
            total_changed: 0,
            total_failed: 0,
            total_skipped: 0,
            total_unreachable: 0,
            duration_histogram: histogram,
            memory_snapshots: Vec::new(),
            current_play: None,
            plays: Vec::new(),
            success: true,
        }
    }

    /// Calculate overall success rate.
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            100.0
        } else {
            let successful = self.total_ok + self.total_changed + self.total_skipped;
            (successful as f64 / self.total_tasks as f64) * 100.0
        }
    }

    /// Check if any failures occurred.
    pub fn has_failures(&self) -> bool {
        self.total_failed > 0 || self.total_unreachable > 0
    }
}

/// Thread-safe internal state for the stats callback.
struct StatsState {
    /// Configuration
    config: StatsConfig,
    /// Current playbook stats (if a playbook is running)
    current_stats: Option<PlaybookStats>,
    /// Historical stats from previous playbook runs in this session
    history: Vec<PlaybookStats>,
    /// Current task start time (for timing)
    task_start: Option<Instant>,
    /// Current play start time
    play_start: Option<Instant>,
    /// Playbook start time
    playbook_start: Option<Instant>,
    /// Module to classification mapping cache
    module_classifications: HashMap<String, ModuleClassification>,
}

impl StatsState {
    fn new(config: StatsConfig) -> Self {
        Self {
            config,
            current_stats: None,
            history: Vec::new(),
            task_start: None,
            play_start: None,
            playbook_start: None,
            module_classifications: Self::build_classification_map(),
        }
    }

    /// Build the default module classification map.
    fn build_classification_map() -> HashMap<String, ModuleClassification> {
        let mut map = HashMap::new();

        // LocalLogic modules (Tier 1)
        for module in &[
            "debug",
            "set_fact",
            "assert",
            "fail",
            "meta",
            "include_tasks",
            "import_tasks",
            "include_vars",
            "pause",
            "wait_for",
        ] {
            map.insert(module.to_string(), ModuleClassification::LocalLogic);
        }

        // NativeTransport modules (Tier 2)
        for module in &[
            "copy",
            "template",
            "file",
            "lineinfile",
            "blockinfile",
            "fetch",
            "stat",
            "synchronize",
        ] {
            map.insert(module.to_string(), ModuleClassification::NativeTransport);
        }

        // RemoteCommand modules (Tier 3)
        for module in &[
            "command", "shell", "raw", "script", "service", "systemd", "apt", "yum", "dnf",
            "package", "pip", "user", "group", "git", "cron",
        ] {
            map.insert(module.to_string(), ModuleClassification::RemoteCommand);
        }

        map
    }

    /// Get classification for a module.
    fn get_classification(&self, module: &str) -> ModuleClassification {
        self.module_classifications
            .get(module)
            .copied()
            .unwrap_or(ModuleClassification::PythonFallback)
    }
}

/// Statistics Aggregator callback plugin.
///
/// Collects comprehensive execution statistics and exports them in
/// multiple formats for analysis and monitoring integration.
///
/// # Thread Safety
///
/// This callback is thread-safe and can be safely shared across
/// parallel task executions. All state mutations are protected
/// by a RwLock.
pub struct StatsCallback {
    state: Arc<RwLock<StatsState>>,
    /// Atomic counters for lock-free hot path updates
    task_counter: AtomicU64,
}

impl StatsCallback {
    /// Create a new stats callback with default configuration.
    pub fn new() -> Self {
        Self::with_config(StatsConfig::default())
    }

    /// Create a stats callback with custom configuration.
    pub fn with_config(config: StatsConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(StatsState::new(config))),
            task_counter: AtomicU64::new(0),
        }
    }

    /// Get the current playbook statistics (if running).
    pub fn current_stats(&self) -> Option<PlaybookStats> {
        self.state.read().current_stats.clone()
    }

    /// Get historical statistics from previous runs.
    pub fn history(&self) -> Vec<PlaybookStats> {
        self.state.read().history.clone()
    }

    /// Export current statistics as JSON.
    pub fn export_json(&self) -> String {
        let state = self.state.read();
        if let Some(ref current_stats) = state.current_stats {
            serde_json::to_string_pretty(current_stats).unwrap_or_else(|_| "{}".to_string())
        } else if let Some(last) = state.history.last() {
            serde_json::to_string_pretty(last).unwrap_or_else(|_| "{}".to_string())
        } else {
            "{}".to_string()
        }
    }

    /// Export current statistics as compact JSON (single line).
    pub fn export_json_compact(&self) -> String {
        let state = self.state.read();
        if let Some(ref current_stats) = state.current_stats {
            serde_json::to_string(current_stats).unwrap_or_else(|_| "{}".to_string())
        } else if let Some(last) = state.history.last() {
            serde_json::to_string(last).unwrap_or_else(|_| "{}".to_string())
        } else {
            "{}".to_string()
        }
    }

    /// Export current statistics in Prometheus exposition format.
    pub fn export_prometheus(&self) -> String {
        let state = self.state.read();
        let current_stats = state
            .current_stats
            .as_ref()
            .or_else(|| state.history.last());

        let Some(playbook_stats) = current_stats else {
            return String::new();
        };

        let mut output = String::new();

        // Task totals
        output.push_str("# HELP rustible_tasks_total Total number of tasks executed\n");
        output.push_str("# TYPE rustible_tasks_total counter\n");
        output.push_str(&format!(
            "rustible_tasks_total{{status=\"ok\"}} {}\n",
            playbook_stats.total_ok
        ));
        output.push_str(&format!(
            "rustible_tasks_total{{status=\"changed\"}} {}\n",
            playbook_stats.total_changed
        ));
        output.push_str(&format!(
            "rustible_tasks_total{{status=\"failed\"}} {}\n",
            playbook_stats.total_failed
        ));
        output.push_str(&format!(
            "rustible_tasks_total{{status=\"skipped\"}} {}\n",
            playbook_stats.total_skipped
        ));
        output.push_str(&format!(
            "rustible_tasks_total{{status=\"unreachable\"}} {}\n",
            playbook_stats.total_unreachable
        ));

        // Duration
        if let Some(duration) = playbook_stats.duration_secs {
            output.push_str(
                "\n# HELP rustible_playbook_duration_seconds Total playbook execution time\n",
            );
            output.push_str("# TYPE rustible_playbook_duration_seconds gauge\n");
            output.push_str(&format!(
                "rustible_playbook_duration_seconds{{playbook=\"{}\"}} {:.3}\n",
                playbook_stats.playbook, duration
            ));
        }

        // Success rate
        output.push_str("\n# HELP rustible_success_rate Percentage of successful tasks\n");
        output.push_str("# TYPE rustible_success_rate gauge\n");
        output.push_str(&format!(
            "rustible_success_rate{{playbook=\"{}\"}} {:.2}\n",
            playbook_stats.playbook,
            playbook_stats.success_rate()
        ));

        // Per-host stats
        output.push_str("\n# HELP rustible_host_tasks_total Tasks per host by status\n");
        output.push_str("# TYPE rustible_host_tasks_total counter\n");
        for (host, host_stats) in &playbook_stats.hosts {
            output.push_str(&format!(
                "rustible_host_tasks_total{{host=\"{}\",status=\"ok\"}} {}\n",
                host, host_stats.ok
            ));
            output.push_str(&format!(
                "rustible_host_tasks_total{{host=\"{}\",status=\"changed\"}} {}\n",
                host, host_stats.changed
            ));
            output.push_str(&format!(
                "rustible_host_tasks_total{{host=\"{}\",status=\"failed\"}} {}\n",
                host, host_stats.failed
            ));
            output.push_str(&format!(
                "rustible_host_tasks_total{{host=\"{}\",status=\"skipped\"}} {}\n",
                host, host_stats.skipped
            ));
        }

        // Per-module stats
        output.push_str(
            "\n# HELP rustible_module_duration_seconds_total Total execution time per module\n",
        );
        output.push_str("# TYPE rustible_module_duration_seconds_total counter\n");
        output.push_str("\n# HELP rustible_module_executions_total Executions per module\n");
        output.push_str("# TYPE rustible_module_executions_total counter\n");
        for (module, module_stats) in &playbook_stats.module_stats {
            output.push_str(&format!(
                "rustible_module_executions_total{{module=\"{}\"}} {}\n",
                module, module_stats.count
            ));
            output.push_str(&format!(
                "rustible_module_duration_seconds_total{{module=\"{}\"}} {:.3}\n",
                module,
                module_stats.total_duration_ms as f64 / 1000.0
            ));
            output.push_str(&format!(
                "rustible_module_failures_total{{module=\"{}\"}} {}\n",
                module, module_stats.failures
            ));
        }

        // Classification stats
        output.push_str(
            "\n# HELP rustible_classification_duration_seconds_total Time by module classification\n",
        );
        output.push_str("# TYPE rustible_classification_duration_seconds_total counter\n");
        for (classification, class_stats) in &playbook_stats.classification_stats {
            output.push_str(&format!(
                "rustible_classification_executions_total{{classification=\"{}\"}} {}\n",
                classification, class_stats.count
            ));
            output.push_str(&format!(
                "rustible_classification_duration_seconds_total{{classification=\"{}\"}} {:.3}\n",
                classification,
                class_stats.total_duration_ms as f64 / 1000.0
            ));
        }

        // Histogram buckets if available
        if let Some(ref histogram) = playbook_stats.duration_histogram {
            output.push_str(
                "\n# HELP rustible_task_duration_seconds Task execution duration histogram\n",
            );
            output.push_str("# TYPE rustible_task_duration_seconds histogram\n");

            let mut cumulative = 0u64;
            for (i, &bucket) in histogram.buckets.iter().enumerate() {
                cumulative += histogram.counts[i];
                output.push_str(&format!(
                    "rustible_task_duration_seconds_bucket{{le=\"{:.3}\"}} {}\n",
                    bucket as f64 / 1000.0,
                    cumulative
                ));
            }
            output.push_str(&format!(
                "rustible_task_duration_seconds_bucket{{le=\"+Inf\"}} {}\n",
                histogram.total
            ));
            output.push_str(&format!(
                "rustible_task_duration_seconds_sum {:.3}\n",
                histogram.sum_ms as f64 / 1000.0
            ));
            output.push_str(&format!(
                "rustible_task_duration_seconds_count {}\n",
                histogram.total
            ));
        }

        // Memory stats if available
        if let Some(snapshot) = playbook_stats.memory_snapshots.last() {
            if let Some(rss) = snapshot.rss_bytes {
                output.push_str("\n# HELP rustible_memory_rss_bytes Resident set size in bytes\n");
                output.push_str("# TYPE rustible_memory_rss_bytes gauge\n");
                output.push_str(&format!("rustible_memory_rss_bytes {}\n", rss));
            }
            if let Some(vms) = snapshot.vms_bytes {
                output
                    .push_str("\n# HELP rustible_memory_vms_bytes Virtual memory size in bytes\n");
                output.push_str("# TYPE rustible_memory_vms_bytes gauge\n");
                output.push_str(&format!("rustible_memory_vms_bytes {}\n", vms));
            }
        }

        output
    }

    /// Get a summary string suitable for logging.
    pub fn summary(&self) -> String {
        let state = self.state.read();
        let current_stats = state
            .current_stats
            .as_ref()
            .or_else(|| state.history.last());

        let Some(playbook_stats) = current_stats else {
            return "No statistics available".to_string();
        };

        let mut summary = String::new();
        summary.push_str(&format!("Playbook: {}\n", playbook_stats.playbook));

        if let Some(duration) = playbook_stats.duration_secs {
            summary.push_str(&format!("Duration: {:.2}s\n", duration));
        }

        summary.push_str(&format!(
            "Tasks: {} total, {} ok, {} changed, {} failed, {} skipped\n",
            playbook_stats.total_tasks,
            playbook_stats.total_ok,
            playbook_stats.total_changed,
            playbook_stats.total_failed,
            playbook_stats.total_skipped
        ));

        summary.push_str(&format!(
            "Success Rate: {:.1}%\n",
            playbook_stats.success_rate()
        ));

        if !playbook_stats.module_stats.is_empty() {
            summary.push_str("\nTop modules by execution time:\n");
            let mut modules: Vec<_> = playbook_stats.module_stats.iter().collect();
            modules.sort_by(|a, b| b.1.total_duration_ms.cmp(&a.1.total_duration_ms));

            for (module, module_stats) in modules.iter().take(5) {
                summary.push_str(&format!(
                    "  {}: {} calls, {:.1}ms avg, {:.1}% success\n",
                    module,
                    module_stats.count,
                    module_stats.avg_duration_ms(),
                    module_stats.success_rate()
                ));
            }
        }

        summary
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.current_stats = None;
        state.task_start = None;
        state.play_start = None;
        state.playbook_start = None;
        self.task_counter.store(0, Ordering::SeqCst);
    }

    /// Clear history but keep current stats.
    pub fn clear_history(&self) {
        let mut state = self.state.write();
        state.history.clear();
    }

    /// Take a memory snapshot if tracking is enabled.
    fn maybe_snapshot_memory(&self) {
        let mut state = self.state.write();
        if state.config.track_memory {
            if let Some(ref mut current_stats) = state.current_stats {
                current_stats.memory_snapshots.push(MemorySnapshot::take());
            }
        }
    }

    /// Record module execution with explicit module name.
    pub fn record_module_execution(&self, module: &str, result: &ModuleResult, duration: Duration) {
        let duration_ms = duration.as_millis() as u64;
        let mut state = self.state.write();

        let classification = state.get_classification(module);

        let Some(ref mut current_stats) = state.current_stats else {
            return;
        };

        // Update per-module stats
        let module_stats = current_stats
            .module_stats
            .entry(module.to_string())
            .or_insert_with(|| ModuleStats::with_classification(classification));
        module_stats.record(result, duration_ms);

        // Update classification stats
        let class_stats = current_stats
            .classification_stats
            .entry(classification.to_string())
            .or_default();
        class_stats.record(result, duration_ms);
    }
}

impl Default for StatsCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StatsCallback {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            task_counter: AtomicU64::new(self.task_counter.load(Ordering::SeqCst)),
        }
    }
}

impl std::fmt::Debug for StatsCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatsCallback")
            .field("task_counter", &self.task_counter.load(Ordering::SeqCst))
            .finish()
    }
}

#[async_trait]
impl ExecutionCallback for StatsCallback {
    async fn on_playbook_start(&self, name: &str) {
        let mut state = self.state.write();

        // Archive any existing stats
        if let Some(old_stats) = state.current_stats.take() {
            state.history.push(old_stats);
        }

        // Start new stats
        state.current_stats = Some(PlaybookStats::new(name, &state.config));
        state.task_start = None;
        state.play_start = None;
        state.playbook_start = Some(Instant::now());

        self.task_counter.store(0, Ordering::SeqCst);

        // Initial memory snapshot
        drop(state);
        self.maybe_snapshot_memory();
    }

    async fn on_playbook_end(&self, _name: &str, success: bool) {
        let mut state = self.state.write();

        // Get duration before borrowing current_stats mutably
        let duration_secs = state
            .playbook_start
            .map(|start| start.elapsed().as_secs_f64());

        if let Some(ref mut playbook_stats) = state.current_stats {
            playbook_stats.end_time_ms = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            );

            if let Some(secs) = duration_secs {
                playbook_stats.duration_secs = Some(secs);
            }

            playbook_stats.success = success;
        }

        // Final memory snapshot
        drop(state);
        self.maybe_snapshot_memory();
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let mut state = self.state.write();

        state.play_start = Some(Instant::now());

        if let Some(ref mut current_stats) = state.current_stats {
            current_stats.current_play = Some(name.to_string());
            current_stats.plays.push(PlayStats {
                name: name.to_string(),
                hosts: hosts.to_vec(),
                duration_ms: None,
                task_count: 0,
                start_time_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                success: true,
            });
        }
    }

    async fn on_play_end(&self, _name: &str, success: bool) {
        let mut state = self.state.write();

        let duration_ms = state
            .play_start
            .map(|start| start.elapsed().as_millis() as u64);

        if let Some(ref mut current_stats) = state.current_stats {
            current_stats.current_play = None;

            if let Some(play) = current_stats.plays.last_mut() {
                play.duration_ms = duration_ms;
                play.success = success;
            }
        }

        state.play_start = None;
    }

    async fn on_task_start(&self, _name: &str, _host: &str) {
        let mut state = self.state.write();
        state.task_start = Some(Instant::now());
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Increment atomic counter first (lock-free)
        self.task_counter.fetch_add(1, Ordering::SeqCst);

        let mut state = self.state.write();

        // Calculate duration from task_start or use result.duration
        let duration = state
            .task_start
            .take()
            .map(|start| start.elapsed())
            .unwrap_or(result.duration);

        let duration_ms = duration.as_millis() as u64;

        let Some(ref mut current_stats) = state.current_stats else {
            return;
        };

        // Update total counters
        current_stats.total_tasks += 1;
        if result.result.skipped {
            current_stats.total_skipped += 1;
        } else if !result.result.success {
            current_stats.total_failed += 1;
        } else if result.result.changed {
            current_stats.total_changed += 1;
        } else {
            current_stats.total_ok += 1;
        }

        // Update per-host stats
        let host_stats = current_stats.hosts.entry(result.host.clone()).or_default();
        host_stats.record(&result.result, duration);

        // Update histogram if enabled
        if let Some(ref mut histogram) = current_stats.duration_histogram {
            histogram.record(duration_ms);
        }

        // Update current play task count
        if let Some(play) = current_stats.plays.last_mut() {
            play.task_count += 1;
        }
    }

    async fn on_handler_triggered(&self, _name: &str) {
        // Handlers don't affect stats directly
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        // Optionally track fact gathering time
        let _ = host;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_callback_creation() {
        let callback = StatsCallback::new();
        assert_eq!(callback.task_counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_stats_config_default() {
        let config = StatsConfig::default();
        assert!(config.enable_histograms);
        assert!(!config.per_host_module_stats);
        assert!(!config.track_memory);
        assert!(!config.histogram_buckets_ms.is_empty());
    }

    #[test]
    fn test_host_stats_recording() {
        let mut stats = HostStats::new();

        let ok_result = ModuleResult::ok("success");
        let changed_result = ModuleResult::changed("modified");
        let failed_result = ModuleResult::failed("error");
        let skipped_result = ModuleResult::skipped("skipped");

        stats.record(&ok_result, Duration::from_millis(100));
        stats.record(&changed_result, Duration::from_millis(200));
        stats.record(&failed_result, Duration::from_millis(50));
        stats.record(&skipped_result, Duration::from_millis(10));

        assert_eq!(stats.ok, 1);
        assert_eq!(stats.changed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.total(), 4);
        assert_eq!(stats.total_duration_ms, 360);
    }

    #[test]
    fn test_module_stats_recording() {
        let mut stats = ModuleStats::new();

        let ok_result = ModuleResult::ok("success");
        let changed_result = ModuleResult::changed("modified");
        let failed_result = ModuleResult::failed("error");

        stats.record(&ok_result, 100);
        stats.record(&changed_result, 200);
        stats.record(&failed_result, 50);

        assert_eq!(stats.count, 3);
        assert_eq!(stats.successes, 2);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.changed, 1);
        assert_eq!(stats.min_duration_ms, Some(50));
        assert_eq!(stats.max_duration_ms, Some(200));
        assert!((stats.avg_duration_ms() - 116.67).abs() < 0.1);
    }

    #[test]
    fn test_duration_histogram() {
        let mut histogram = DurationHistogram::new(vec![10, 50, 100, 500]);

        histogram.record(5); // <= 10
        histogram.record(15); // <= 50
        histogram.record(45); // <= 50
        histogram.record(75); // <= 100
        histogram.record(200); // <= 500
        histogram.record(1000); // overflow

        assert_eq!(histogram.total, 6);
        assert_eq!(histogram.counts, vec![1, 2, 1, 1]);
        assert_eq!(histogram.overflow, 1);

        // Test percentiles
        assert_eq!(histogram.percentile(50.0), Some(50));
        assert_eq!(histogram.percentile(90.0), Some(500));
    }

    #[test]
    fn test_playbook_stats_success_rate() {
        let mut stats = PlaybookStats::new("test.yml", &StatsConfig::default());

        stats.total_tasks = 100;
        stats.total_ok = 60;
        stats.total_changed = 30;
        stats.total_failed = 5;
        stats.total_skipped = 5;

        assert!((stats.success_rate() - 95.0).abs() < 0.01);
        assert!(stats.has_failures());
    }

    #[test]
    fn test_module_classification() {
        let state = StatsState::new(StatsConfig::default());

        assert_eq!(
            state.get_classification("debug"),
            ModuleClassification::LocalLogic
        );
        assert_eq!(
            state.get_classification("copy"),
            ModuleClassification::NativeTransport
        );
        assert_eq!(
            state.get_classification("apt"),
            ModuleClassification::RemoteCommand
        );
        assert_eq!(
            state.get_classification("unknown_module"),
            ModuleClassification::PythonFallback
        );
    }

    #[tokio::test]
    async fn test_callback_lifecycle() {
        let callback = StatsCallback::new();

        // Start playbook
        callback.on_playbook_start("test.yml").await;
        assert!(callback.current_stats().is_some());

        // Start play
        callback
            .on_play_start("Test Play", &["host1".to_string()])
            .await;

        // Simulate task
        callback.on_task_start("Test Task", "host1").await;

        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Test Task".to_string(),
            result: ModuleResult::changed("done"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;

        // End play and playbook
        callback.on_play_end("Test Play", true).await;
        callback.on_playbook_end("test.yml", true).await;

        // Verify stats
        let stats = callback.current_stats().unwrap();
        assert_eq!(stats.total_tasks, 1);
        assert_eq!(stats.total_changed, 1);
        assert!(stats.duration_secs.is_some());
    }

    #[test]
    fn test_export_json() {
        let callback = StatsCallback::new();

        // Need to run async in a tokio runtime for this test
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            callback.on_playbook_start("test.yml").await;
            callback.on_playbook_end("test.yml", true).await;
        });

        let json = callback.export_json();
        assert!(json.contains("test.yml"));
    }

    #[test]
    fn test_export_prometheus() {
        let callback = StatsCallback::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            callback.on_playbook_start("test.yml").await;

            let result = ExecutionResult {
                host: "host1".to_string(),
                task_name: "Test Task".to_string(),
                result: ModuleResult::ok("done"),
                duration: Duration::from_millis(100),
                notify: vec![],
            };
            callback.on_task_complete(&result).await;
            callback.on_playbook_end("test.yml", true).await;
        });

        let prometheus = callback.export_prometheus();
        assert!(prometheus.contains("rustible_tasks_total"));
        assert!(prometheus.contains("rustible_success_rate"));
    }

    #[test]
    fn test_callback_clone_shares_state() {
        let callback1 = StatsCallback::new();
        let callback2 = callback1.clone();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            callback1.on_playbook_start("test.yml").await;
        });

        // Both should see the same stats
        assert!(callback1.current_stats().is_some());
        assert!(callback2.current_stats().is_some());
    }

    #[test]
    fn test_reset() {
        let callback = StatsCallback::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            callback.on_playbook_start("test.yml").await;
        });

        assert!(callback.current_stats().is_some());

        callback.reset();
        assert!(callback.current_stats().is_none());
    }

    #[test]
    fn test_memory_snapshot() {
        let snapshot = MemorySnapshot::take();
        assert!(snapshot.timestamp_ms > 0);
        // RSS/VMS may or may not be available depending on platform
    }

    #[test]
    fn test_record_module_execution() {
        let callback = StatsCallback::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            callback.on_playbook_start("test.yml").await;
        });

        let result = ModuleResult::changed("installed");
        callback.record_module_execution("apt", &result, Duration::from_millis(1500));
        callback.record_module_execution(
            "apt",
            &ModuleResult::ok("ok"),
            Duration::from_millis(500),
        );
        callback.record_module_execution(
            "debug",
            &ModuleResult::ok("msg"),
            Duration::from_millis(1),
        );

        let stats = callback.current_stats().unwrap();

        // Check module stats
        let apt_stats = stats.module_stats.get("apt").unwrap();
        assert_eq!(apt_stats.count, 2);
        assert_eq!(apt_stats.changed, 1);
        assert_eq!(apt_stats.total_duration_ms, 2000);

        // Check classification stats
        let remote_stats = stats.classification_stats.get("remote_command").unwrap();
        assert_eq!(remote_stats.count, 2);

        let local_stats = stats.classification_stats.get("local_logic").unwrap();
        assert_eq!(local_stats.count, 1);
    }
}
