//! Mock-based benchmark test harness.
//!
//! Provides a [`BenchmarkEnvironment`] that simulates fanout execution
//! according to a [`BenchmarkScenario`], using `tokio::time::sleep` to model
//! per-node latency. This allows integration-style benchmarks to run quickly
//! without real infrastructure.

use crate::benchmarks::results::BenchmarkRun;
use crate::benchmarks::scenarios::BenchmarkScenario;
use std::time::Instant;

/// A mock-based benchmark environment that simulates scenario execution.
#[derive(Debug)]
pub struct BenchmarkEnvironment {
    scenario: BenchmarkScenario,
}

impl BenchmarkEnvironment {
    /// Create a new environment bound to the given scenario.
    pub fn new(scenario: BenchmarkScenario) -> Self {
        Self { scenario }
    }

    /// Simulate the fanout described by the scenario.
    ///
    /// Each node's latency is modelled by a `tokio::time::sleep` call driven
    /// by the scenario's [`LatencyProfile`]. All nodes execute concurrently
    /// via `tokio::spawn`. The returned [`BenchmarkRun`] records wall-clock
    /// duration and per-node latency percentiles.
    pub async fn simulate_fanout(&self) -> BenchmarkRun {
        let node_count = self.scenario.node_count;
        let start = Instant::now();

        // Collect per-node latencies for percentile computation.
        let mut handles = Vec::with_capacity(node_count);

        for i in 0..node_count {
            let latency_ms = self.scenario.latency_profile.get_latency_for_node(i);
            handles.push(tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(latency_ms)).await;
                latency_ms as f64
            }));
        }

        let mut latencies = Vec::with_capacity(node_count);
        for handle in handles {
            if let Ok(lat) = handle.await {
                latencies.push(lat);
            }
        }

        let duration = start.elapsed();
        let duration_ms = duration.as_secs_f64() * 1000.0;

        // Sort latencies for percentile computation.
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let throughput = if duration_ms > 0.0 {
            (node_count as f64 / duration_ms) * 1000.0
        } else {
            0.0
        };

        BenchmarkRun {
            scenario_name: self.scenario.name.clone(),
            duration_ms,
            throughput_nodes_per_sec: throughput,
            p50_ms: percentile(&latencies, 0.50),
            p95_ms: percentile(&latencies, 0.95),
            p99_ms: percentile(&latencies, 0.99),
            max_ms: latencies.last().copied().unwrap_or(0.0),
        }
    }
}

/// Compute the value at the given percentile (0.0 .. 1.0) from a sorted slice.
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * pct).round() as usize;
    let idx = idx.min(sorted.len() - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmarks::scenarios::LatencyProfile;

    #[tokio::test]
    async fn test_simulate_fanout_small() {
        // Use a tiny scenario so the test finishes well under 1 second.
        let scenario =
            BenchmarkScenario::new("test_small", 20, LatencyProfile::Uniform(1), "linear");
        let env = BenchmarkEnvironment::new(scenario);
        let run = env.simulate_fanout().await;

        assert_eq!(run.scenario_name, "test_small");
        assert!(run.duration_ms > 0.0);
        assert!(run.throughput_nodes_per_sec > 0.0);
        // With uniform 1 ms latency, all percentiles should be 1.0.
        assert!((run.p50_ms - 1.0).abs() < f64::EPSILON);
        assert!((run.p95_ms - 1.0).abs() < f64::EPSILON);
        assert!((run.max_ms - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_simulate_fanout_bimodal() {
        let scenario = BenchmarkScenario::new(
            "test_bimodal",
            10,
            LatencyProfile::Bimodal {
                fast_ms: 1,
                slow_ms: 5,
            },
            "free",
        );
        let env = BenchmarkEnvironment::new(scenario);
        let run = env.simulate_fanout().await;

        assert_eq!(run.scenario_name, "test_bimodal");
        // Max latency must be the slow bucket.
        assert!((run.max_ms - 5.0).abs() < f64::EPSILON);
        // Median should be somewhere between 1 and 5.
        assert!(run.p50_ms >= 1.0 && run.p50_ms <= 5.0);
    }

    #[test]
    fn test_percentile_edge_cases() {
        assert!((percentile(&[], 0.5) - 0.0).abs() < f64::EPSILON);
        assert!((percentile(&[42.0], 0.99) - 42.0).abs() < f64::EPSILON);

        let sorted = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&sorted, 0.0) - 1.0).abs() < f64::EPSILON);
        assert!((percentile(&sorted, 1.0) - 5.0).abs() < f64::EPSILON);
        assert!((percentile(&sorted, 0.5) - 3.0).abs() < f64::EPSILON);
    }
}
