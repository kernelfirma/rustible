//! Moved blocks for declarative resource address refactoring
//!
//! Mirrors Terraform's `moved {}` block: before planning, the executor
//! applies pending moves so the plan sees resources at their new addresses.

use serde::{Deserialize, Serialize};
use tracing::info;

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::ProvisioningState;
use super::state_ops;

/// A single `moved` block from configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovedBlock {
    /// Original resource address (e.g. "aws_vpc.old_name")
    pub from: String,
    /// New resource address (e.g. "aws_vpc.new_name")
    pub to: String,
}

impl MovedBlock {
    /// Create a new moved block.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// Apply a list of moved blocks to state **in order**.
///
/// Each block is attempted once; if the source no longer exists in state the
/// move is silently skipped (it may have been applied in a previous run).
/// Returns the number of moves actually executed.
pub fn apply_moved_blocks(
    state: &mut ProvisioningState,
    blocks: &[MovedBlock],
) -> ProvisioningResult<usize> {
    let mut applied = 0usize;

    for block in blocks {
        match state_ops::state_mv(state, &block.from, &block.to) {
            Ok(()) => {
                info!("Applied moved block: {} -> {}", block.from, block.to);
                applied += 1;
            }
            Err(ProvisioningError::ResourceNotInState(_)) => {
                // Already moved or never existed -- skip
                info!(
                    "Skipping moved block {} -> {} (source not in state)",
                    block.from, block.to
                );
            }
            Err(e) => return Err(e),
        }
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::{ProvisioningState, ResourceId, ResourceState};

    fn sample_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();
        state.add_resource(ResourceState::new(
            ResourceId::new("aws_vpc", "old"),
            "vpc-123",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({}),
        ));
        state
    }

    #[test]
    fn test_apply_moved_blocks_success() {
        let mut state = sample_state();
        let blocks = vec![MovedBlock::new("aws_vpc.old", "aws_vpc.new")];
        let count = apply_moved_blocks(&mut state, &blocks).unwrap();
        assert_eq!(count, 1);
        assert!(state.get_resource(&ResourceId::new("aws_vpc", "old")).is_none());
        assert!(state.get_resource(&ResourceId::new("aws_vpc", "new")).is_some());
    }

    #[test]
    fn test_apply_moved_blocks_idempotent() {
        let mut state = sample_state();
        let blocks = vec![MovedBlock::new("aws_vpc.old", "aws_vpc.new")];
        apply_moved_blocks(&mut state, &blocks).unwrap();
        // Second run -- source is gone, should skip
        let count = apply_moved_blocks(&mut state, &blocks).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_apply_moved_blocks_chain() {
        let mut state = sample_state();
        let blocks = vec![
            MovedBlock::new("aws_vpc.old", "aws_vpc.mid"),
            MovedBlock::new("aws_vpc.mid", "aws_vpc.final_name"),
        ];
        let count = apply_moved_blocks(&mut state, &blocks).unwrap();
        assert_eq!(count, 2);
        assert!(state.get_resource(&ResourceId::new("aws_vpc", "final_name")).is_some());
    }
}
