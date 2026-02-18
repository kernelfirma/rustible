//! Canary deployment strategy for provisioning
//!
//! This module provides a canary deployment strategy that splits an execution
//! plan into a small "canary" batch and a larger "remaining" batch.  The canary
//! batch is applied first; if it succeeds the remaining batch can proceed.  An
//! optional pause between batches allows operators to validate the canary.

use serde::{Deserialize, Serialize};

use super::plan::PlannedAction;

/// Configuration for the canary deployment strategy.
///
/// Exactly one of `canary_count` or `canary_percentage` should be set.
/// If both are set, `canary_count` takes precedence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryStrategy {
    /// Explicit number of actions to include in the canary batch.
    pub canary_count: Option<usize>,

    /// Percentage of total actions to include in the canary batch (0.0 .. 1.0).
    pub canary_percentage: Option<f64>,

    /// Whether to pause after the canary batch before applying the rest.
    #[serde(default)]
    pub pause_after_canary: bool,
}

impl CanaryStrategy {
    /// Create a strategy that selects a fixed number of canary actions.
    pub fn with_count(count: usize) -> Self {
        Self {
            canary_count: Some(count),
            canary_percentage: None,
            pause_after_canary: false,
        }
    }

    /// Create a strategy that selects a percentage of actions as canary.
    ///
    /// `percentage` should be between 0.0 and 1.0 (e.g. 0.1 for 10%).
    pub fn with_percentage(percentage: f64) -> Self {
        Self {
            canary_count: None,
            canary_percentage: Some(percentage),
            pause_after_canary: false,
        }
    }

    /// Builder: set whether to pause after the canary batch.
    pub fn with_pause(mut self, pause: bool) -> Self {
        self.pause_after_canary = pause;
        self
    }
}

/// Executor that splits a plan's actions into canary and remaining batches.
#[derive(Debug, Clone)]
pub struct CanaryExecutor<'a> {
    /// The canary strategy configuration.
    pub strategy: &'a CanaryStrategy,

    /// The actions from the execution plan to split.
    actions: &'a [PlannedAction],
}

impl<'a> CanaryExecutor<'a> {
    /// Create a new canary executor for the given strategy and plan actions.
    pub fn new(strategy: &'a CanaryStrategy, actions: &'a [PlannedAction]) -> Self {
        Self { strategy, actions }
    }

    /// Resolve the effective canary count.
    ///
    /// If `canary_count` is set it is used directly (clamped to total).
    /// Otherwise `canary_percentage` is applied.  If neither is set, returns 0.
    /// The result is always at least 1 when there are actions and a strategy
    /// value is configured, and never exceeds the total number of actions.
    pub fn effective_canary_count(&self) -> usize {
        let total = self.actions.len();
        if total == 0 {
            return 0;
        }

        if let Some(count) = self.strategy.canary_count {
            return count.min(total);
        }

        if let Some(pct) = self.strategy.canary_percentage {
            let clamped = pct.clamp(0.0, 1.0);
            let computed = (total as f64 * clamped).ceil() as usize;
            // Ensure at least 1 when percentage > 0 and there are actions
            return if computed == 0 && clamped > 0.0 {
                1
            } else {
                computed.min(total)
            };
        }

        0
    }

    /// Select the canary actions (the first N actions in plan order).
    pub fn select_canary_actions(&self) -> &'a [PlannedAction] {
        let n = self.effective_canary_count();
        &self.actions[..n]
    }

    /// Split the plan into (canary_actions, remaining_actions).
    pub fn split_plan(&self) -> (&'a [PlannedAction], &'a [PlannedAction]) {
        let n = self.effective_canary_count();
        (&self.actions[..n], &self.actions[n..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::plan::PlannedAction;
    use crate::provisioning::state::ResourceId;
    use crate::provisioning::traits::{ChangeType, ResourceDiff};

    fn make_actions(count: usize) -> Vec<PlannedAction> {
        (0..count)
            .map(|i| {
                let id = ResourceId::new("aws_instance", format!("server_{}", i));
                PlannedAction {
                    resource_id: id,
                    change_type: ChangeType::Create,
                    provider: "aws".to_string(),
                    diff: ResourceDiff::no_change(),
                    reason: format!("test action {}", i),
                    depends_on: vec![],
                    parallelizable: true,
                }
            })
            .collect()
    }

    #[test]
    fn test_canary_with_count() {
        let strategy = CanaryStrategy::with_count(2);
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 2);
        assert_eq!(executor.select_canary_actions().len(), 2);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 2);
        assert_eq!(remaining.len(), 8);
    }

    #[test]
    fn test_canary_with_percentage() {
        let strategy = CanaryStrategy::with_percentage(0.2);
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        // 10 * 0.2 = 2.0, ceil = 2
        assert_eq!(executor.effective_canary_count(), 2);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 2);
        assert_eq!(remaining.len(), 8);
    }

    #[test]
    fn test_canary_percentage_rounds_up() {
        let strategy = CanaryStrategy::with_percentage(0.15);
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        // 10 * 0.15 = 1.5, ceil = 2
        assert_eq!(executor.effective_canary_count(), 2);
    }

    #[test]
    fn test_canary_count_clamped_to_total() {
        let strategy = CanaryStrategy::with_count(100);
        let actions = make_actions(5);
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 5);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 5);
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_canary_percentage_100_takes_all() {
        let strategy = CanaryStrategy::with_percentage(1.0);
        let actions = make_actions(5);
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 5);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 5);
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_canary_empty_actions() {
        let strategy = CanaryStrategy::with_count(3);
        let actions: Vec<PlannedAction> = vec![];
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 0);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 0);
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_canary_no_strategy_values() {
        let strategy = CanaryStrategy {
            canary_count: None,
            canary_percentage: None,
            pause_after_canary: false,
        };
        let actions = make_actions(5);
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 0);

        let (canary, remaining) = executor.split_plan();
        assert_eq!(canary.len(), 0);
        assert_eq!(remaining.len(), 5);
    }

    #[test]
    fn test_canary_count_takes_precedence_over_percentage() {
        let strategy = CanaryStrategy {
            canary_count: Some(3),
            canary_percentage: Some(0.5),
            pause_after_canary: false,
        };
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        // count=3 takes precedence over percentage=50%=5
        assert_eq!(executor.effective_canary_count(), 3);
    }

    #[test]
    fn test_canary_pause_flag() {
        let strategy = CanaryStrategy::with_count(1).with_pause(true);
        assert!(strategy.pause_after_canary);
    }

    #[test]
    fn test_canary_very_small_percentage_gives_at_least_one() {
        let strategy = CanaryStrategy::with_percentage(0.001);
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        // 10 * 0.001 = 0.01, ceil = 1
        assert_eq!(executor.effective_canary_count(), 1);
    }

    #[test]
    fn test_canary_zero_percentage() {
        let strategy = CanaryStrategy::with_percentage(0.0);
        let actions = make_actions(10);
        let executor = CanaryExecutor::new(&strategy, &actions);

        assert_eq!(executor.effective_canary_count(), 0);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let strategy = CanaryStrategy {
            canary_count: Some(3),
            canary_percentage: Some(0.1),
            pause_after_canary: true,
        };

        let json = serde_json::to_string(&strategy).expect("serialize");
        let deserialized: CanaryStrategy = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.canary_count, Some(3));
        assert_eq!(deserialized.canary_percentage, Some(0.1));
        assert!(deserialized.pause_after_canary);
    }
}
