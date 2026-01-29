//! Recovery mechanisms for distributed execution
//!
//! This module handles failure recovery including:
//! - Work unit checkpointing
//! - Leader failure recovery
//! - Idempotency tracking
//! - Network partition detection

use super::types::{ControllerId, HostId, WorkUnit, WorkUnitCheckpoint, WorkUnitId, WorkUnitState};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Checkpoint manager for work unit recovery
pub struct CheckpointManager {
    /// Stored checkpoints
    checkpoints: DashMap<WorkUnitId, WorkUnitCheckpoint>,
    /// Checkpoint interval
    checkpoint_interval: Duration,
    /// Last checkpoint time per work unit
    last_checkpoint: DashMap<WorkUnitId, Instant>,
    /// Maximum checkpoints to keep per work unit
    max_checkpoints: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    pub fn new(checkpoint_interval: Duration) -> Self {
        Self {
            checkpoints: DashMap::new(),
            checkpoint_interval,
            last_checkpoint: DashMap::new(),
            max_checkpoints: 5,
        }
    }

    /// Check if a checkpoint should be created
    pub fn should_checkpoint(&self, work_unit_id: &WorkUnitId) -> bool {
        self.last_checkpoint
            .get(work_unit_id)
            .map(|t| t.elapsed() >= self.checkpoint_interval)
            .unwrap_or(true)
    }

    /// Create a checkpoint
    pub fn create_checkpoint(&self, checkpoint: WorkUnitCheckpoint) {
        let id = checkpoint.work_unit_id.clone();
        self.checkpoints.insert(id.clone(), checkpoint);
        self.last_checkpoint.insert(id, Instant::now());
    }

    /// Get latest checkpoint for a work unit
    pub fn get_checkpoint(&self, work_unit_id: &WorkUnitId) -> Option<WorkUnitCheckpoint> {
        self.checkpoints.get(work_unit_id).map(|c| c.clone())
    }

    /// Remove checkpoint (after successful completion)
    pub fn remove_checkpoint(&self, work_unit_id: &WorkUnitId) {
        self.checkpoints.remove(work_unit_id);
        self.last_checkpoint.remove(work_unit_id);
    }

    /// Get all checkpoints for recovery
    pub fn all_checkpoints(&self) -> Vec<WorkUnitCheckpoint> {
        self.checkpoints.iter().map(|e| e.value().clone()).collect()
    }
}

/// Idempotency key for tracking executed operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey {
    /// Work unit ID
    pub work_unit_id: WorkUnitId,
    /// Host ID
    pub host_id: HostId,
    /// Task index within work unit
    pub task_index: usize,
    /// Loop iteration (if applicable)
    pub loop_index: Option<usize>,
}

impl IdempotencyKey {
    /// Create a new idempotency key
    pub fn new(work_unit_id: WorkUnitId, host_id: HostId, task_index: usize) -> Self {
        Self {
            work_unit_id,
            host_id,
            task_index,
            loop_index: None,
        }
    }

    /// Create with loop index
    pub fn with_loop_index(mut self, index: usize) -> Self {
        self.loop_index = Some(index);
        self
    }
}

/// Cached task result for idempotency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTaskResult {
    /// Whether task succeeded
    pub success: bool,
    /// Whether task made changes
    pub changed: bool,
    /// Result data
    pub result: serde_json::Value,
    /// Error message if failed
    pub error: Option<String>,
    /// When the result was cached
    pub cached_at_ms: u64,
}

impl CachedTaskResult {
    /// Create a new cached result
    pub fn new(success: bool, changed: bool, result: serde_json::Value) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            success,
            changed,
            result,
            error: None,
            cached_at_ms: cached_at,
        }
    }

    /// Create a failed result
    pub fn failed(error: String) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            success: false,
            changed: false,
            result: serde_json::Value::Null,
            error: Some(error),
            cached_at_ms: cached_at,
        }
    }
}

/// Idempotency tracker to prevent duplicate task execution
pub struct IdempotencyTracker {
    /// Executed operations and their results
    executed: DashMap<IdempotencyKey, CachedTaskResult>,
    /// TTL for cached results
    ttl: Duration,
}

impl IdempotencyTracker {
    /// Create a new idempotency tracker
    pub fn new(ttl: Duration) -> Self {
        Self {
            executed: DashMap::new(),
            ttl,
        }
    }

    /// Check if an operation was already executed
    pub fn check(&self, key: &IdempotencyKey) -> Option<CachedTaskResult> {
        self.executed.get(key).and_then(|entry| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            let age_ms = now.saturating_sub(entry.cached_at_ms);
            if age_ms < self.ttl.as_millis() as u64 {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    /// Record a successful execution
    pub fn record(&self, key: IdempotencyKey, result: CachedTaskResult) {
        self.executed.insert(key, result);
    }

    /// Clear expired entries
    pub fn cleanup(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let ttl_ms = self.ttl.as_millis() as u64;

        self.executed
            .retain(|_, v| now.saturating_sub(v.cached_at_ms) < ttl_ms);
    }

    /// Clear all entries for a work unit
    pub fn clear_work_unit(&self, work_unit_id: &WorkUnitId) {
        self.executed.retain(|k, _| &k.work_unit_id != work_unit_id);
    }
}

/// Network partition state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionState {
    /// Cluster is healthy, all nodes reachable
    Healthy,
    /// In minority partition, cannot make progress
    MinorityPartition,
    /// Completely isolated from other nodes
    Isolated,
}

/// Partition detector for network split detection
pub struct PartitionDetector {
    /// Known cluster members
    members: RwLock<Vec<ControllerId>>,
    /// Heartbeat failure counts per member
    heartbeat_failures: DashMap<ControllerId, u32>,
    /// Threshold for considering a node unreachable
    failure_threshold: u32,
    /// This node's ID
    local_id: ControllerId,
}

impl PartitionDetector {
    /// Create a new partition detector
    pub fn new(local_id: ControllerId, failure_threshold: u32) -> Self {
        Self {
            members: RwLock::new(Vec::new()),
            heartbeat_failures: DashMap::new(),
            failure_threshold,
            local_id,
        }
    }

    /// Set cluster members
    pub async fn set_members(&self, members: Vec<ControllerId>) {
        let mut m = self.members.write().await;
        *m = members;
    }

    /// Record heartbeat success for a member
    pub fn heartbeat_success(&self, member: &ControllerId) {
        self.heartbeat_failures.insert(member.clone(), 0);
    }

    /// Record heartbeat failure for a member
    pub fn heartbeat_failure(&self, member: &ControllerId) {
        self.heartbeat_failures
            .entry(member.clone())
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    /// Check current partition state
    pub async fn check_partition(&self) -> PartitionState {
        let members = self.members.read().await;

        if members.is_empty() {
            return PartitionState::Healthy; // Single node cluster
        }

        let reachable_count = members
            .iter()
            .filter(|id| {
                self.heartbeat_failures
                    .get(*id)
                    .map(|f| *f < self.failure_threshold)
                    .unwrap_or(true) // Unknown = reachable
            })
            .count();

        let total = members.len() + 1; // Include self
        let quorum = total / 2 + 1;

        if reachable_count + 1 >= quorum {
            // +1 for self
            PartitionState::Healthy
        } else if reachable_count > 0 {
            PartitionState::MinorityPartition
        } else {
            PartitionState::Isolated
        }
    }

    /// Get list of unreachable members
    pub async fn unreachable_members(&self) -> Vec<ControllerId> {
        let members = self.members.read().await;

        members
            .iter()
            .filter(|id| {
                self.heartbeat_failures
                    .get(*id)
                    .map(|f| *f >= self.failure_threshold)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}

/// Leader recovery handler
pub struct LeaderRecovery {
    /// Checkpoint manager reference
    checkpoint_manager: CheckpointManager,
    /// Idempotency tracker
    idempotency_tracker: IdempotencyTracker,
}

impl LeaderRecovery {
    /// Create a new leader recovery handler
    pub fn new(checkpoint_interval: Duration, idempotency_ttl: Duration) -> Self {
        Self {
            checkpoint_manager: CheckpointManager::new(checkpoint_interval),
            idempotency_tracker: IdempotencyTracker::new(idempotency_ttl),
        }
    }

    /// Get checkpoint manager
    pub fn checkpoint_manager(&self) -> &CheckpointManager {
        &self.checkpoint_manager
    }

    /// Get idempotency tracker
    pub fn idempotency_tracker(&self) -> &IdempotencyTracker {
        &self.idempotency_tracker
    }

    /// Identify orphaned work units from a failed controller
    pub fn identify_orphaned_work(
        &self,
        failed_controller: &ControllerId,
        assigned_work: &DashMap<WorkUnitId, ControllerId>,
    ) -> Vec<WorkUnitId> {
        assigned_work
            .iter()
            .filter(|e| e.value() == failed_controller)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Determine recovery action for a work unit
    pub fn determine_recovery_action(
        &self,
        work_unit_id: &WorkUnitId,
        state: Option<WorkUnitState>,
    ) -> RecoveryAction {
        match state {
            Some(WorkUnitState::Running) => {
                // Check for checkpoint
                if let Some(checkpoint) = self.checkpoint_manager.get_checkpoint(work_unit_id) {
                    RecoveryAction::ResumeFromCheckpoint(checkpoint)
                } else {
                    RecoveryAction::Restart
                }
            }
            Some(WorkUnitState::Pending) | Some(WorkUnitState::Assigned) => {
                RecoveryAction::Reassign
            }
            Some(WorkUnitState::Completed) => RecoveryAction::NoAction,
            Some(WorkUnitState::Failed { .. }) | Some(WorkUnitState::Cancelled) => {
                RecoveryAction::NoAction
            }
            None => RecoveryAction::Restart,
        }
    }

    /// Clear recovery data for completed work unit
    pub fn clear_recovery_data(&self, work_unit_id: &WorkUnitId) {
        self.checkpoint_manager.remove_checkpoint(work_unit_id);
        self.idempotency_tracker.clear_work_unit(work_unit_id);
    }
}

/// Action to take for work unit recovery
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// No action needed (completed or explicitly failed)
    NoAction,
    /// Reassign to another controller
    Reassign,
    /// Resume from checkpoint
    ResumeFromCheckpoint(WorkUnitCheckpoint),
    /// Restart from beginning
    Restart,
}

/// Work unit execution tracker for recovery
pub struct ExecutionTracker {
    /// Active executions
    executions: DashMap<WorkUnitId, ExecutionState>,
    /// Controller assignments
    assignments: DashMap<WorkUnitId, ControllerId>,
}

/// State of a work unit execution
#[derive(Debug, Clone)]
pub struct ExecutionState {
    /// Work unit being executed
    pub work_unit: WorkUnit,
    /// Current state
    pub state: WorkUnitState,
    /// Completed hosts
    pub completed_hosts: HashSet<HostId>,
    /// Failed hosts with errors
    pub failed_hosts: HashMap<HostId, String>,
    /// Current task index per host
    pub host_task_index: HashMap<HostId, usize>,
    /// Started at
    pub started_at: Instant,
    /// Last progress update
    pub last_update: Instant,
}

impl ExecutionState {
    /// Create new execution state
    pub fn new(work_unit: WorkUnit) -> Self {
        let now = Instant::now();
        Self {
            work_unit,
            state: WorkUnitState::Running,
            completed_hosts: HashSet::new(),
            failed_hosts: HashMap::new(),
            host_task_index: HashMap::new(),
            started_at: now,
            last_update: now,
        }
    }

    /// Create checkpoint from execution state
    pub fn checkpoint(&self) -> WorkUnitCheckpoint {
        let mut checkpoint = WorkUnitCheckpoint::new(self.work_unit.id.clone());
        checkpoint.completed_hosts = self.completed_hosts.iter().cloned().collect();
        checkpoint.failed_hosts = self.failed_hosts.clone();
        checkpoint.host_task_index = self.host_task_index.clone();
        checkpoint
    }

    /// Resume from checkpoint
    pub fn resume_from_checkpoint(work_unit: WorkUnit, checkpoint: &WorkUnitCheckpoint) -> Self {
        let now = Instant::now();
        Self {
            work_unit,
            state: WorkUnitState::Running,
            completed_hosts: checkpoint.completed_hosts.iter().cloned().collect(),
            failed_hosts: checkpoint.failed_hosts.clone(),
            host_task_index: checkpoint.host_task_index.clone(),
            started_at: now,
            last_update: now,
        }
    }

    /// Mark host as completed
    pub fn mark_host_completed(&mut self, host: HostId) {
        self.completed_hosts.insert(host);
        self.last_update = Instant::now();
    }

    /// Mark host as failed
    pub fn mark_host_failed(&mut self, host: HostId, error: String) {
        self.failed_hosts.insert(host, error);
        self.last_update = Instant::now();
    }

    /// Update task index for host
    pub fn update_task_index(&mut self, host: HostId, index: usize) {
        self.host_task_index.insert(host, index);
        self.last_update = Instant::now();
    }

    /// Check if execution is complete
    pub fn is_complete(&self) -> bool {
        let total_hosts = self.work_unit.hosts.len();
        let finished = self.completed_hosts.len() + self.failed_hosts.len();
        finished >= total_hosts
    }

    /// Calculate progress percentage
    pub fn progress(&self) -> f64 {
        if self.work_unit.hosts.is_empty() {
            return 100.0;
        }

        let total_hosts = self.work_unit.hosts.len() as f64;
        let total_tasks = self.work_unit.tasks.len() as f64;

        if total_tasks == 0.0 {
            let completed = self.completed_hosts.len() as f64;
            return (completed / total_hosts) * 100.0;
        }

        let mut progress = 0.0;
        for host in &self.work_unit.hosts {
            if self.completed_hosts.contains(host) {
                progress += 1.0;
            } else if let Some(&idx) = self.host_task_index.get(host) {
                progress += idx as f64 / total_tasks;
            }
        }

        (progress / total_hosts) * 100.0
    }
}

impl ExecutionTracker {
    /// Create a new execution tracker
    pub fn new() -> Self {
        Self {
            executions: DashMap::new(),
            assignments: DashMap::new(),
        }
    }

    /// Start tracking a work unit execution
    pub fn start_execution(&self, work_unit: WorkUnit, controller: ControllerId) {
        let id = work_unit.id.clone();
        self.executions
            .insert(id.clone(), ExecutionState::new(work_unit));
        self.assignments.insert(id, controller);
    }

    /// Get execution state
    pub fn get_execution(&self, work_unit_id: &WorkUnitId) -> Option<ExecutionState> {
        self.executions.get(work_unit_id).map(|e| e.clone())
    }

    /// Update execution state
    pub fn update_execution<F>(&self, work_unit_id: &WorkUnitId, f: F)
    where
        F: FnOnce(&mut ExecutionState),
    {
        if let Some(mut entry) = self.executions.get_mut(work_unit_id) {
            f(entry.value_mut());
        }
    }

    /// Complete execution
    pub fn complete_execution(&self, work_unit_id: &WorkUnitId) {
        if let Some(mut entry) = self.executions.get_mut(work_unit_id) {
            entry.state = WorkUnitState::Completed;
        }
        self.assignments.remove(work_unit_id);
    }

    /// Fail execution
    pub fn fail_execution(&self, work_unit_id: &WorkUnitId, error: String) {
        if let Some(mut entry) = self.executions.get_mut(work_unit_id) {
            entry.state = WorkUnitState::Failed { error };
        }
        self.assignments.remove(work_unit_id);
    }

    /// Get all executions for a controller
    pub fn executions_for_controller(&self, controller: &ControllerId) -> Vec<WorkUnitId> {
        self.assignments
            .iter()
            .filter(|e| e.value() == controller)
            .map(|e| e.key().clone())
            .collect()
    }
}

impl Default for ExecutionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::types::RunId;

    fn test_work_unit() -> WorkUnit {
        WorkUnit::new(
            RunId::generate(),
            0,
            vec![HostId::new("host-1"), HostId::new("host-2")],
        )
    }

    #[test]
    fn test_checkpoint_manager() {
        let manager = CheckpointManager::new(Duration::from_secs(5));

        let work_unit = test_work_unit();
        let checkpoint = WorkUnitCheckpoint::new(work_unit.id.clone());

        assert!(manager.should_checkpoint(&work_unit.id));

        manager.create_checkpoint(checkpoint.clone());

        assert!(!manager.should_checkpoint(&work_unit.id));

        let retrieved = manager.get_checkpoint(&work_unit.id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().work_unit_id, work_unit.id);
    }

    #[test]
    fn test_idempotency_tracker() {
        let tracker = IdempotencyTracker::new(Duration::from_secs(300));

        let key = IdempotencyKey::new(WorkUnitId::new("wu-1"), HostId::new("host-1"), 0);

        // First check - not executed
        assert!(tracker.check(&key).is_none());

        // Record execution
        let result = CachedTaskResult::new(true, true, serde_json::json!({"status": "ok"}));
        tracker.record(key.clone(), result);

        // Second check - already executed
        let cached = tracker.check(&key);
        assert!(cached.is_some());
        assert!(cached.unwrap().success);
    }

    #[tokio::test]
    async fn test_partition_detector() {
        let detector = PartitionDetector::new(ControllerId::new("local"), 3);

        detector
            .set_members(vec![
                ControllerId::new("node-1"),
                ControllerId::new("node-2"),
            ])
            .await;

        // Initial state - all healthy
        assert_eq!(detector.check_partition().await, PartitionState::Healthy);

        // Record failures for one node
        for _ in 0..3 {
            detector.heartbeat_failure(&ControllerId::new("node-1"));
        }

        // Still healthy (2 out of 3 including self)
        assert_eq!(detector.check_partition().await, PartitionState::Healthy);

        // Record failures for second node
        for _ in 0..3 {
            detector.heartbeat_failure(&ControllerId::new("node-2"));
        }

        // Now isolated
        assert_eq!(detector.check_partition().await, PartitionState::Isolated);
    }

    #[test]
    fn test_execution_state_progress() {
        let mut work_unit = test_work_unit();
        work_unit.tasks = vec![
            crate::distributed::types::TaskSpec {
                name: "task1".to_string(),
                module: "ping".to_string(),
                args: std::collections::HashMap::new(),
                when: None,
                register: None,
                ignore_errors: false,
            },
            crate::distributed::types::TaskSpec {
                name: "task2".to_string(),
                module: "ping".to_string(),
                args: std::collections::HashMap::new(),
                when: None,
                register: None,
                ignore_errors: false,
            },
        ];

        let mut state = ExecutionState::new(work_unit);

        // Initial progress should be 0
        assert_eq!(state.progress(), 0.0);

        // Complete first task on first host
        state.update_task_index(HostId::new("host-1"), 1);
        assert!(state.progress() > 0.0);
        assert!(state.progress() < 50.0);

        // Complete first host
        state.mark_host_completed(HostId::new("host-1"));
        assert_eq!(state.progress(), 50.0);

        // Complete second host
        state.mark_host_completed(HostId::new("host-2"));
        assert_eq!(state.progress(), 100.0);
        assert!(state.is_complete());
    }

    #[test]
    fn test_leader_recovery_actions() {
        let recovery = LeaderRecovery::new(Duration::from_secs(5), Duration::from_secs(300));

        // Pending work unit - should reassign
        let action = recovery
            .determine_recovery_action(&WorkUnitId::new("wu-1"), Some(WorkUnitState::Pending));
        assert!(matches!(action, RecoveryAction::Reassign));

        // Completed work unit - no action
        let action = recovery
            .determine_recovery_action(&WorkUnitId::new("wu-2"), Some(WorkUnitState::Completed));
        assert!(matches!(action, RecoveryAction::NoAction));

        // Running without checkpoint - restart
        let action = recovery
            .determine_recovery_action(&WorkUnitId::new("wu-3"), Some(WorkUnitState::Running));
        assert!(matches!(action, RecoveryAction::Restart));
    }
}
