//! Benchmark scenario definitions for reproducible HPC benchmarking.
//!
//! This module provides predefined benchmark scenarios that model realistic
//! fanout patterns across node clusters with configurable latency profiles.

use serde::{Deserialize, Serialize};

/// Latency profile for simulating different network topologies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LatencyProfile {
    /// Every node has the same latency.
    Uniform(u64),
    /// Nodes in the same rack are fast; cross-rack nodes are slower.
    RackAware { local_ms: u64, remote_ms: u64 },
    /// Most nodes are fast, but a configurable ratio are slow.
    Mixed {
        low_ms: u64,
        high_ms: u64,
        /// Fraction of nodes that use `high_ms` (0.0 .. 1.0).
        high_ratio: f64,
    },
    /// Two distinct latency buckets (e.g. local SSD vs remote storage).
    Bimodal { fast_ms: u64, slow_ms: u64 },
}

impl LatencyProfile {
    /// Return the simulated latency in milliseconds for the given node index.
    pub fn get_latency_for_node(&self, node_index: usize) -> u64 {
        match self {
            LatencyProfile::Uniform(ms) => *ms,
            LatencyProfile::RackAware {
                local_ms,
                remote_ms,
            } => {
                // Simple rack model: nodes 0..half are local, rest remote.
                if node_index % 2 == 0 {
                    *local_ms
                } else {
                    *remote_ms
                }
            }
            LatencyProfile::Mixed {
                low_ms,
                high_ms,
                high_ratio,
            } => {
                // Deterministic: the first `high_ratio` fraction of nodes are slow.
                // We use a simple hash-like approach so it is reproducible.
                let threshold = (*high_ratio * 1000.0) as usize;
                let bucket = (node_index * 7 + 13) % 1000;
                if bucket < threshold {
                    *high_ms
                } else {
                    *low_ms
                }
            }
            LatencyProfile::Bimodal { fast_ms, slow_ms } => {
                if node_index % 2 == 0 {
                    *fast_ms
                } else {
                    *slow_ms
                }
            }
        }
    }
}

/// A reproducible benchmark scenario describing a fanout workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkScenario {
    /// Human-readable name for the scenario.
    pub name: String,
    /// Number of target nodes in the fanout.
    pub node_count: usize,
    /// Latency profile modelling network behaviour.
    pub latency_profile: LatencyProfile,
    /// Execution strategy label (e.g. "linear", "free", "host-pinned").
    pub strategy: String,
}

impl BenchmarkScenario {
    /// Create a new benchmark scenario.
    pub fn new(
        name: impl Into<String>,
        node_count: usize,
        latency_profile: LatencyProfile,
        strategy: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            node_count,
            latency_profile,
            strategy: strategy.into(),
        }
    }

    /// 1 000-node fanout with uniform 2 ms latency.
    pub fn fanout_1k() -> Self {
        Self::new("fanout_1k", 1_000, LatencyProfile::Uniform(2), "linear")
    }

    /// 5 000-node fanout with rack-aware latency.
    pub fn fanout_5k() -> Self {
        Self::new(
            "fanout_5k",
            5_000,
            LatencyProfile::RackAware {
                local_ms: 1,
                remote_ms: 5,
            },
            "free",
        )
    }

    /// 10 000-node fanout with a mixed latency profile (20 % slow nodes).
    pub fn fanout_10k() -> Self {
        Self::new(
            "fanout_10k",
            10_000,
            LatencyProfile::Mixed {
                low_ms: 1,
                high_ms: 10,
                high_ratio: 0.2,
            },
            "host-pinned",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_latency() {
        let profile = LatencyProfile::Uniform(5);
        assert_eq!(profile.get_latency_for_node(0), 5);
        assert_eq!(profile.get_latency_for_node(99), 5);
        assert_eq!(profile.get_latency_for_node(10_000), 5);
    }

    #[test]
    fn test_rack_aware_latency() {
        let profile = LatencyProfile::RackAware {
            local_ms: 1,
            remote_ms: 8,
        };
        // Even indices are local, odd are remote.
        assert_eq!(profile.get_latency_for_node(0), 1);
        assert_eq!(profile.get_latency_for_node(1), 8);
        assert_eq!(profile.get_latency_for_node(2), 1);
        assert_eq!(profile.get_latency_for_node(3), 8);
    }

    #[test]
    fn test_mixed_latency_determinism() {
        let profile = LatencyProfile::Mixed {
            low_ms: 1,
            high_ms: 50,
            high_ratio: 0.3,
        };
        // Verify deterministic: same index always yields the same value.
        let first = profile.get_latency_for_node(42);
        let second = profile.get_latency_for_node(42);
        assert_eq!(first, second);

        // The result must be one of the two configured values.
        assert!(first == 1 || first == 50);
    }

    #[test]
    fn test_bimodal_latency() {
        let profile = LatencyProfile::Bimodal {
            fast_ms: 2,
            slow_ms: 20,
        };
        assert_eq!(profile.get_latency_for_node(0), 2);
        assert_eq!(profile.get_latency_for_node(1), 20);
        assert_eq!(profile.get_latency_for_node(100), 2);
        assert_eq!(profile.get_latency_for_node(101), 20);
    }

    #[test]
    fn test_predefined_scenarios() {
        let s1 = BenchmarkScenario::fanout_1k();
        assert_eq!(s1.node_count, 1_000);
        assert_eq!(s1.strategy, "linear");

        let s2 = BenchmarkScenario::fanout_5k();
        assert_eq!(s2.node_count, 5_000);
        assert_eq!(s2.strategy, "free");

        let s3 = BenchmarkScenario::fanout_10k();
        assert_eq!(s3.node_count, 10_000);
        assert_eq!(s3.strategy, "host-pinned");
    }
}
