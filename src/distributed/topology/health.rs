//! Health aggregation for the cluster topology
//!
//! Provides [`HealthAggregator`] which collects per-node health checks and
//! produces a [`ClusterHealthSummary`] reflecting the overall cluster state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::model::ClusterTopology;

/// Status of a single health dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Fully operational
    Healthy,
    /// Operational but with warnings
    Degraded,
    /// Not operational
    Unhealthy,
    /// Status could not be determined
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unhealthy => write!(f, "unhealthy"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// A single health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Name of the check (e.g. "disk", "raft", "connectivity")
    pub name: String,
    /// Result of the check
    pub status: HealthStatus,
    /// Optional human-readable message
    pub message: Option<String>,
}

impl HealthCheck {
    /// Create a new health check.
    pub fn new(name: impl Into<String>, status: HealthStatus) -> Self {
        Self {
            name: name.into(),
            status,
            message: None,
        }
    }

    /// Attach a message to the health check.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Per-node health record containing all checks for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    /// Topology node id this health record belongs to
    pub node_id: String,
    /// Aggregate status for the node
    pub status: HealthStatus,
    /// When the node was last seen / checked
    pub last_seen: DateTime<Utc>,
    /// Individual health checks
    pub checks: Vec<HealthCheck>,
}

impl NodeHealth {
    /// Create a new node health record.
    pub fn new(node_id: impl Into<String>, checks: Vec<HealthCheck>) -> Self {
        let status = Self::aggregate_status(&checks);
        Self {
            node_id: node_id.into(),
            status,
            last_seen: Utc::now(),
            checks,
        }
    }

    /// Derive the aggregate status from a set of checks.
    ///
    /// The worst individual status wins:
    /// Unhealthy > Degraded > Unknown > Healthy
    fn aggregate_status(checks: &[HealthCheck]) -> HealthStatus {
        if checks.is_empty() {
            return HealthStatus::Unknown;
        }
        let mut worst = HealthStatus::Healthy;
        for c in checks {
            worst = worse(worst, c.status);
        }
        worst
    }
}

/// Cluster-wide health summary produced by [`HealthAggregator`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHealthSummary {
    /// Total number of nodes evaluated
    pub total: usize,
    /// Number of healthy nodes
    pub healthy: usize,
    /// Number of degraded nodes
    pub degraded: usize,
    /// Number of unhealthy nodes
    pub unhealthy: usize,
    /// Number of nodes with unknown status
    pub unknown: usize,
    /// Overall cluster status
    pub overall: HealthStatus,
}

/// Aggregates individual node health checks into a cluster-wide summary.
pub struct HealthAggregator;

impl HealthAggregator {
    /// Aggregate per-node health checks against the topology.
    ///
    /// Every node in `topology` that does **not** have a corresponding entry
    /// in `node_checks` is treated as [`HealthStatus::Unknown`].
    pub fn aggregate(
        topology: &ClusterTopology,
        node_checks: &[NodeHealth],
    ) -> ClusterHealthSummary {
        let total = topology.node_count();

        // Build a lookup of node_id -> NodeHealth
        let check_map: std::collections::HashMap<&str, &NodeHealth> = node_checks
            .iter()
            .map(|nh| (nh.node_id.as_str(), nh))
            .collect();

        let mut healthy: usize = 0;
        let mut degraded: usize = 0;
        let mut unhealthy: usize = 0;
        let mut unknown: usize = 0;

        for node in topology.nodes() {
            let status = check_map
                .get(node.id.as_str())
                .map(|nh| nh.status)
                .unwrap_or(HealthStatus::Unknown);
            match status {
                HealthStatus::Healthy => healthy += 1,
                HealthStatus::Degraded => degraded += 1,
                HealthStatus::Unhealthy => unhealthy += 1,
                HealthStatus::Unknown => unknown += 1,
            }
        }

        let overall = if unhealthy > 0 {
            HealthStatus::Unhealthy
        } else if degraded > 0 || unknown > 0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };

        ClusterHealthSummary {
            total,
            healthy,
            degraded,
            unhealthy,
            unknown,
            overall,
        }
    }
}

/// Return the worse of two health statuses.
fn worse(a: HealthStatus, b: HealthStatus) -> HealthStatus {
    fn severity(s: HealthStatus) -> u8 {
        match s {
            HealthStatus::Healthy => 0,
            HealthStatus::Unknown => 1,
            HealthStatus::Degraded => 2,
            HealthStatus::Unhealthy => 3,
        }
    }
    if severity(b) > severity(a) {
        b
    } else {
        a
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::topology::model::{NodeRole, NodeType, TopologyNode};

    fn sample_topology() -> ClusterTopology {
        let mut topo = ClusterTopology::new();
        topo.add_node(TopologyNode::new(
            "ctrl-1",
            "Controller 1",
            NodeType::Controller,
            NodeRole::Leader,
        ));
        topo.add_node(TopologyNode::new(
            "worker-1",
            "Worker 1",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo.add_node(TopologyNode::new(
            "worker-2",
            "Worker 2",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo
    }

    #[test]
    fn test_all_healthy() {
        let topo = sample_topology();
        let checks = vec![
            NodeHealth::new(
                "ctrl-1",
                vec![HealthCheck::new("raft", HealthStatus::Healthy)],
            ),
            NodeHealth::new(
                "worker-1",
                vec![HealthCheck::new("disk", HealthStatus::Healthy)],
            ),
            NodeHealth::new(
                "worker-2",
                vec![HealthCheck::new("disk", HealthStatus::Healthy)],
            ),
        ];

        let summary = HealthAggregator::aggregate(&topo, &checks);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.healthy, 3);
        assert_eq!(summary.overall, HealthStatus::Healthy);
    }

    #[test]
    fn test_mixed_health() {
        let topo = sample_topology();
        let checks = vec![
            NodeHealth::new(
                "ctrl-1",
                vec![HealthCheck::new("raft", HealthStatus::Healthy)],
            ),
            NodeHealth::new(
                "worker-1",
                vec![HealthCheck::new("disk", HealthStatus::Degraded).with_message("90% full")],
            ),
            // worker-2 has no checks -> Unknown
        ];

        let summary = HealthAggregator::aggregate(&topo, &checks);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.healthy, 1);
        assert_eq!(summary.degraded, 1);
        assert_eq!(summary.unknown, 1);
        assert_eq!(summary.overall, HealthStatus::Degraded);
    }

    #[test]
    fn test_unhealthy_dominates() {
        let topo = sample_topology();
        let checks = vec![
            NodeHealth::new(
                "ctrl-1",
                vec![HealthCheck::new("raft", HealthStatus::Unhealthy)],
            ),
            NodeHealth::new(
                "worker-1",
                vec![HealthCheck::new("disk", HealthStatus::Healthy)],
            ),
            NodeHealth::new(
                "worker-2",
                vec![HealthCheck::new("disk", HealthStatus::Healthy)],
            ),
        ];

        let summary = HealthAggregator::aggregate(&topo, &checks);
        assert_eq!(summary.unhealthy, 1);
        assert_eq!(summary.overall, HealthStatus::Unhealthy);
    }
}
