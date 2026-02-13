//! Fanout controller for tier-based work distribution
//!
//! Distributes work units across multiple tiers of hosts for scaled execution.
//! Supports flat, hierarchical, and geographic distribution strategies.

use serde::{Deserialize, Serialize};

use crate::distributed::types::WorkUnit;

/// Configuration for fanout distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutConfig {
    /// Maximum concurrent operations across all tiers.
    pub max_concurrent: usize,
    /// Default batch size per tier.
    pub batch_size: usize,
    /// Distribution strategy.
    pub tier_strategy: TierStrategy,
}

impl Default for FanoutConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 100,
            batch_size: 25,
            tier_strategy: TierStrategy::Flat,
        }
    }
}

/// Strategy for distributing work across tiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TierStrategy {
    /// All hosts treated equally.
    Flat,
    /// Hierarchical distribution with priority tiers.
    Hierarchical { tiers: Vec<TierConfig> },
    /// Distribute by geographic region.
    Geographic,
}

/// Configuration for a single tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierConfig {
    /// Tier name.
    pub name: String,
    /// Maximum hosts in this tier.
    pub max_hosts: usize,
    /// Priority (lower = higher priority).
    pub priority: u32,
}

/// Result of fanout distribution.
#[derive(Debug, Clone)]
pub struct FanoutResult {
    /// Distribution by tier.
    pub tiers: Vec<TierResult>,
    /// Total units distributed.
    pub total_distributed: usize,
}

/// Per-tier distribution result.
#[derive(Debug, Clone)]
pub struct TierResult {
    /// Tier name.
    pub tier_name: String,
    /// Work units assigned to this tier.
    pub units: Vec<WorkUnit>,
}

impl TierResult {
    /// Number of work units in this tier.
    pub fn count(&self) -> usize {
        self.units.len()
    }
}

/// Controller managing tier-based work distribution.
pub struct FanoutController {
    config: FanoutConfig,
}

impl FanoutController {
    /// Create a new fanout controller.
    pub fn new(config: FanoutConfig) -> Self {
        Self { config }
    }

    /// Distribute work units across tiers according to strategy.
    pub fn distribute(&self, units: Vec<WorkUnit>) -> FanoutResult {
        match &self.config.tier_strategy {
            TierStrategy::Flat => self.distribute_flat(units),
            TierStrategy::Hierarchical { tiers } => self.distribute_hierarchical(units, tiers),
            TierStrategy::Geographic => self.distribute_flat(units), // fallback
        }
    }

    fn distribute_flat(&self, units: Vec<WorkUnit>) -> FanoutResult {
        let total = units.len();
        let batches: Vec<Vec<WorkUnit>> = units
            .chunks(self.config.batch_size)
            .map(|c| c.to_vec())
            .collect();

        let tiers = batches
            .into_iter()
            .enumerate()
            .map(|(i, batch)| TierResult {
                tier_name: format!("batch-{}", i),
                units: batch,
            })
            .collect();

        FanoutResult {
            tiers,
            total_distributed: total,
        }
    }

    fn distribute_hierarchical(
        &self,
        mut units: Vec<WorkUnit>,
        tier_configs: &[TierConfig],
    ) -> FanoutResult {
        let mut sorted_tiers: Vec<&TierConfig> = tier_configs.iter().collect();
        sorted_tiers.sort_by_key(|t| t.priority);

        let mut result_tiers = Vec::new();
        let mut total = 0usize;

        for tier in sorted_tiers {
            if units.is_empty() {
                break;
            }
            let take = units.len().min(tier.max_hosts);
            let batch: Vec<WorkUnit> = units.drain(..take).collect();
            total += batch.len();
            result_tiers.push(TierResult {
                tier_name: tier.name.clone(),
                units: batch,
            });
        }

        // Any remaining go into overflow
        if !units.is_empty() {
            total += units.len();
            result_tiers.push(TierResult {
                tier_name: "overflow".to_string(),
                units,
            });
        }

        FanoutResult {
            tiers: result_tiers,
            total_distributed: total,
        }
    }

    /// Get the configured batch size.
    pub fn batch_size(&self) -> usize {
        self.config.batch_size
    }

    /// Determine which tier a host should be assigned to based on hints.
    ///
    /// Returns the tier name from hierarchical config, or `"default"` for flat
    /// strategies.
    pub fn tier_for_host(&self, _host: &str) -> String {
        match &self.config.tier_strategy {
            TierStrategy::Flat => "default".to_string(),
            TierStrategy::Hierarchical { tiers } => tiers
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "default".to_string()),
            TierStrategy::Geographic => "default".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::types::{HostId, RunId, WorkUnit};

    fn make_units(n: usize) -> Vec<WorkUnit> {
        (0..n)
            .map(|i| {
                WorkUnit::new(
                    RunId::new(format!("run-{}", i)),
                    0,
                    vec![HostId::new(format!("host-{}", i))],
                )
            })
            .collect()
    }

    #[test]
    fn test_flat_distribution() {
        let ctrl = FanoutController::new(FanoutConfig {
            batch_size: 3,
            ..Default::default()
        });
        let result = ctrl.distribute(make_units(7));
        assert_eq!(result.total_distributed, 7);
        assert_eq!(result.tiers.len(), 3); // 3+3+1
        assert_eq!(result.tiers[0].units.len(), 3);
        assert_eq!(result.tiers[2].units.len(), 1);
    }

    #[test]
    fn test_hierarchical_distribution() {
        let tiers = vec![
            TierConfig {
                name: "canary".into(),
                max_hosts: 2,
                priority: 0,
            },
            TierConfig {
                name: "main".into(),
                max_hosts: 10,
                priority: 1,
            },
        ];
        let ctrl = FanoutController::new(FanoutConfig {
            tier_strategy: TierStrategy::Hierarchical { tiers },
            ..Default::default()
        });
        let result = ctrl.distribute(make_units(5));
        assert_eq!(result.total_distributed, 5);
        assert_eq!(result.tiers[0].tier_name, "canary");
        assert_eq!(result.tiers[0].units.len(), 2);
        assert_eq!(result.tiers[1].tier_name, "main");
        assert_eq!(result.tiers[1].units.len(), 3);
    }

    #[test]
    fn test_empty_distribution() {
        let ctrl = FanoutController::new(FanoutConfig::default());
        let result = ctrl.distribute(vec![]);
        assert_eq!(result.total_distributed, 0);
        assert!(result.tiers.is_empty());
    }

    #[test]
    fn test_overflow_tier() {
        let tiers = vec![TierConfig {
            name: "small".into(),
            max_hosts: 2,
            priority: 0,
        }];
        let ctrl = FanoutController::new(FanoutConfig {
            tier_strategy: TierStrategy::Hierarchical { tiers },
            ..Default::default()
        });
        let result = ctrl.distribute(make_units(5));
        assert_eq!(result.tiers.len(), 2);
        assert_eq!(result.tiers[1].tier_name, "overflow");
        assert_eq!(result.tiers[1].units.len(), 3);
    }

    #[test]
    fn test_batch_size() {
        let ctrl = FanoutController::new(FanoutConfig {
            batch_size: 42,
            ..Default::default()
        });
        assert_eq!(ctrl.batch_size(), 42);
    }

    #[test]
    fn test_tier_for_host_flat() {
        let ctrl = FanoutController::new(FanoutConfig::default());
        assert_eq!(ctrl.tier_for_host("host-1"), "default");
    }

    #[test]
    fn test_tier_for_host_hierarchical() {
        let tiers = vec![TierConfig {
            name: "primary".into(),
            max_hosts: 10,
            priority: 0,
        }];
        let ctrl = FanoutController::new(FanoutConfig {
            tier_strategy: TierStrategy::Hierarchical { tiers },
            ..Default::default()
        });
        assert_eq!(ctrl.tier_for_host("host-1"), "primary");
    }

    #[test]
    fn test_tier_result_count() {
        let result = TierResult {
            tier_name: "test".to_string(),
            units: make_units(3),
        };
        assert_eq!(result.count(), 3);
    }
}
