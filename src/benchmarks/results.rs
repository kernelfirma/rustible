//! Benchmark result collection and reporting.
//!
//! Provides structured types for recording benchmark runs, system metadata,
//! and serialising reports to JSON for reproducible comparison over time.

use serde::{Deserialize, Serialize};

/// Information about the system that executed the benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Machine hostname.
    pub hostname: String,
    /// Number of logical CPUs.
    pub cpus: usize,
    /// Total physical memory in megabytes.
    pub memory_mb: u64,
    /// Operating system description.
    pub os: String,
    /// Rustible version string.
    pub rustible_version: String,
}

impl SystemInfo {
    /// Collect system information from the current host.
    pub fn collect() -> Self {
        Self {
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown".into()),
            cpus: num_cpus::get(),
            memory_mb: Self::read_memory_mb(),
            os: std::env::consts::OS.to_string(),
            rustible_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Attempt to read total physical memory from /proc/meminfo on Linux.
    /// Falls back to 0 on other platforms.
    fn read_memory_mb() -> u64 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
                for line in content.lines() {
                    if line.starts_with("MemTotal:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<u64>() {
                                return kb / 1024;
                            }
                        }
                    }
                }
            }
            0
        }
        #[cfg(not(target_os = "linux"))]
        {
            0
        }
    }
}

/// A single benchmark run recording latency statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRun {
    /// Name of the scenario that was executed.
    pub scenario_name: String,
    /// Wall-clock duration of the run in milliseconds.
    pub duration_ms: f64,
    /// Throughput measured as nodes processed per second.
    pub throughput_nodes_per_sec: f64,
    /// 50th-percentile (median) per-node latency in milliseconds.
    pub p50_ms: f64,
    /// 95th-percentile per-node latency in milliseconds.
    pub p95_ms: f64,
    /// 99th-percentile per-node latency in milliseconds.
    pub p99_ms: f64,
    /// Maximum observed per-node latency in milliseconds.
    pub max_ms: f64,
}

/// A complete benchmark report containing system info and all runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Metadata about the machine that produced the report.
    pub system_info: SystemInfo,
    /// Ordered list of benchmark runs.
    pub runs: Vec<BenchmarkRun>,
    /// ISO-8601 timestamp of when the report was created.
    pub timestamp: String,
}

impl BenchmarkReport {
    /// Create a new report, populating system info and timestamp automatically.
    pub fn new() -> Self {
        Self {
            system_info: SystemInfo::collect(),
            runs: Vec::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Append a benchmark run to the report.
    pub fn add_run(&mut self, run: BenchmarkRun) {
        self.runs.push(run);
    }

    /// Serialise the report to a pretty-printed JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {}\"}}", e))
    }
}

impl Default for BenchmarkReport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_roundtrip() {
        let mut report = BenchmarkReport::new();
        assert!(report.runs.is_empty());

        report.add_run(BenchmarkRun {
            scenario_name: "fanout_1k".into(),
            duration_ms: 250.0,
            throughput_nodes_per_sec: 4000.0,
            p50_ms: 2.0,
            p95_ms: 4.5,
            p99_ms: 8.0,
            max_ms: 12.0,
        });

        assert_eq!(report.runs.len(), 1);
        assert_eq!(report.runs[0].scenario_name, "fanout_1k");

        // Verify JSON round-trip.
        let json = report.to_json();
        let parsed: BenchmarkReport = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed.runs.len(), 1);
        assert!((parsed.runs[0].p50_ms - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_system_info_collect() {
        let info = SystemInfo::collect();
        assert!(info.cpus > 0);
        assert!(!info.rustible_version.is_empty());
    }
}
