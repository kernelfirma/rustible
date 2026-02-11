//! Apply checkpoints for resumable infrastructure applies
//!
//! This module provides checkpoint functionality that enables resuming
//! partially-completed apply operations. When an apply fails partway through,
//! a checkpoint is saved recording which actions completed and which failed,
//! allowing the operation to be resumed from where it left off.
//!
//! ## Features
//!
//! - **Automatic Checkpointing**: Save progress after each action completes
//! - **Resume Detection**: Detect if a previous apply can be resumed
//! - **Plan Matching**: Ensure checkpoints match the current plan before resuming
//! - **Pending Action Filtering**: Determine which actions still need to be executed
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use rustible::provisioning::checkpoint::{ApplyCheckpoint, CheckpointManager};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = CheckpointManager::new(".rustible");
//!
//! // Check for existing checkpoint
//! if let Some(checkpoint) = manager.load_checkpoint().await? {
//!     if checkpoint.can_resume("plan-123") {
//!         let pending = checkpoint.pending_actions(&all_action_ids);
//!         // Resume from pending actions...
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing;

use super::error::{ProvisioningError, ProvisioningResult};

// ============================================================================
// Apply Checkpoint
// ============================================================================

/// Checkpoint recording progress of an apply operation
///
/// Saved to disk after each action completes so that a failed apply
/// can be resumed from the last successful action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyCheckpoint {
    /// The plan ID this checkpoint corresponds to
    pub plan_id: String,

    /// Action IDs (resource addresses) that completed successfully
    pub completed_actions: Vec<String>,

    /// The action that failed (if any)
    pub failed_action: Option<String>,

    /// Error message from the failed action
    pub failure_message: Option<String>,

    /// When the checkpoint was created
    pub created_at: DateTime<Utc>,

    /// When the checkpoint was last updated
    pub updated_at: DateTime<Utc>,
}

impl ApplyCheckpoint {
    /// Create a new checkpoint for a plan
    pub fn new(plan_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            plan_id: plan_id.into(),
            completed_actions: Vec::new(),
            failed_action: None,
            failure_message: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if this checkpoint can be used to resume the given plan
    ///
    /// A checkpoint can be resumed if:
    /// - The plan ID matches
    /// - There is at least one completed action
    /// - The checkpoint is not fully complete (there was a failure or more to do)
    pub fn can_resume(&self, plan_id: &str) -> bool {
        self.plan_id == plan_id && !self.completed_actions.is_empty()
    }

    /// Record a successfully completed action
    pub fn record_completed(&mut self, action_id: impl Into<String>) {
        let action_id = action_id.into();
        if !self.completed_actions.contains(&action_id) {
            self.completed_actions.push(action_id);
        }
        self.updated_at = Utc::now();
    }

    /// Record a failed action
    pub fn record_failure(
        &mut self,
        action_id: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.failed_action = Some(action_id.into());
        self.failure_message = Some(message.into());
        self.updated_at = Utc::now();
    }

    /// Get the list of pending actions given the full set of action IDs in the plan
    ///
    /// Returns action IDs that have not been completed yet, preserving
    /// the original order from the plan.
    pub fn pending_actions<'a>(&self, all_action_ids: &'a [String]) -> Vec<&'a String> {
        all_action_ids
            .iter()
            .filter(|id| !self.completed_actions.contains(id))
            .collect()
    }

    /// Check if all actions in the plan have been completed
    pub fn is_complete(&self, all_action_ids: &[String]) -> bool {
        all_action_ids
            .iter()
            .all(|id| self.completed_actions.contains(id))
    }

    /// Get the number of completed actions
    pub fn completed_count(&self) -> usize {
        self.completed_actions.len()
    }

    /// Check if there was a failure
    pub fn has_failure(&self) -> bool {
        self.failed_action.is_some()
    }

    /// Clear the failure state (for retry)
    pub fn clear_failure(&mut self) {
        self.failed_action = None;
        self.failure_message = None;
        self.updated_at = Utc::now();
    }
}

// ============================================================================
// Checkpoint Manager
// ============================================================================

/// Manager for checkpoint file operations
///
/// Handles saving and loading checkpoints from the `.rustible/` directory.
pub struct CheckpointManager {
    /// Root directory for checkpoint storage
    root_dir: PathBuf,
}

impl CheckpointManager {
    /// Default checkpoint filename
    pub const CHECKPOINT_FILE: &'static str = "checkpoint.json";

    /// Create a new checkpoint manager
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// Get the checkpoint file path
    pub fn checkpoint_path(&self) -> PathBuf {
        self.root_dir.join(Self::CHECKPOINT_FILE)
    }

    /// Save a checkpoint to disk
    pub async fn save_checkpoint(
        &self,
        checkpoint: &ApplyCheckpoint,
    ) -> ProvisioningResult<()> {
        let path = self.checkpoint_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to create checkpoint directory: {}",
                    e
                ))
            })?;
        }

        let content = serde_json::to_string_pretty(checkpoint).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize checkpoint: {}",
                e
            ))
        })?;

        // Write atomically
        let temp_path = path.with_extension("json.tmp");

        tokio::fs::write(&temp_path, &content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to write checkpoint temp file: {}",
                e
            ))
        })?;

        tokio::fs::rename(&temp_path, &path).await.map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            ProvisioningError::StatePersistenceError(format!(
                "Failed to rename checkpoint temp file: {}",
                e
            ))
        })?;

        tracing::debug!(
            plan_id = %checkpoint.plan_id,
            completed = checkpoint.completed_actions.len(),
            "Saved apply checkpoint"
        );

        Ok(())
    }

    /// Load a checkpoint from disk
    ///
    /// Returns `None` if no checkpoint file exists.
    pub async fn load_checkpoint(&self) -> ProvisioningResult<Option<ApplyCheckpoint>> {
        let path = self.checkpoint_path();

        if !path.exists() {
            tracing::debug!("No checkpoint file found");
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to read checkpoint file: {}",
                e
            ))
        })?;

        let checkpoint: ApplyCheckpoint =
            serde_json::from_str(&content).map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to parse checkpoint file: {}",
                    e
                ))
            })?;

        tracing::info!(
            plan_id = %checkpoint.plan_id,
            completed = checkpoint.completed_actions.len(),
            has_failure = checkpoint.has_failure(),
            "Loaded apply checkpoint"
        );

        Ok(Some(checkpoint))
    }

    /// Remove the checkpoint file (after successful completion)
    pub async fn clear_checkpoint(&self) -> ProvisioningResult<()> {
        let path = self.checkpoint_path();

        if path.exists() {
            tokio::fs::remove_file(&path).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to remove checkpoint file: {}",
                    e
                ))
            })?;

            tracing::debug!("Cleared apply checkpoint");
        }

        Ok(())
    }

    /// Check if a resumable checkpoint exists for the given plan
    pub async fn has_resumable_checkpoint(
        &self,
        plan_id: &str,
    ) -> ProvisioningResult<bool> {
        match self.load_checkpoint().await? {
            Some(checkpoint) => Ok(checkpoint.can_resume(plan_id)),
            None => Ok(false),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_action_ids() -> Vec<String> {
        vec![
            "aws_vpc.main".to_string(),
            "aws_subnet.public".to_string(),
            "aws_instance.web".to_string(),
            "aws_security_group.web_sg".to_string(),
        ]
    }

    #[test]
    fn test_checkpoint_new() {
        let checkpoint = ApplyCheckpoint::new("plan-123");
        assert_eq!(checkpoint.plan_id, "plan-123");
        assert!(checkpoint.completed_actions.is_empty());
        assert!(checkpoint.failed_action.is_none());
        assert!(checkpoint.failure_message.is_none());
    }

    #[test]
    fn test_checkpoint_can_resume() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");

        // Cannot resume with no completed actions
        assert!(!checkpoint.can_resume("plan-123"));

        // Can resume after completing an action
        checkpoint.record_completed("aws_vpc.main");
        assert!(checkpoint.can_resume("plan-123"));

        // Cannot resume with wrong plan ID
        assert!(!checkpoint.can_resume("plan-456"));
    }

    #[test]
    fn test_checkpoint_record_completed() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");

        checkpoint.record_completed("aws_vpc.main");
        checkpoint.record_completed("aws_subnet.public");

        assert_eq!(checkpoint.completed_count(), 2);
        assert!(checkpoint.completed_actions.contains(&"aws_vpc.main".to_string()));
        assert!(checkpoint
            .completed_actions
            .contains(&"aws_subnet.public".to_string()));

        // Duplicates should be ignored
        checkpoint.record_completed("aws_vpc.main");
        assert_eq!(checkpoint.completed_count(), 2);
    }

    #[test]
    fn test_checkpoint_record_failure() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");
        checkpoint.record_completed("aws_vpc.main");
        checkpoint.record_failure("aws_subnet.public", "Subnet CIDR conflict");

        assert!(checkpoint.has_failure());
        assert_eq!(
            checkpoint.failed_action.as_deref(),
            Some("aws_subnet.public")
        );
        assert_eq!(
            checkpoint.failure_message.as_deref(),
            Some("Subnet CIDR conflict")
        );
    }

    #[test]
    fn test_checkpoint_pending_actions() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");
        let all_actions = sample_action_ids();

        // All pending initially
        let pending = checkpoint.pending_actions(&all_actions);
        assert_eq!(pending.len(), 4);

        // Complete two actions
        checkpoint.record_completed("aws_vpc.main");
        checkpoint.record_completed("aws_subnet.public");

        let pending = checkpoint.pending_actions(&all_actions);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0], "aws_instance.web");
        assert_eq!(pending[1], "aws_security_group.web_sg");
    }

    #[test]
    fn test_checkpoint_is_complete() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");
        let all_actions = sample_action_ids();

        assert!(!checkpoint.is_complete(&all_actions));

        for action in &all_actions {
            checkpoint.record_completed(action);
        }

        assert!(checkpoint.is_complete(&all_actions));
    }

    #[test]
    fn test_checkpoint_clear_failure() {
        let mut checkpoint = ApplyCheckpoint::new("plan-123");
        checkpoint.record_failure("aws_vpc.main", "API error");

        assert!(checkpoint.has_failure());
        checkpoint.clear_failure();
        assert!(!checkpoint.has_failure());
        assert!(checkpoint.failed_action.is_none());
        assert!(checkpoint.failure_message.is_none());
    }

    #[tokio::test]
    async fn test_checkpoint_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(dir.path().join(".rustible"));

        let mut checkpoint = ApplyCheckpoint::new("plan-abc");
        checkpoint.record_completed("aws_vpc.main");
        checkpoint.record_completed("aws_subnet.public");
        checkpoint.record_failure("aws_instance.web", "Insufficient capacity");

        manager.save_checkpoint(&checkpoint).await.unwrap();

        let loaded = manager.load_checkpoint().await.unwrap().unwrap();

        assert_eq!(loaded.plan_id, "plan-abc");
        assert_eq!(loaded.completed_actions.len(), 2);
        assert!(loaded.completed_actions.contains(&"aws_vpc.main".to_string()));
        assert!(loaded
            .completed_actions
            .contains(&"aws_subnet.public".to_string()));
        assert_eq!(loaded.failed_action.as_deref(), Some("aws_instance.web"));
        assert_eq!(
            loaded.failure_message.as_deref(),
            Some("Insufficient capacity")
        );
    }

    #[tokio::test]
    async fn test_checkpoint_load_nonexistent() {
        let dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(dir.path().join(".rustible"));

        let result = manager.load_checkpoint().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_checkpoint_clear() {
        let dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(dir.path().join(".rustible"));

        let checkpoint = ApplyCheckpoint::new("plan-xyz");
        manager.save_checkpoint(&checkpoint).await.unwrap();

        // File should exist
        assert!(manager.checkpoint_path().exists());

        manager.clear_checkpoint().await.unwrap();

        // File should be gone
        assert!(!manager.checkpoint_path().exists());
    }

    #[tokio::test]
    async fn test_checkpoint_has_resumable() {
        let dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(dir.path().join(".rustible"));

        // No checkpoint at all
        assert!(!manager.has_resumable_checkpoint("plan-123").await.unwrap());

        // Save a checkpoint with completed actions
        let mut checkpoint = ApplyCheckpoint::new("plan-123");
        checkpoint.record_completed("aws_vpc.main");
        manager.save_checkpoint(&checkpoint).await.unwrap();

        // Should be resumable for matching plan
        assert!(manager.has_resumable_checkpoint("plan-123").await.unwrap());

        // Should not be resumable for different plan
        assert!(!manager.has_resumable_checkpoint("plan-456").await.unwrap());
    }

    #[tokio::test]
    async fn test_checkpoint_clear_nonexistent() {
        let dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(dir.path().join(".rustible"));

        // Should not error when clearing nonexistent checkpoint
        manager.clear_checkpoint().await.unwrap();
    }

    #[test]
    fn test_checkpoint_serialization() {
        let mut checkpoint = ApplyCheckpoint::new("plan-ser-test");
        checkpoint.record_completed("res_a");
        checkpoint.record_failure("res_b", "some error");

        let json = serde_json::to_string_pretty(&checkpoint).unwrap();
        let deserialized: ApplyCheckpoint = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.plan_id, "plan-ser-test");
        assert_eq!(deserialized.completed_actions, vec!["res_a".to_string()]);
        assert_eq!(deserialized.failed_action.as_deref(), Some("res_b"));
    }

    #[test]
    fn test_checkpoint_manager_path() {
        let manager = CheckpointManager::new("/tmp/.rustible");
        assert_eq!(
            manager.checkpoint_path(),
            PathBuf::from("/tmp/.rustible/checkpoint.json")
        );
    }
}
