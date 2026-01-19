//! State Management System for Rustible
//!
//! This module solves Ansible's "no state" pain point by providing comprehensive
//! state tracking, persistence, diff reporting, rollback capability, and dependency
//! tracking between tasks.
//!
//! ## Key Features
//!
//! - **Execution State Tracking**: Track the state of every task execution including
//!   success/failure, changes made, and execution metadata.
//! - **State Persistence**: Persist state to JSON files or SQLite databases for
//!   cross-execution state management.
//! - **State Diff Reporting**: Compare states between executions to understand what
//!   changed and when.
//! - **Rollback Capability**: Generate and execute rollback plans to revert changes.
//! - **Dependency Tracking**: Track dependencies between tasks for intelligent
//!   execution ordering and impact analysis.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         StateManager                                 │
//! │  (Central coordinator for all state operations)                      │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!          ┌─────────────────────────┼─────────────────────────┐
//!          ▼                         ▼                         ▼
//! ┌─────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
//! │   Persistence   │   │    Diff Engine      │   │  Dependency Graph   │
//! │  (JSON/SQLite)  │   │  (State comparison) │   │  (Task ordering)    │
//! └─────────────────┘   └─────────────────────┘   └─────────────────────┘
//!          │                         │                         │
//!          └─────────────────────────┼─────────────────────────┘
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Rollback Engine                                 │
//! │  (Generate and execute rollback plans from state history)            │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::state::{
//!     PersistenceBackend, StateConfig, StateManager, TaskStateRecord, TaskStatus,
//! };
//!
//! // Create a state manager with SQLite persistence
//! let config = StateConfig::builder()
//!     .persistence(PersistenceBackend::Sqlite("./state.db".into()))
//!     .enable_rollback(true)
//!     .build();
//!
//! let state_manager = StateManager::new(config)?;
//!
//! // Start an execution session
//! let session = state_manager.start_session("playbook.yml")?;
//!
//! // Record task execution
//! session.record_task(TaskStateRecord {
//!     task_id: "install_nginx".to_string(),
//!     host: "web1".to_string(),
//!     status: TaskStatus::Changed,
//!     before_state: Some(serde_json::json!({"installed": false})),
//!     after_state: Some(serde_json::json!({"installed": true})),
//!     ..Default::default()
//! })?;
//!
//! // Get diff from previous execution
//! let diff = state_manager.diff_from_previous(&session)?;
//! println!("Changes since last run: {:?}", diff);
//!
//! // Generate rollback plan
//! let rollback = state_manager.create_rollback_plan(&session)?;
//! # Ok(())
//! # }
//! ```

pub mod dependencies;
pub mod diff;
pub mod hashing;
pub mod persistence;
pub mod rollback;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub use dependencies::{DependencyGraph, DependencyNode, TaskDependency};
pub use diff::{DiffEngine, DiffReport, StateChange, StateDiff};
pub use hashing::{
    CachedTaskResult, HashCacheStats, HashingConfig, StateHashCache, TaskHashBuilder, TaskStateHash,
};
pub use persistence::{JsonPersistence, PersistenceBackend, SqlitePersistence, StatePersistence};
pub use rollback::{RollbackAction, RollbackExecutor, RollbackPlan, RollbackStatus};

/// Errors that can occur during state management operations
#[derive(Error, Debug)]
pub enum StateError {
    #[error("State persistence error: {0}")]
    Persistence(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("State not found for key: {0}")]
    StateNotFound(String),

    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Invalid state transition: {0}")]
    InvalidTransition(String),
}

/// Result type for state operations
pub type StateResult<T> = Result<T, StateError>;

/// Configuration for the state management system
#[derive(Debug, Clone)]
pub struct StateConfig {
    /// Persistence backend to use
    pub persistence: PersistenceBackend,
    /// Enable rollback capability (stores additional undo information)
    pub enable_rollback: bool,
    /// Enable dependency tracking between tasks
    pub enable_dependencies: bool,
    /// Maximum number of state snapshots to retain
    pub max_snapshots: usize,
    /// How long to retain state history
    pub retention_period: Duration,
    /// Enable state compression for storage efficiency
    pub enable_compression: bool,
    /// Enable state encryption for sensitive data
    pub enable_encryption: bool,
    /// Path to store state files (for file-based backends)
    pub state_dir: PathBuf,
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            persistence: PersistenceBackend::Json(PathBuf::from(".rustible_state")),
            enable_rollback: true,
            enable_dependencies: true,
            max_snapshots: 100,
            retention_period: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
            enable_compression: false,
            enable_encryption: false,
            state_dir: PathBuf::from(".rustible_state"),
        }
    }
}

impl StateConfig {
    /// Create a new builder for StateConfig
    pub fn builder() -> StateConfigBuilder {
        StateConfigBuilder::default()
    }

    /// Create a minimal configuration for testing
    pub fn minimal() -> Self {
        Self {
            persistence: PersistenceBackend::Memory,
            enable_rollback: false,
            enable_dependencies: false,
            max_snapshots: 10,
            retention_period: Duration::from_secs(3600),
            enable_compression: false,
            enable_encryption: false,
            state_dir: PathBuf::from("/tmp/rustible_state"),
        }
    }

    /// Create a production-ready configuration
    pub fn production(state_dir: PathBuf) -> Self {
        Self {
            persistence: PersistenceBackend::Sqlite(state_dir.join("state.db")),
            enable_rollback: true,
            enable_dependencies: true,
            max_snapshots: 1000,
            retention_period: Duration::from_secs(90 * 24 * 60 * 60), // 90 days
            enable_compression: true,
            enable_encryption: false,
            state_dir,
        }
    }
}

/// Builder for StateConfig
#[derive(Debug, Default)]
pub struct StateConfigBuilder {
    config: StateConfig,
}

impl StateConfigBuilder {
    /// Set the persistence backend
    pub fn persistence(mut self, backend: PersistenceBackend) -> Self {
        self.config.persistence = backend;
        self
    }

    /// Enable or disable rollback capability
    pub fn enable_rollback(mut self, enable: bool) -> Self {
        self.config.enable_rollback = enable;
        self
    }

    /// Enable or disable dependency tracking
    pub fn enable_dependencies(mut self, enable: bool) -> Self {
        self.config.enable_dependencies = enable;
        self
    }

    /// Set maximum number of snapshots to retain
    pub fn max_snapshots(mut self, max: usize) -> Self {
        self.config.max_snapshots = max;
        self
    }

    /// Set retention period for state history
    pub fn retention_period(mut self, period: Duration) -> Self {
        self.config.retention_period = period;
        self
    }

    /// Set the state directory
    pub fn state_dir(mut self, dir: PathBuf) -> Self {
        self.config.state_dir = dir;
        self
    }

    /// Build the configuration
    pub fn build(self) -> StateConfig {
        self.config
    }
}

/// Status of a task execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Task completed successfully with no changes
    Ok,
    /// Task completed successfully and made changes
    Changed,
    /// Task failed
    Failed,
    /// Task was skipped
    Skipped,
    /// Task is running
    Running,
    /// Task is pending
    Pending,
    /// Host was unreachable
    Unreachable,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Ok => write!(f, "ok"),
            TaskStatus::Changed => write!(f, "changed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Skipped => write!(f, "skipped"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Unreachable => write!(f, "unreachable"),
        }
    }
}

/// Record of a single task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateRecord {
    /// Unique identifier for this task execution
    pub id: String,
    /// Task identifier (name or unique ID from playbook)
    pub task_id: String,
    /// Task name (human-readable)
    pub task_name: String,
    /// Host this task was executed on
    pub host: String,
    /// Module used for this task
    pub module: String,
    /// Module arguments
    pub args: serde_json::Value,
    /// Execution status
    pub status: TaskStatus,
    /// State before task execution (for rollback)
    pub before_state: Option<serde_json::Value>,
    /// State after task execution
    pub after_state: Option<serde_json::Value>,
    /// When the task started
    pub started_at: DateTime<Utc>,
    /// When the task completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Duration of execution
    pub duration_ms: Option<u64>,
    /// Error message if failed
    pub error: Option<String>,
    /// Output from the task
    pub output: Option<serde_json::Value>,
    /// Whether this task can be rolled back
    pub rollback_available: bool,
    /// Rollback information (if available)
    pub rollback_info: Option<RollbackAction>,
    /// Tags associated with this task
    pub tags: Vec<String>,
    /// Play name this task belongs to
    pub play_name: Option<String>,
    /// Role name if task is from a role
    pub role_name: Option<String>,
    /// Check mode indicator
    pub check_mode: bool,
    /// Diff output if available
    pub diff: Option<serde_json::Value>,
}

impl Default for TaskStateRecord {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            task_id: String::new(),
            task_name: String::new(),
            host: String::new(),
            module: String::new(),
            args: serde_json::Value::Null,
            status: TaskStatus::Pending,
            before_state: None,
            after_state: None,
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            error: None,
            output: None,
            rollback_available: false,
            rollback_info: None,
            tags: Vec::new(),
            play_name: None,
            role_name: None,
            check_mode: false,
            diff: None,
        }
    }
}

impl TaskStateRecord {
    /// Create a new task state record
    pub fn new(
        task_id: impl Into<String>,
        host: impl Into<String>,
        module: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            host: host.into(),
            module: module.into(),
            ..Default::default()
        }
    }

    /// Set the task name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.task_name = name.into();
        self
    }

    /// Set the module arguments
    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = args;
        self
    }

    /// Mark the task as completed
    pub fn complete(&mut self, status: TaskStatus) {
        self.status = status;
        self.completed_at = Some(Utc::now());
        if let Some(started) = self.started_at.timestamp_millis().checked_sub(0) {
            let now = Utc::now().timestamp_millis();
            self.duration_ms = Some((now - started) as u64);
        }
    }

    /// Mark the task as failed with an error
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
    }
}

/// A point-in-time snapshot of execution state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Unique identifier for this snapshot
    pub id: String,
    /// Session ID this snapshot belongs to
    pub session_id: String,
    /// When this snapshot was created
    pub created_at: DateTime<Utc>,
    /// Description of the snapshot
    pub description: Option<String>,
    /// Playbook being executed
    pub playbook: String,
    /// All task records at this point
    pub tasks: Vec<TaskStateRecord>,
    /// Host-level state summaries
    pub host_states: HashMap<String, HostState>,
    /// Overall execution statistics
    pub stats: ExecutionStats,
    /// Metadata about the execution
    pub metadata: HashMap<String, serde_json::Value>,
    /// Parent snapshot ID (for incremental snapshots)
    pub parent_id: Option<String>,
}

impl StateSnapshot {
    /// Create a new snapshot
    pub fn new(session_id: impl Into<String>, playbook: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            created_at: Utc::now(),
            description: None,
            playbook: playbook.into(),
            tasks: Vec::new(),
            host_states: HashMap::new(),
            stats: ExecutionStats::default(),
            metadata: HashMap::new(),
            parent_id: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Calculate stats from task records
    pub fn calculate_stats(&mut self) {
        let mut stats = ExecutionStats::default();
        for task in &self.tasks {
            match task.status {
                TaskStatus::Ok => stats.ok += 1,
                TaskStatus::Changed => stats.changed += 1,
                TaskStatus::Failed => stats.failed += 1,
                TaskStatus::Skipped => stats.skipped += 1,
                TaskStatus::Unreachable => stats.unreachable += 1,
                _ => {}
            }
        }
        self.stats = stats;
    }
}

/// State of a specific host during execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostState {
    /// Host name
    pub host: String,
    /// Number of tasks that completed OK
    pub ok: usize,
    /// Number of tasks that made changes
    pub changed: usize,
    /// Number of tasks that failed
    pub failed: usize,
    /// Number of tasks that were skipped
    pub skipped: usize,
    /// Whether host is unreachable
    pub unreachable: bool,
    /// Facts gathered from host
    pub facts: Option<serde_json::Value>,
    /// Custom variables set for host
    pub vars: HashMap<String, serde_json::Value>,
    /// Last successful task
    pub last_successful_task: Option<String>,
    /// Last error encountered
    pub last_error: Option<String>,
}

/// Statistics for an execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionStats {
    /// Tasks that completed OK
    pub ok: usize,
    /// Tasks that made changes
    pub changed: usize,
    /// Tasks that failed
    pub failed: usize,
    /// Tasks that were skipped
    pub skipped: usize,
    /// Hosts that were unreachable
    pub unreachable: usize,
    /// Total tasks executed
    pub total: usize,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
}

impl ExecutionStats {
    /// Check if execution was successful (no failures or unreachable)
    pub fn is_successful(&self) -> bool {
        self.failed == 0 && self.unreachable == 0
    }

    /// Merge another stats into this one
    pub fn merge(&mut self, other: &ExecutionStats) {
        self.ok += other.ok;
        self.changed += other.changed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.unreachable += other.unreachable;
        self.total += other.total;
    }
}

/// An active execution session
pub struct ExecutionSession {
    /// Session ID
    pub id: String,
    /// Playbook being executed
    pub playbook: String,
    /// When session started
    pub started_at: DateTime<Utc>,
    /// Task records for this session
    tasks: Arc<DashMap<String, TaskStateRecord>>,
    /// Host states
    hosts: Arc<DashMap<String, HostState>>,
    /// Dependency graph for this session
    dependencies: Arc<RwLock<DependencyGraph>>,
    /// Sequence counter for task ordering
    sequence: Arc<AtomicU64>,
    /// Configuration
    config: Arc<StateConfig>,
    /// Parent state manager reference
    state_manager: Arc<StateManagerInner>,
}

impl ExecutionSession {
    /// Record a task execution
    pub fn record_task(&self, record: TaskStateRecord) -> StateResult<()> {
        let task_key = format!("{}::{}", record.host, record.task_id);

        // Update host state
        {
            let mut state = self
                .hosts
                .entry(record.host.clone())
                .or_insert_with(|| HostState {
                    host: record.host.clone(),
                    ..Default::default()
                });

            match record.status {
                TaskStatus::Ok => state.ok += 1,
                TaskStatus::Changed => state.changed += 1,
                TaskStatus::Failed => {
                    state.failed += 1;
                    state.last_error = record.error.clone();
                }
                TaskStatus::Skipped => state.skipped += 1,
                TaskStatus::Unreachable => state.unreachable = true,
                _ => {}
            }
            if record.status == TaskStatus::Ok || record.status == TaskStatus::Changed {
                state.last_successful_task = Some(record.task_id.clone());
            }
        }

        // Add to dependency graph if enabled
        if self.config.enable_dependencies {
            let mut deps = self.dependencies.write();
            deps.add_node(DependencyNode {
                id: record.task_id.clone(),
                name: record.task_name.clone(),
                host: record.host.clone(),
                module: record.module.clone(),
                sequence: self.sequence.fetch_add(1, Ordering::SeqCst),
            });
        }

        self.tasks.insert(task_key, record);
        Ok(())
    }

    /// Get a task record
    pub fn get_task(&self, host: &str, task_id: &str) -> Option<TaskStateRecord> {
        let key = format!("{}::{}", host, task_id);
        self.tasks.get(&key).map(|r| r.value().clone())
    }

    /// Get all tasks for a host
    pub fn get_host_tasks(&self, host: &str) -> Vec<TaskStateRecord> {
        self.tasks
            .iter()
            .filter(|r| r.value().host == host)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get host state
    pub fn get_host_state(&self, host: &str) -> Option<HostState> {
        self.hosts.get(host).map(|r| r.value().clone())
    }

    /// Create a snapshot of current state
    pub fn create_snapshot(&self, description: Option<String>) -> StateSnapshot {
        let mut snapshot = StateSnapshot::new(&self.id, &self.playbook);
        snapshot.description = description;

        // Collect all tasks
        snapshot.tasks = self.tasks.iter().map(|r| r.value().clone()).collect();

        // Collect host states
        snapshot.host_states = self
            .hosts
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        // Calculate stats
        snapshot.calculate_stats();

        snapshot
    }

    /// Get the dependency graph
    pub fn dependencies(&self) -> impl std::ops::Deref<Target = DependencyGraph> + '_ {
        self.dependencies.read()
    }

    /// Get execution statistics
    pub fn stats(&self) -> ExecutionStats {
        let mut stats = ExecutionStats::default();
        for task in self.tasks.iter() {
            match task.value().status {
                TaskStatus::Ok => stats.ok += 1,
                TaskStatus::Changed => stats.changed += 1,
                TaskStatus::Failed => stats.failed += 1,
                TaskStatus::Skipped => stats.skipped += 1,
                TaskStatus::Unreachable => stats.unreachable += 1,
                _ => {}
            }
            stats.total += 1;
        }
        if let Ok(duration) = Utc::now()
            .signed_duration_since(self.started_at)
            .num_milliseconds()
            .try_into()
        {
            stats.duration_ms = duration;
        }
        stats
    }

    /// Get the number of tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get all changed tasks (for rollback consideration)
    pub fn get_changed_tasks(&self) -> Vec<TaskStateRecord> {
        self.tasks
            .iter()
            .filter(|r| r.value().status == TaskStatus::Changed)
            .map(|r| r.value().clone())
            .collect()
    }
}

/// Internal state manager implementation
struct StateManagerInner {
    config: StateConfig,
    persistence: Box<dyn StatePersistence + Send + Sync>,
    sessions: DashMap<String, Arc<RwLock<ExecutionSession>>>,
    diff_engine: DiffEngine,
    rollback_executor: RollbackExecutor,
}

/// The main state manager that coordinates all state operations
pub struct StateManager {
    inner: Arc<StateManagerInner>,
}

impl StateManager {
    /// Create a new state manager with the given configuration
    pub fn new(config: StateConfig) -> StateResult<Self> {
        // Ensure state directory exists
        if !config.state_dir.exists() {
            std::fs::create_dir_all(&config.state_dir)?;
        }

        let persistence: Box<dyn StatePersistence + Send + Sync> = match &config.persistence {
            PersistenceBackend::Json(path) => Box::new(JsonPersistence::new(path.clone())?),
            PersistenceBackend::Sqlite(path) => Box::new(SqlitePersistence::new(path.clone())?),
            PersistenceBackend::Memory => Box::new(persistence::MemoryPersistence::new()),
        };

        let inner = StateManagerInner {
            config: config.clone(),
            persistence,
            sessions: DashMap::new(),
            diff_engine: DiffEngine::new(),
            rollback_executor: RollbackExecutor::new(config.clone()),
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Create a new state manager with default configuration
    pub fn default_manager() -> StateResult<Self> {
        Self::new(StateConfig::default())
    }

    /// Start a new execution session
    pub fn start_session(&self, playbook: impl Into<String>) -> StateResult<Arc<ExecutionSession>> {
        let playbook = playbook.into();
        let session_id = Uuid::new_v4().to_string();

        let session = ExecutionSession {
            id: session_id.clone(),
            playbook,
            started_at: Utc::now(),
            tasks: Arc::new(DashMap::new()),
            hosts: Arc::new(DashMap::new()),
            dependencies: Arc::new(RwLock::new(DependencyGraph::new())),
            sequence: Arc::new(AtomicU64::new(0)),
            config: Arc::new(self.inner.config.clone()),
            state_manager: self.inner.clone(),
        };

        let session = Arc::new(session);
        self.inner.sessions.insert(
            session_id,
            Arc::new(RwLock::new(
                // We need to return the session, so we clone the Arc
                // This is a workaround since ExecutionSession isn't Clone
                ExecutionSession {
                    id: session.id.clone(),
                    playbook: session.playbook.clone(),
                    started_at: session.started_at,
                    tasks: session.tasks.clone(),
                    hosts: session.hosts.clone(),
                    dependencies: session.dependencies.clone(),
                    sequence: session.sequence.clone(),
                    config: session.config.clone(),
                    state_manager: session.state_manager.clone(),
                },
            )),
        );

        Ok(session)
    }

    /// Get an existing session
    pub fn get_session(&self, session_id: &str) -> Option<Arc<RwLock<ExecutionSession>>> {
        self.inner
            .sessions
            .get(session_id)
            .map(|r| r.value().clone())
    }

    /// End a session and persist its state
    pub fn end_session(&self, session: &ExecutionSession) -> StateResult<StateSnapshot> {
        let snapshot = session.create_snapshot(Some("Session completed".to_string()));

        // Persist the snapshot
        self.inner.persistence.save_snapshot(&snapshot)?;

        // Remove from active sessions
        self.inner.sessions.remove(&session.id);

        Ok(snapshot)
    }

    /// Save a snapshot
    pub fn save_snapshot(&self, snapshot: &StateSnapshot) -> StateResult<()> {
        self.inner.persistence.save_snapshot(snapshot)
    }

    /// Load a snapshot by ID
    pub fn load_snapshot(&self, snapshot_id: &str) -> StateResult<StateSnapshot> {
        self.inner.persistence.load_snapshot(snapshot_id)
    }

    /// List all snapshots
    pub fn list_snapshots(&self) -> StateResult<Vec<StateSnapshot>> {
        self.inner.persistence.list_snapshots()
    }

    /// Get the most recent snapshot for a playbook
    pub fn get_latest_snapshot(&self, playbook: &str) -> StateResult<Option<StateSnapshot>> {
        self.inner.persistence.get_latest_snapshot(playbook)
    }

    /// Compare two snapshots and generate a diff report
    pub fn diff_snapshots(&self, old_id: &str, new_id: &str) -> StateResult<DiffReport> {
        let old = self.load_snapshot(old_id)?;
        let new = self.load_snapshot(new_id)?;
        Ok(self.inner.diff_engine.diff(&old, &new))
    }

    /// Compare current session state with the most recent snapshot
    pub fn diff_from_previous(&self, session: &ExecutionSession) -> StateResult<DiffReport> {
        let current = session.create_snapshot(None);

        if let Some(previous) = self.get_latest_snapshot(&session.playbook)? {
            Ok(self.inner.diff_engine.diff(&previous, &current))
        } else {
            // No previous snapshot, return empty diff
            Ok(DiffReport::new(None, current))
        }
    }

    /// Create a rollback plan from a session
    pub fn create_rollback_plan(&self, session: &ExecutionSession) -> StateResult<RollbackPlan> {
        if !self.inner.config.enable_rollback {
            return Err(StateError::RollbackFailed(
                "Rollback is not enabled in configuration".to_string(),
            ));
        }

        let changed_tasks = session.get_changed_tasks();
        self.inner.rollback_executor.create_plan(&changed_tasks)
    }

    /// Execute a rollback plan
    pub async fn execute_rollback(&self, plan: &RollbackPlan) -> StateResult<RollbackStatus> {
        self.inner.rollback_executor.execute(plan).await
    }

    /// Get the dependency graph for a session
    pub fn get_dependencies(&self, session: &ExecutionSession) -> DependencyGraph {
        session.dependencies.read().clone()
    }

    /// Cleanup old snapshots based on retention policy
    pub fn cleanup_old_snapshots(&self) -> StateResult<usize> {
        let cutoff = SystemTime::now() - self.inner.config.retention_period;
        self.inner.persistence.cleanup_before(cutoff)
    }

    /// Get state manager configuration
    pub fn config(&self) -> &StateConfig {
        &self.inner.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_config_builder() {
        let config = StateConfig::builder()
            .persistence(PersistenceBackend::Memory)
            .enable_rollback(true)
            .max_snapshots(50)
            .build();

        assert!(config.enable_rollback);
        assert_eq!(config.max_snapshots, 50);
    }

    #[test]
    fn test_task_state_record() {
        let mut record = TaskStateRecord::new("task1", "host1", "apt")
            .with_name("Install nginx")
            .with_args(serde_json::json!({"name": "nginx", "state": "present"}));

        assert_eq!(record.task_id, "task1");
        assert_eq!(record.host, "host1");
        assert_eq!(record.module, "apt");
        assert_eq!(record.status, TaskStatus::Pending);

        record.complete(TaskStatus::Changed);
        assert_eq!(record.status, TaskStatus::Changed);
        assert!(record.completed_at.is_some());
    }

    #[test]
    fn test_execution_stats() {
        let mut stats = ExecutionStats::default();
        stats.ok = 5;
        stats.changed = 3;
        stats.failed = 0;
        stats.unreachable = 0;

        assert!(stats.is_successful());

        stats.failed = 1;
        assert!(!stats.is_successful());
    }

    #[test]
    fn test_state_snapshot() {
        let mut snapshot =
            StateSnapshot::new("session1", "playbook.yml").with_description("Test snapshot");

        assert_eq!(snapshot.playbook, "playbook.yml");
        assert!(snapshot.description.is_some());

        // Add some tasks
        snapshot
            .tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));
        snapshot
            .tasks
            .push(TaskStateRecord::new("task2", "host1", "service"));
        snapshot.tasks[0].status = TaskStatus::Changed;
        snapshot.tasks[1].status = TaskStatus::Ok;

        snapshot.calculate_stats();
        assert_eq!(snapshot.stats.changed, 1);
        assert_eq!(snapshot.stats.ok, 1);
    }

    #[tokio::test]
    async fn test_state_manager_session() {
        let config = StateConfig::minimal();
        let manager = StateManager::new(config).unwrap();

        let session = manager.start_session("test.yml").unwrap();
        assert!(!session.id.is_empty());
        assert_eq!(session.playbook, "test.yml");

        // Record a task
        let record =
            TaskStateRecord::new("install_pkg", "localhost", "apt").with_name("Install package");
        session.record_task(record).unwrap();

        assert_eq!(session.task_count(), 1);

        let snapshot = manager.end_session(&session).unwrap();
        assert_eq!(snapshot.tasks.len(), 1);
    }
}
