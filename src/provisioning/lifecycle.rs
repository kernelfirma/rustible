//! Resource lifecycle configuration
//!
//! Mirrors Terraform's `lifecycle {}` block, providing:
//! - `prevent_destroy`: blocks plans that would destroy the resource
//! - `replace_triggered_by`: triggers replacement when referenced resources change

use serde::{Deserialize, Serialize};

use super::error::{ProvisioningError, ProvisioningResult};
use super::plan::ExecutionPlan;
use super::traits::ChangeType;

/// Per-resource lifecycle configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// If `true`, any plan that would destroy this resource is rejected.
    #[serde(default)]
    pub prevent_destroy: bool,

    /// List of resource addresses whose changes trigger replacement of
    /// the owning resource.
    #[serde(default)]
    pub replace_triggered_by: Vec<String>,
}

impl LifecycleConfig {
    /// Create a default (permissive) lifecycle config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: set prevent_destroy.
    pub fn with_prevent_destroy(mut self, value: bool) -> Self {
        self.prevent_destroy = value;
        self
    }

    /// Builder: add a replacement trigger.
    pub fn with_trigger(mut self, address: impl Into<String>) -> Self {
        self.replace_triggered_by.push(address.into());
        self
    }
}

/// Check an execution plan against lifecycle `prevent_destroy` rules.
///
/// Returns `Err(PreventDestroyViolation)` if any action in the plan would
/// destroy a resource that has `prevent_destroy: true`.
pub fn check_prevent_destroy(
    plan: &ExecutionPlan,
    lifecycles: &std::collections::HashMap<String, LifecycleConfig>,
) -> ProvisioningResult<()> {
    for action in &plan.actions {
        let destroys = matches!(
            action.change_type,
            ChangeType::Destroy | ChangeType::Replace
        );
        if !destroys {
            continue;
        }
        let address = action.resource_id.address();
        if let Some(lc) = lifecycles.get(&address) {
            if lc.prevent_destroy {
                return Err(ProvisioningError::PreventDestroyViolation {
                    resource: address,
                });
            }
        }
    }
    Ok(())
}

/// Promote resources to `Replace` if any of their `replace_triggered_by`
/// targets have changed in the plan.
///
/// Returns the number of actions promoted to Replace.
pub fn apply_replace_triggers(
    plan: &mut ExecutionPlan,
    lifecycles: &std::collections::HashMap<String, LifecycleConfig>,
) -> usize {
    // Collect addresses of resources that have changes in this plan
    let changed_addresses: std::collections::HashSet<String> = plan
        .actions
        .iter()
        .filter(|a| a.change_type != ChangeType::NoOp)
        .map(|a| a.resource_id.address())
        .collect();

    let mut promoted = 0usize;

    for action in &mut plan.actions {
        if matches!(action.change_type, ChangeType::Destroy | ChangeType::Replace) {
            continue; // already destructive
        }
        let address = action.resource_id.address();
        if let Some(lc) = lifecycles.get(&address) {
            let triggered = lc
                .replace_triggered_by
                .iter()
                .any(|trigger| changed_addresses.contains(trigger));
            if triggered {
                action.change_type = ChangeType::Replace;
                action.reason = format!(
                    "Replacement triggered by changes in: {:?}",
                    lc.replace_triggered_by
                );
                promoted += 1;
            }
        }
    }

    promoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::plan::{ExecutionPlan, PlannedAction};
    use super::super::state::ResourceId;
    use super::super::traits::{ChangeType, ResourceDiff};
    use std::collections::HashMap;

    fn make_action(addr: &str, change: ChangeType) -> PlannedAction {
        let id = ResourceId::from_address(addr).unwrap();
        let (provider, _) = addr.split_once('.').unwrap();
        PlannedAction {
            resource_id: id,
            change_type: change,
            provider: provider.to_string(),
            diff: ResourceDiff::no_change(),
            reason: String::new(),
            depends_on: vec![],
            parallelizable: true,
        }
    }

    #[test]
    fn test_prevent_destroy_blocks() {
        let mut plan = ExecutionPlan::empty();
        plan.actions.push(make_action("aws_rds.db", ChangeType::Destroy));

        let mut lc = HashMap::new();
        lc.insert(
            "aws_rds.db".to_string(),
            LifecycleConfig::new().with_prevent_destroy(true),
        );

        let err = check_prevent_destroy(&plan, &lc).unwrap_err();
        assert!(matches!(err, ProvisioningError::PreventDestroyViolation { .. }));
    }

    #[test]
    fn test_prevent_destroy_allows_create() {
        let mut plan = ExecutionPlan::empty();
        plan.actions.push(make_action("aws_rds.db", ChangeType::Create));

        let mut lc = HashMap::new();
        lc.insert(
            "aws_rds.db".to_string(),
            LifecycleConfig::new().with_prevent_destroy(true),
        );

        check_prevent_destroy(&plan, &lc).unwrap();
    }

    #[test]
    fn test_replace_triggers() {
        let mut plan = ExecutionPlan::empty();
        plan.actions.push(make_action("aws_ami.latest", ChangeType::Update));
        plan.actions.push(make_action("aws_instance.web", ChangeType::NoOp));

        let mut lc = HashMap::new();
        lc.insert(
            "aws_instance.web".to_string(),
            LifecycleConfig::new().with_trigger("aws_ami.latest"),
        );

        let promoted = apply_replace_triggers(&mut plan, &lc);
        assert_eq!(promoted, 1);
        assert_eq!(plan.actions[1].change_type, ChangeType::Replace);
    }

    #[test]
    fn test_no_false_trigger() {
        let mut plan = ExecutionPlan::empty();
        plan.actions.push(make_action("aws_instance.web", ChangeType::NoOp));

        let mut lc = HashMap::new();
        lc.insert(
            "aws_instance.web".to_string(),
            LifecycleConfig::new().with_trigger("aws_ami.latest"),
        );

        let promoted = apply_replace_triggers(&mut plan, &lc);
        assert_eq!(promoted, 0);
    }
}
