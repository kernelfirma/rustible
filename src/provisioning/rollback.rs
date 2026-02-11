//! Deterministic rollback for failed applies
//!
//! When a provisioning apply partially fails, this module records the
//! successful actions so they can be reversed in order to restore the
//! infrastructure to its pre-apply state.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The kind of rollback action to take in order to undo a successful apply step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackAction {
    /// The original action was Create -- undo by destroying the resource.
    UndoCreate,
    /// The original action was Update -- undo by restoring the previous config.
    UndoUpdate,
    /// The original action was Destroy -- undo by recreating from saved state.
    UndoDestroy,
    /// The original action was Replace -- undo by recreating the original resource.
    UndoReplace,
}

/// A single entry in the rollback log, representing one action that succeeded
/// and may need to be reversed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackEntry {
    /// Fully-qualified resource address (e.g. `aws_vpc.main`).
    pub resource_address: String,

    /// What rollback operation is needed to undo this step.
    pub action: RollbackAction,

    /// The resource state *before* the action was applied.  `None` for
    /// creates (where there was no previous state).
    pub previous_state: Option<Value>,
}

impl RollbackEntry {
    /// Create a new rollback entry.
    pub fn new(
        resource_address: impl Into<String>,
        action: RollbackAction,
        previous_state: Option<Value>,
    ) -> Self {
        Self {
            resource_address: resource_address.into(),
            action,
            previous_state,
        }
    }
}

/// Accumulates rollback entries during an apply and can produce a
/// reversed rollback plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningRollback {
    /// Ordered list of recorded entries (in the order they were applied).
    entries: Vec<RollbackEntry>,

    /// Identifier of the execution plan this rollback belongs to.
    pub plan_id: String,
}

impl ProvisioningRollback {
    /// Create a new, empty rollback tracker for the given plan.
    pub fn new(plan_id: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            plan_id: plan_id.into(),
        }
    }

    /// Record a successful action so it can later be reversed.
    ///
    /// Call this *after* each action succeeds during apply.
    pub fn record(&mut self, entry: RollbackEntry) {
        self.entries.push(entry);
    }

    /// Return the recorded entries in **reverse** order, suitable for
    /// executing as a rollback plan.
    pub fn rollback_plan(&self) -> Vec<&RollbackEntry> {
        self.entries.iter().rev().collect()
    }

    /// Return the number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check whether any entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Discard all recorded entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Consume self and return the entries in reverse order (owned).
    pub fn into_rollback_plan(self) -> Vec<RollbackEntry> {
        let mut entries = self.entries;
        entries.reverse();
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_rollback_is_empty() {
        let rb = ProvisioningRollback::new("plan-1");
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.plan_id, "plan-1");
    }

    #[test]
    fn test_record_and_len() {
        let mut rb = ProvisioningRollback::new("plan-2");

        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoCreate,
            None,
        ));
        assert_eq!(rb.len(), 1);
        assert!(!rb.is_empty());

        rb.record(RollbackEntry::new(
            "aws_subnet.public",
            RollbackAction::UndoCreate,
            None,
        ));
        assert_eq!(rb.len(), 2);
    }

    #[test]
    fn test_rollback_plan_reverses_order() {
        let mut rb = ProvisioningRollback::new("plan-3");

        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoCreate,
            None,
        ));
        rb.record(RollbackEntry::new(
            "aws_subnet.public",
            RollbackAction::UndoCreate,
            None,
        ));
        rb.record(RollbackEntry::new(
            "aws_instance.web",
            RollbackAction::UndoCreate,
            None,
        ));

        let plan = rb.rollback_plan();
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].resource_address, "aws_instance.web");
        assert_eq!(plan[1].resource_address, "aws_subnet.public");
        assert_eq!(plan[2].resource_address, "aws_vpc.main");
    }

    #[test]
    fn test_rollback_preserves_previous_state() {
        let mut rb = ProvisioningRollback::new("plan-4");

        let prev = serde_json::json!({"cidr_block": "10.0.0.0/16", "tags": {"Name": "old"}});
        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoUpdate,
            Some(prev.clone()),
        ));

        let plan = rb.rollback_plan();
        assert_eq!(plan[0].action, RollbackAction::UndoUpdate);
        assert_eq!(plan[0].previous_state, Some(prev));
    }

    #[test]
    fn test_rollback_action_variants() {
        let mut rb = ProvisioningRollback::new("plan-5");

        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoCreate,
            None,
        ));
        rb.record(RollbackEntry::new(
            "aws_subnet.public",
            RollbackAction::UndoUpdate,
            Some(serde_json::json!({"cidr": "10.0.1.0/24"})),
        ));
        rb.record(RollbackEntry::new(
            "aws_sg.web",
            RollbackAction::UndoDestroy,
            Some(serde_json::json!({"name": "web-sg"})),
        ));
        rb.record(RollbackEntry::new(
            "aws_instance.app",
            RollbackAction::UndoReplace,
            Some(serde_json::json!({"ami": "ami-old"})),
        ));

        let plan = rb.rollback_plan();
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0].action, RollbackAction::UndoReplace);
        assert_eq!(plan[1].action, RollbackAction::UndoDestroy);
        assert_eq!(plan[2].action, RollbackAction::UndoUpdate);
        assert_eq!(plan[3].action, RollbackAction::UndoCreate);
    }

    #[test]
    fn test_clear() {
        let mut rb = ProvisioningRollback::new("plan-6");
        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoCreate,
            None,
        ));
        rb.record(RollbackEntry::new(
            "aws_subnet.public",
            RollbackAction::UndoCreate,
            None,
        ));
        assert_eq!(rb.len(), 2);

        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert!(rb.rollback_plan().is_empty());
    }

    #[test]
    fn test_into_rollback_plan() {
        let mut rb = ProvisioningRollback::new("plan-7");
        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoCreate,
            None,
        ));
        rb.record(RollbackEntry::new(
            "aws_subnet.public",
            RollbackAction::UndoCreate,
            None,
        ));

        let plan = rb.into_rollback_plan();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].resource_address, "aws_subnet.public");
        assert_eq!(plan[1].resource_address, "aws_vpc.main");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut rb = ProvisioningRollback::new("plan-8");
        rb.record(RollbackEntry::new(
            "aws_vpc.main",
            RollbackAction::UndoUpdate,
            Some(serde_json::json!({"cidr_block": "10.0.0.0/16"})),
        ));

        let json = serde_json::to_string(&rb).expect("serialize");
        let deserialized: ProvisioningRollback = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.plan_id, "plan-8");
        assert_eq!(deserialized.len(), 1);
        let plan = deserialized.rollback_plan();
        assert_eq!(plan[0].resource_address, "aws_vpc.main");
        assert_eq!(plan[0].action, RollbackAction::UndoUpdate);
    }

    #[test]
    fn test_empty_rollback_plan() {
        let rb = ProvisioningRollback::new("plan-9");
        let plan = rb.rollback_plan();
        assert!(plan.is_empty());
    }
}
