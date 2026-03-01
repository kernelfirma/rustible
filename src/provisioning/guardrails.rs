//! Blast radius protection guardrails
//!
//! Provides pre-apply safety checks that limit the number of resources that
//! can be destroyed or replaced in a single apply.  This prevents accidental
//! mass-deletion scenarios ("fat-finger protection").

use serde::{Deserialize, Serialize};

use super::error::{ProvisioningError, ProvisioningResult};
use super::plan::ExecutionPlan;
use super::traits::ChangeType;

/// Configuration for blast radius guardrails.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlastRadiusConfig {
    /// Maximum absolute number of resources that may be destroyed.
    pub max_destroy_count: Option<usize>,

    /// Maximum percentage of total plan actions that may be destroys (0.0 .. 1.0).
    pub max_destroy_percentage: Option<f64>,

    /// If the number of destructive actions exceeds this threshold, require
    /// explicit approval before proceeding.
    pub require_approval_above: Option<usize>,
}

impl BlastRadiusConfig {
    /// Create a config that limits the absolute destroy count.
    pub fn with_max_count(count: usize) -> Self {
        Self {
            max_destroy_count: Some(count),
            max_destroy_percentage: None,
            require_approval_above: None,
        }
    }

    /// Create a config that limits the destroy percentage.
    pub fn with_max_percentage(percentage: f64) -> Self {
        Self {
            max_destroy_count: None,
            max_destroy_percentage: Some(percentage),
            require_approval_above: None,
        }
    }

    /// Builder: set the approval threshold.
    pub fn with_approval_threshold(mut self, threshold: usize) -> Self {
        self.require_approval_above = Some(threshold);
        self
    }
}

/// Report summarising the blast radius of an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlastRadiusReport {
    /// Number of Destroy actions in the plan.
    pub destroy_count: usize,

    /// Number of Replace actions in the plan (each replace implies a destroy).
    pub replace_count: usize,

    /// Total affected resources (destroy + replace).
    pub total_affected: usize,

    /// Total number of actions in the plan.
    pub total_actions: usize,

    /// Whether the configured limits are exceeded.
    pub exceeds_limit: bool,

    /// Human-readable reason when limits are exceeded.
    pub reason: Option<String>,

    /// Whether explicit approval is required.
    pub requires_approval: bool,
}

/// Count the destructive actions in a plan.
fn count_destructive(plan: &ExecutionPlan) -> (usize, usize) {
    let mut destroy_count = 0usize;
    let mut replace_count = 0usize;

    for action in &plan.actions {
        match action.change_type {
            ChangeType::Destroy => destroy_count += 1,
            ChangeType::Replace => replace_count += 1,
            _ => {}
        }
    }

    (destroy_count, replace_count)
}

/// Assess the blast radius of a plan against the given configuration.
///
/// Returns a [`BlastRadiusReport`] describing the situation.  This function
/// never errors -- use [`check_blast_radius`] if you want an error on
/// violation.
pub fn assess(plan: &ExecutionPlan, config: &BlastRadiusConfig) -> BlastRadiusReport {
    let (destroy_count, replace_count) = count_destructive(plan);
    let total_affected = destroy_count + replace_count;
    let total_actions = plan.actions.len();

    let mut exceeds_limit = false;
    let mut reason = None;

    // Check absolute count limit
    if let Some(max_count) = config.max_destroy_count {
        if total_affected > max_count {
            exceeds_limit = true;
            reason = Some(format!(
                "Destructive actions ({}) exceed maximum allowed ({})",
                total_affected, max_count,
            ));
        }
    }

    // Check percentage limit
    if !exceeds_limit {
        if let Some(max_pct) = config.max_destroy_percentage {
            if total_actions > 0 {
                let actual_pct = total_affected as f64 / total_actions as f64;
                if actual_pct > max_pct {
                    exceeds_limit = true;
                    reason = Some(format!(
                        "Destructive action percentage ({:.1}%) exceeds maximum allowed ({:.1}%)",
                        actual_pct * 100.0,
                        max_pct * 100.0,
                    ));
                }
            }
        }
    }

    // Check approval threshold
    let requires_approval = config
        .require_approval_above
        .map(|threshold| total_affected > threshold)
        .unwrap_or(false);

    BlastRadiusReport {
        destroy_count,
        replace_count,
        total_affected,
        total_actions,
        exceeds_limit,
        reason,
        requires_approval,
    }
}

/// Check the blast radius of a plan against the given configuration.
///
/// Returns `Ok(())` if within limits, or `Err(BlastRadiusExceeded)` if
/// the plan exceeds the configured thresholds.
pub fn check_blast_radius(
    plan: &ExecutionPlan,
    config: &BlastRadiusConfig,
) -> ProvisioningResult<()> {
    let report = assess(plan, config);

    if report.exceeds_limit {
        return Err(ProvisioningError::BlastRadiusExceeded {
            message: report
                .reason
                .unwrap_or_else(|| "Blast radius limit exceeded".to_string()),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::plan::{ExecutionPlan, PlannedAction};
    use crate::provisioning::state::ResourceId;
    use crate::provisioning::traits::{ChangeType, ResourceDiff};

    fn make_action(name: &str, change_type: ChangeType) -> PlannedAction {
        let id = ResourceId::new("aws_instance", name);
        PlannedAction {
            resource_id: id,
            change_type,
            provider: "aws".to_string(),
            diff: ResourceDiff::no_change(),
            reason: String::new(),
            depends_on: vec![],
            parallelizable: true,
        }
    }

    fn plan_with_actions(actions: Vec<PlannedAction>) -> ExecutionPlan {
        let mut plan = ExecutionPlan::empty();
        plan.actions = actions;
        plan
    }

    #[test]
    fn test_no_limits_passes() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Destroy),
        ]);
        let config = BlastRadiusConfig::default();

        check_blast_radius(&plan, &config).unwrap();
    }

    #[test]
    fn test_max_count_within_limit() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Create),
        ]);
        let config = BlastRadiusConfig::with_max_count(2);

        check_blast_radius(&plan, &config).unwrap();
    }

    #[test]
    fn test_max_count_exceeded() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Destroy),
            make_action("c", ChangeType::Destroy),
        ]);
        let config = BlastRadiusConfig::with_max_count(2);

        let err = check_blast_radius(&plan, &config).unwrap_err();
        assert!(matches!(err, ProvisioningError::BlastRadiusExceeded { .. }));
    }

    #[test]
    fn test_replace_counts_as_destructive() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Replace),
        ]);
        let config = BlastRadiusConfig::with_max_count(1);

        let err = check_blast_radius(&plan, &config).unwrap_err();
        assert!(matches!(err, ProvisioningError::BlastRadiusExceeded { .. }));
    }

    #[test]
    fn test_max_percentage_within_limit() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Create),
            make_action("c", ChangeType::Create),
            make_action("d", ChangeType::Create),
        ]);
        // 1 destroy out of 4 = 25%, limit 50%
        let config = BlastRadiusConfig::with_max_percentage(0.5);

        check_blast_radius(&plan, &config).unwrap();
    }

    #[test]
    fn test_max_percentage_exceeded() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Destroy),
            make_action("c", ChangeType::Destroy),
            make_action("d", ChangeType::Create),
        ]);
        // 3 destroys out of 4 = 75%, limit 50%
        let config = BlastRadiusConfig::with_max_percentage(0.5);

        let err = check_blast_radius(&plan, &config).unwrap_err();
        assert!(matches!(err, ProvisioningError::BlastRadiusExceeded { .. }));
    }

    #[test]
    fn test_assess_report_fields() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Replace),
            make_action("c", ChangeType::Create),
            make_action("d", ChangeType::Update),
        ]);
        let config = BlastRadiusConfig::with_max_count(5);

        let report = assess(&plan, &config);
        assert_eq!(report.destroy_count, 1);
        assert_eq!(report.replace_count, 1);
        assert_eq!(report.total_affected, 2);
        assert_eq!(report.total_actions, 4);
        assert!(!report.exceeds_limit);
        assert!(report.reason.is_none());
    }

    #[test]
    fn test_assess_exceeds_reports_reason() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Destroy),
        ]);
        let config = BlastRadiusConfig::with_max_count(1);

        let report = assess(&plan, &config);
        assert!(report.exceeds_limit);
        assert!(report.reason.is_some());
        assert!(report.reason.unwrap().contains("exceed"));
    }

    #[test]
    fn test_approval_threshold() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Destroy),
            make_action("b", ChangeType::Destroy),
            make_action("c", ChangeType::Destroy),
        ]);
        let config = BlastRadiusConfig {
            max_destroy_count: None,
            max_destroy_percentage: None,
            require_approval_above: Some(2),
        };

        let report = assess(&plan, &config);
        assert!(report.requires_approval);
        assert!(!report.exceeds_limit); // no hard limit set
    }

    #[test]
    fn test_approval_threshold_not_met() {
        let plan = plan_with_actions(vec![make_action("a", ChangeType::Destroy)]);
        let config = BlastRadiusConfig {
            max_destroy_count: None,
            max_destroy_percentage: None,
            require_approval_above: Some(5),
        };

        let report = assess(&plan, &config);
        assert!(!report.requires_approval);
    }

    #[test]
    fn test_empty_plan_passes() {
        let plan = plan_with_actions(vec![]);
        let config = BlastRadiusConfig::with_max_count(0);

        check_blast_radius(&plan, &config).unwrap();
    }

    #[test]
    fn test_only_creates_and_updates_pass() {
        let plan = plan_with_actions(vec![
            make_action("a", ChangeType::Create),
            make_action("b", ChangeType::Update),
            make_action("c", ChangeType::Create),
        ]);
        let config = BlastRadiusConfig::with_max_count(0);

        check_blast_radius(&plan, &config).unwrap();
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = BlastRadiusConfig {
            max_destroy_count: Some(10),
            max_destroy_percentage: Some(0.5),
            require_approval_above: Some(5),
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: BlastRadiusConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.max_destroy_count, Some(10));
        assert_eq!(deserialized.max_destroy_percentage, Some(0.5));
        assert_eq!(deserialized.require_approval_above, Some(5));
    }
}
