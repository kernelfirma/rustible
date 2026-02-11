//! Partition-aware rolling update policy for Slurm clusters
//!
//! Defines batch strategies for updating nodes within Slurm partitions
//! without fully draining the cluster capacity.

use serde::{Deserialize, Serialize};

/// Rolling batch configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingBatch {
    /// Maximum number of nodes to update simultaneously.
    pub max_parallel: usize,
    /// Minimum percentage of partition that must remain available.
    pub min_available_pct: f64,
    /// Whether to respect job reservations.
    pub respect_reservations: bool,
}

impl Default for RollingBatch {
    fn default() -> Self {
        Self {
            max_parallel: 5,
            min_available_pct: 75.0,
            respect_reservations: true,
        }
    }
}

/// Partition policy for rolling updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionPolicy {
    /// Partition name in Slurm.
    pub partition_name: String,
    /// Total nodes in partition.
    pub total_nodes: usize,
    /// Rolling batch configuration.
    pub batch: RollingBatch,
}

impl PartitionPolicy {
    /// Create a new partition policy.
    pub fn new(partition_name: impl Into<String>, total_nodes: usize) -> Self {
        Self {
            partition_name: partition_name.into(),
            total_nodes,
            batch: RollingBatch::default(),
        }
    }

    /// Builder: set rolling batch.
    pub fn with_batch(mut self, batch: RollingBatch) -> Self {
        self.batch = batch;
        self
    }

    /// Calculate the maximum nodes that can be taken offline simultaneously
    /// while respecting `min_available_pct`.
    pub fn max_offline(&self) -> usize {
        let min_online =
            ((self.total_nodes as f64) * (self.batch.min_available_pct / 100.0)).ceil() as usize;
        let max_off = self.total_nodes.saturating_sub(min_online);
        max_off.min(self.batch.max_parallel)
    }

    /// Generate update batches for a list of nodes.
    pub fn plan_batches(&self, nodes: &[String]) -> Vec<Vec<String>> {
        let batch_size = self.max_offline().max(1);
        nodes
            .chunks(batch_size)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    /// Check whether the given number of offline nodes is within policy.
    pub fn within_policy(&self, offline_count: usize) -> bool {
        offline_count <= self.max_offline()
    }
}

/// Partition policy module for managing multiple partitions.
pub struct PartitionPolicyModule {
    policies: Vec<PartitionPolicy>,
}

impl PartitionPolicyModule {
    /// Create a new module with no policies.
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Add a partition policy.
    pub fn add_policy(&mut self, policy: PartitionPolicy) {
        self.policies.push(policy);
    }

    /// Get policy for a partition.
    pub fn get_policy(&self, partition: &str) -> Option<&PartitionPolicy> {
        self.policies.iter().find(|p| p.partition_name == partition)
    }

    /// List all configured partitions.
    pub fn partitions(&self) -> Vec<&str> {
        self.policies
            .iter()
            .map(|p| p.partition_name.as_str())
            .collect()
    }

    /// Number of configured policies.
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// Whether any policies are configured.
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }
}

impl Default for PartitionPolicyModule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rolling_batch() {
        let batch = RollingBatch::default();
        assert_eq!(batch.max_parallel, 5);
        assert!((batch.min_available_pct - 75.0).abs() < f64::EPSILON);
        assert!(batch.respect_reservations);
    }

    #[test]
    fn test_max_offline_default() {
        let policy = PartitionPolicy::new("compute", 100);
        // 75% must remain = 75 online, so 25 offline, but max_parallel=5
        assert_eq!(policy.max_offline(), 5);
    }

    #[test]
    fn test_max_offline_relaxed() {
        let policy = PartitionPolicy::new("compute", 100).with_batch(RollingBatch {
            max_parallel: 50,
            min_available_pct: 50.0,
            respect_reservations: false,
        });
        // 50% must remain = 50 online, 50 offline, capped at max_parallel=50
        assert_eq!(policy.max_offline(), 50);
    }

    #[test]
    fn test_max_offline_small_cluster() {
        let policy = PartitionPolicy::new("debug", 4);
        // 75% of 4 = 3 online, 1 offline, min(1, 5) = 1
        assert_eq!(policy.max_offline(), 1);
    }

    #[test]
    fn test_max_offline_single_node() {
        let policy = PartitionPolicy::new("single", 1);
        // 75% of 1 = 1 online, 0 offline
        assert_eq!(policy.max_offline(), 0);
    }

    #[test]
    fn test_plan_batches() {
        let policy = PartitionPolicy::new("compute", 20).with_batch(RollingBatch {
            max_parallel: 3,
            min_available_pct: 75.0,
            respect_reservations: true,
        });
        let nodes: Vec<String> = (0..10).map(|i| format!("node{:02}", i)).collect();
        let batches = policy.plan_batches(&nodes);
        assert_eq!(batches.len(), 4); // 3+3+3+1
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[3].len(), 1);
    }

    #[test]
    fn test_plan_batches_empty() {
        let policy = PartitionPolicy::new("compute", 100);
        let batches = policy.plan_batches(&[]);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_within_policy() {
        let policy = PartitionPolicy::new("compute", 100);
        assert!(policy.within_policy(3));
        assert!(policy.within_policy(5));
        assert!(!policy.within_policy(6));
    }

    #[test]
    fn test_module_add_get() {
        let mut module = PartitionPolicyModule::new();
        assert!(module.is_empty());

        module.add_policy(PartitionPolicy::new("compute", 100));
        module.add_policy(PartitionPolicy::new("gpu", 10));

        assert_eq!(module.len(), 2);
        assert!(!module.is_empty());
        assert_eq!(module.partitions().len(), 2);
        assert!(module.get_policy("compute").is_some());
        assert!(module.get_policy("gpu").is_some());
        assert!(module.get_policy("nonexistent").is_none());
    }

    #[test]
    fn test_module_default() {
        let module = PartitionPolicyModule::default();
        assert!(module.is_empty());
    }

    #[test]
    fn test_partition_policy_name() {
        let policy = PartitionPolicy::new("gpu-partition", 16);
        assert_eq!(policy.partition_name, "gpu-partition");
        assert_eq!(policy.total_nodes, 16);
    }
}
