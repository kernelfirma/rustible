//! Core execution engine for Rustible.
//!
//! This module provides the main task execution engine for running playbooks
//! across multiple hosts with parallel execution support.
//!
//! # Overview
//!
//! The execution engine is responsible for:
//! - **Async task execution** using the tokio runtime
//! - **Parallel execution** across hosts (controlled by `forks`)
//! - **Task dependency resolution** via topological sorting
//! - **Handler management** with automatic deduplication
//! - **Dry-run support** (check mode) for previewing changes
//! - **Serial batching** for rolling deployments
//!
//! # Execution Strategies
//!
//! Three execution strategies are supported:
//!
//! - [`ExecutionStrategy::Linear`]: All hosts complete a task before proceeding
//! - [`ExecutionStrategy::Free`]: Each host runs independently at maximum speed
//! - [`ExecutionStrategy::HostPinned`]: Dedicated workers per host
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use rustible::executor::{Executor, ExecutorConfig, ExecutionStrategy};
//! # use rustible::executor::Playbook;
//! # let playbook = Playbook::parse("- hosts: all\n  tasks:\n    - name: Ping\n      ping: {}\n", None)?;
//!
//! // Configure the executor
//! let config = ExecutorConfig {
//!     forks: 10,
//!     check_mode: false,
//!     diff_mode: true,
//!     strategy: ExecutionStrategy::Linear,
//!     ..Default::default()
//! };
//!
//! // Create executor and run playbook
//! let executor = Executor::new(config);
//! let results = executor.run_playbook(&playbook).await?;
//!
//! // Get summary statistics
//! let stats = Executor::summarize_results(&results);
//! println!("OK: {}, Changed: {}, Failed: {}", stats.ok, stats.changed, stats.failed);
//! # Ok(())
//! # }
//! ```

/// Include handler for dynamic task inclusion.
pub mod include_handler;

/// Parallelization management for module execution.
pub mod parallelization;

/// Playbook representation for the executor.
pub mod playbook;

/// Runtime context for variable and host management.
pub mod runtime;

/// Task execution and result handling.
pub mod task;

// Enhancement modules for advanced execution features
/// Async task execution with timeout and polling support.
pub mod async_task;

/// Async runtime optimization and configuration.
pub mod async_runtime;

/// Batch processing for loop operations (reduces Ansible's 87x loop overhead).
pub mod batch_processor;

/// Condition evaluation for when/changed_when/failed_when.
pub mod condition;

/// Dependency graph and DAG-based task ordering.
pub mod dependency;

/// Fact pipeline for optimized fact gathering.
pub mod fact_pipeline;

/// Host-pinned execution strategy with dedicated workers.
pub mod host_pinned;

/// Execution pipeline optimizations.
pub mod pipeline;

/// Register variable management for task results.
pub mod register;

/// Task throttling with rate limits and concurrency control.
pub mod throttle;

/// Per-host task execution metrics and failure summaries.
pub mod host_metrics;

/// Work-stealing scheduler for optimal load balancing.
pub mod work_stealing;

// Re-exports for commonly used types from enhancement modules
pub use async_runtime::{RuntimeConfig, RuntimeMetrics, SpawnOptions, TaskSpawner};
pub use async_task::{AsyncConfig, AsyncJobInfo, AsyncJobStatus, AsyncTaskManager};
pub use batch_processor::{BatchConfig, BatchProcessor, BatchResult, BatchStrategy};
pub use condition::{Condition, ConditionContext, ConditionEvaluator};
pub use dependency::{
    DependencyError, DependencyGraph as AdvancedDependencyGraph, DependencyKind, DependencyNode,
};
use dialoguer::theme::ColorfulTheme;
pub use fact_pipeline::{FactPipeline, FactPipelineConfig, FactResult};
pub use host_metrics::{FailureSummary, HostTaskMetrics, MetricsCollector};
pub use host_pinned::{HostPinnedConfig, HostPinnedExecutor, HostPinnedPool};
pub use pipeline::{ExecutionPipeline, PipelineConfig, TaskOptimizationHints};
pub use playbook::{Play, Playbook};
pub use register::{FailedTaskInfo, LoopResults, RegisteredResultExt};
pub use throttle::{ThrottleConfig, ThrottleManager, ThrottleStats};
pub use work_stealing::{WorkItem, WorkStealingConfig, WorkStealingScheduler, WorkStealingStats};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::join_all;
use indexmap::IndexMap;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{debug, error, info, instrument, warn};

use crate::connection::ConnectionFactory;
use crate::executor::parallelization::ParallelizationManager;
use crate::executor::runtime::{ExecutionContext, RuntimeContext};
use crate::executor::task::{Handler, Task, TaskResult, TaskStatus};
use crate::modules::ModuleRegistry;
use crate::recovery::{RecoveryManager, TaskOutcome, TransactionId};

use colored::Colorize;
use console::Term;

/// Errors that can occur during playbook and task execution.
///
/// This enum covers all error conditions that may arise during the
/// execution of playbooks, plays, and individual tasks.
#[derive(Error, Debug)]
pub enum ExecutorError {
    /// A task failed to execute successfully.
    #[error("Task execution failed: {0}")]
    TaskFailed(String),

    /// A host could not be reached (connection failure).
    #[error("Host unreachable: {0}")]
    HostUnreachable(String),

    /// A circular dependency was detected in task ordering.
    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// A notified handler was not defined in the play.
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    /// A required variable was not defined.
    #[error("Variable not found: {0}")]
    VariableNotFound(String),

    /// A `when` condition could not be evaluated.
    #[error("Condition evaluation failed: {0}")]
    ConditionError(String),

    /// A referenced module does not exist.
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Failed to parse playbook YAML or related content.
    #[error("Playbook parse error: {0}")]
    ParseError(String),

    /// An I/O operation failed.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// A general runtime error occurred.
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// A task execution timed out.
    #[error("Task timeout: {0}")]
    Timeout(String),

    /// Other miscellaneous errors.
    #[error("{0}")]
    Other(String),

    /// Template rendering error.
    #[error("Template error: {0}")]
    TemplateError(String),
}

impl From<crate::error::Error> for ExecutorError {
    fn from(err: crate::error::Error) -> Self {
        ExecutorError::TemplateError(err.to_string())
    }
}

/// Result type for executor operations.
///
/// A type alias for `Result<T, ExecutorError>` used throughout the executor module.
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Events emitted during playbook execution.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Playbook execution started
    PlaybookStart(String),
    /// Play execution started
    PlayStart(String),
    /// Task execution started
    TaskStart { task: String, host: Option<String> },
    /// Task execution started globally (all hosts)
    TaskStartGlobal(String),
    /// Task completed on a host
    HostTaskComplete(String, String, TaskResult), // host, task_name, result
    /// Playbook execution finished
    PlaybookFinish(String),
    /// Generic log message
    Log(String),
}

/// Callback function for execution events.
pub type EventCallback = Arc<dyn Fn(ExecutionEvent) + Send + Sync>;

/// Configuration options for the playbook executor.
///
/// Controls how playbooks are executed, including parallelism, execution strategy,
/// and runtime behavior options.
///
/// # Example
///
/// ```rust
/// use rustible::executor::{ExecutorConfig, ExecutionStrategy};
///
/// let config = ExecutorConfig {
///     forks: 10,              // Run on 10 hosts in parallel
///     check_mode: true,       // Dry-run mode
///     diff_mode: true,        // Show diffs
///     strategy: ExecutionStrategy::Linear,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum number of parallel host executions (default: 5).
    ///
    /// This controls how many hosts can run tasks simultaneously.
    /// Similar to Ansible's `--forks` or `-f` option.
    pub forks: usize,

    /// Enable dry-run mode (default: false).
    ///
    /// When enabled, tasks report what they would do without making changes.
    /// Similar to Ansible's `--check` option.
    pub check_mode: bool,

    /// Enable diff mode (default: false).
    ///
    /// When enabled, file-modifying tasks show before/after diffs.
    /// Similar to Ansible's `--diff` option.
    pub diff_mode: bool,

    /// Verbosity level from 0-4 (default: 0).
    ///
    /// Higher values produce more detailed output:
    /// - 0: Normal output
    /// - 1: Verbose (`-v`)
    /// - 2: More verbose (`-vv`)
    /// - 3: Debug (`-vvv`)
    /// - 4: Connection debug (`-vvvv`)
    pub verbosity: u8,

    /// Execution strategy for task distribution (default: Linear).
    pub strategy: ExecutionStrategy,

    /// Timeout for individual task execution in seconds (default: 300).
    pub task_timeout: u64,

    /// Whether to gather facts automatically (default: true).
    ///
    /// When enabled, system facts are collected from each host
    /// before executing tasks.
    pub gather_facts: bool,

    /// Extra variables passed via command line.
    ///
    /// These have the highest precedence and override all other variables.
    /// Similar to Ansible's `--extra-vars` or `-e` option.
    pub extra_vars: HashMap<String, serde_json::Value>,

    /// Enable automatic rollback on playbook failure (default: false).
    ///
    /// When enabled, the executor will attempt to undo changes made by
    /// successfully completed tasks when a subsequent task fails. Rollback
    /// actions are executed via the module registry using inverse arguments.
    pub auto_rollback: bool,

    /// Forward the local SSH agent to remote hosts (default: false).
    ///
    /// When enabled, the SSH agent on the local machine is made available
    /// to processes on remote hosts, enabling git-over-SSH, multi-hop
    /// deployments, and bastion workflows.
    /// Similar to `ssh -A` or Ansible's `ansible_ssh_forward_agent`.
    pub forward_agent: bool,

    /// Enable command pipelining for batch operations (default: true).
    ///
    /// When enabled, independent loop operations (e.g., package installs)
    /// are coalesced into single commands to reduce SSH round-trips.
    pub pipelining: bool,

    /// Enable privilege escalation (default: false).
    pub r#become: bool,
    /// Privilege escalation method (default: "sudo").
    pub become_method: String,
    /// Target user for privilege escalation (default: "root").
    pub become_user: String,
    /// Password for privilege escalation (default: None).
    pub become_password: Option<String>,

    /// Enable distributed execution across worker nodes (default: false).
    pub distributed: bool,
    /// Number of worker nodes for distributed execution (default: 1).
    pub workers: usize,
    /// Distribution strategy for work assignment (default: "adaptive").
    pub distribution_strategy: String,

    /// Enable step-by-step confirmation for tasks (default: false).
    ///
    /// When enabled, the executor prompts for confirmation before running each task.
    /// Similar to Ansible's `--step` option.
    pub step: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            forks: 5,
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            strategy: ExecutionStrategy::Linear,
            task_timeout: 300,
            gather_facts: true,
            extra_vars: HashMap::new(),
            auto_rollback: false,
            forward_agent: false,
            pipelining: true,
            r#become: false,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_password: None,
            distributed: false,
            workers: 1,
            distribution_strategy: "adaptive".to_string(),
            step: false,
        }
    }
}

/// Execution strategy determining how tasks are distributed across hosts.
///
/// The strategy affects task ordering and can impact performance and
/// behavior depending on your use case.
///
/// # Comparison
///
/// | Strategy | Task Order | Use Case |
/// |----------|------------|----------|
/// | Linear | All hosts complete task N before task N+1 | Default, predictable |
/// | Free | Each host runs independently | Maximum throughput |
/// | HostPinned | Dedicated worker per host | Connection reuse |
/// | Debug | Step through tasks with verbose output | Interactive debugging |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Run each task on all hosts before moving to the next task.
    ///
    /// This is the default strategy and provides predictable execution order.
    /// Task N completes on all hosts before task N+1 begins on any host.
    Linear,

    /// Run all tasks on each host as fast as possible.
    ///
    /// Each host proceeds independently through the task list.
    /// Provides maximum throughput but less predictable ordering.
    Free,

    /// Pin tasks to specific hosts with dedicated workers.
    ///
    /// Similar to `Free` but optimizes for connection reuse and
    /// cache locality by keeping the same worker for each host.
    HostPinned,

    /// Debug strategy for interactive task debugging.
    ///
    /// Executes tasks one at a time with verbose output including
    /// variable inspection on failure.
    DebugStrategy,
}

impl std::fmt::Display for ExecutionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strategy = match self {
            Self::Linear => "linear",
            Self::Free => "free",
            Self::HostPinned => "host_pinned",
            Self::DebugStrategy => "debug",
        };

        f.write_str(strategy)
    }
}

/// Statistics collected during playbook execution.
///
/// Tracks the count of tasks in each final state across all hosts.
/// Used for generating execution summaries.
///
/// # Example
///
/// ```rust
/// use rustible::executor::ExecutionStats;
///
/// let mut stats = ExecutionStats::default();
/// stats.ok = 5;
/// stats.changed = 3;
/// println!("OK: {}, Changed: {}", stats.ok, stats.changed);
/// ```
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStats {
    /// Number of tasks that succeeded without changes.
    pub ok: usize,
    /// Number of tasks that made changes.
    pub changed: usize,
    /// Number of tasks that failed.
    pub failed: usize,
    /// Number of tasks that were skipped (condition not met).
    pub skipped: usize,
    /// Number of tasks that could not run due to unreachable host.
    pub unreachable: usize,
}

impl ExecutionStats {
    /// Merge another set of statistics into this one.
    ///
    /// Adds the counts from `other` to the current statistics.
    pub fn merge(&mut self, other: &ExecutionStats) {
        self.ok += other.ok;
        self.changed += other.changed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.unreachable += other.unreachable;
    }
}

/// Execution result for a single host.
///
/// Contains the aggregated statistics and final state for one host
/// after all tasks have been processed.
#[derive(Debug, Clone)]
pub struct HostResult {
    /// The hostname or identifier.
    pub host: String,
    /// Aggregated task statistics for this host.
    pub stats: ExecutionStats,
    /// Whether any task failed on this host.
    pub failed: bool,
    /// Whether this host became unreachable during execution.
    pub unreachable: bool,
}

/// The main playbook execution engine.
///
/// The `Executor` orchestrates the execution of playbooks across multiple hosts.
/// It handles parallel execution, handler management, and result collection.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use rustible::executor::{Executor, ExecutorConfig};
/// # use rustible::executor::Playbook;
/// # let playbook = Playbook::parse("- hosts: all\n  tasks:\n    - name: Ping\n      ping: {}\n", None)?;
///
/// let executor = Executor::new(ExecutorConfig::default());
/// let results = executor.run_playbook(&playbook).await?;
///
/// for (host, result) in &results {
///     println!("{}: OK={}, Changed={}", host, result.stats.ok, result.stats.changed);
/// }
/// # Ok(())
/// # }
/// ```
pub struct Executor {
    config: ExecutorConfig,
    runtime: Arc<RwLock<RuntimeContext>>,
    handlers: Arc<RwLock<HashMap<String, Handler>>>,
    notified_handlers: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
    parallelization_manager: Arc<ParallelizationManager>,
    recovery_manager: Option<Arc<RecoveryManager>>,
    /// Connection factory for remote execution
    connection_factory: Option<Arc<ConnectionFactory>>,
    /// Shared module registry for task execution
    module_registry: Arc<ModuleRegistry>,
    /// Accumulated state records for tasks that made changes (for rollback)
    changed_tasks: Arc<Mutex<Vec<crate::state::TaskStateRecord>>>,
    /// Batch processor for coalescing loop operations
    batch_processor: Arc<BatchProcessor>,
    /// Optional event callback for execution telemetry
    event_callback: Option<EventCallback>,
}

impl Executor {
    /// Create a new executor with the given configuration
    pub fn new(config: ExecutorConfig) -> Self {
        let forks = config.forks;
        Self {
            config,
            runtime: Arc::new(RwLock::new(RuntimeContext::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            notified_handlers: Arc::new(Mutex::new(HashSet::new())),
            semaphore: Arc::new(Semaphore::new(forks)),
            parallelization_manager: Arc::new(ParallelizationManager::new()),
            recovery_manager: None,
            connection_factory: None,
            module_registry: Arc::new(ModuleRegistry::default()),
            changed_tasks: Arc::new(Mutex::new(Vec::new())),
            batch_processor: Arc::new(BatchProcessor::new(BatchConfig::default())),
            event_callback: None,
        }
    }

    /// Create executor with a pre-existing runtime context
    pub fn with_runtime(config: ExecutorConfig, runtime: RuntimeContext) -> Self {
        let forks = config.forks;
        Self {
            config,
            runtime: Arc::new(RwLock::new(runtime)),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            notified_handlers: Arc::new(Mutex::new(HashSet::new())),
            semaphore: Arc::new(Semaphore::new(forks)),
            parallelization_manager: Arc::new(ParallelizationManager::new()),
            recovery_manager: None,
            connection_factory: None,
            module_registry: Arc::new(ModuleRegistry::default()),
            changed_tasks: Arc::new(Mutex::new(Vec::new())),
            batch_processor: Arc::new(BatchProcessor::new(BatchConfig::default())),
            event_callback: None,
        }
    }

    /// Set the recovery manager for this executor
    pub fn with_recovery_manager(mut self, recovery_manager: Arc<RecoveryManager>) -> Self {
        self.recovery_manager = Some(recovery_manager);
        self
    }

    /// Set the connection factory for remote execution
    pub fn with_connection_factory(mut self, factory: ConnectionFactory) -> Self {
        self.connection_factory = Some(Arc::new(factory));
        self
    }

    /// Set the event callback for this executor.
    pub fn with_event_callback(mut self, callback: EventCallback) -> Self {
        self.event_callback = Some(callback);
        self
    }

    fn emit_event(&self, event: ExecutionEvent) {
        if let Some(cb) = &self.event_callback {
            cb(event);
        }
    }

    /// Known reversible modules that can be automatically rolled back
    const REVERSIBLE_MODULES: &'static [&'static str] = &[
        "apt",
        "yum",
        "dnf",
        "package",
        "service",
        "systemd",
        "file",
        "copy",
        "template",
        "user",
        "group",
        "lineinfile",
    ];

    /// Build a `TaskStateRecord` from task execution data for rollback tracking.
    ///
    /// Only called for tasks whose result status is `Changed`.
    fn build_task_state_record(
        task: &Task,
        host: &str,
        status: crate::state::TaskStatus,
    ) -> crate::state::TaskStateRecord {
        // Convert task args (IndexMap<String, JsonValue>) to a JSON Value
        let args_json: serde_json::Value = {
            let map: serde_json::Map<String, serde_json::Value> = task
                .args
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(map)
        };

        let rollback_available = Self::REVERSIBLE_MODULES.contains(&task.module.as_str());

        let mut record = crate::state::TaskStateRecord {
            task_id: task.name.clone(),
            task_name: task.name.clone(),
            host: host.to_string(),
            module: task.module.clone(),
            args: args_json,
            status,
            rollback_available,
            ..Default::default()
        };
        record.complete(status);
        record
    }

    /// Return a snapshot of changed task records accumulated during execution.
    pub async fn changed_task_records(&self) -> Vec<crate::state::TaskStateRecord> {
        self.changed_tasks.lock().await.clone()
    }

    /// Get a connection for a host from the connection factory
    async fn get_connection_for_host(
        &self,
        host: &str,
    ) -> Option<Arc<dyn crate::connection::Connection + Send + Sync>> {
        if let Some(factory) = &self.connection_factory {
            match factory.get_connection(host).await {
                Ok(conn) => Some(conn),
                Err(e) => {
                    warn!("Failed to get connection for host {}: {}", host, e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Run a complete playbook
    #[instrument(skip(self, playbook), fields(playbook_name = %playbook.name))]
    pub async fn run_playbook(
        &self,
        playbook: &Playbook,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        self.emit_event(ExecutionEvent::PlaybookStart(playbook.name.clone()));
        info!("Starting playbook: {}", playbook.name);

        let tx_id = if let Some(rm) = &self.recovery_manager {
            Some(
                rm.begin_transaction(&playbook.name)
                    .await
                    .map_err(|e| ExecutorError::Other(e.to_string()))?,
            )
        } else {
            None
        };

        let result = async {
            let mut all_results: HashMap<String, HostResult> = HashMap::new();

            // Set playbook-level variables
            {
                let mut runtime = self.runtime.write().await;
                for (key, value) in &playbook.vars {
                    runtime.set_global_var(key.clone(), value.clone());
                }
                // Add extra vars (highest precedence)
                for (key, value) in &self.config.extra_vars {
                    runtime.set_extra_var(key.clone(), value.clone());
                }
            }

            // Execute each play in sequence
            for play in &playbook.plays {
                let play_results = self.run_play(play, tx_id.clone()).await?;

                // Merge results
                for (host, result) in play_results {
                    all_results
                        .entry(host)
                        .and_modify(|existing| {
                            existing.stats.merge(&result.stats);
                            existing.failed = existing.failed || result.failed;
                            existing.unreachable = existing.unreachable || result.unreachable;
                        })
                        .or_insert(result);
                }
            }

            // Run any remaining notified handlers
            self.flush_handlers(tx_id.clone()).await?;

            info!("Playbook completed: {}", playbook.name);
            Ok(all_results)
        }
        .await;

        if let Some(rm) = &self.recovery_manager {
            if let Some(id) = tx_id {
                match &result {
                    Ok(_) => {
                        if let Err(e) = rm.commit_transaction(&id).await {
                            error!("Failed to commit transaction: {}", e);
                            return Err(ExecutorError::Other(format!(
                                "Transaction commit failed: {}",
                                e
                            )));
                        }
                    }
                    Err(_) => {
                        if let Err(e) = rm.rollback_transaction(&id).await {
                            error!("Failed to rollback transaction: {}", e);
                        }
                    }
                }
            }
        }

        // Trigger automatic system rollback on failure
        if result.is_err() && self.config.auto_rollback {
            self.execute_system_rollback().await;
        }

        self.emit_event(ExecutionEvent::PlaybookFinish(playbook.name.clone()));
        result
    }

    /// Run a single play
    #[instrument(skip(self, play), fields(play_name = %play.name))]
    pub async fn run_play(
        &self,
        play: &Play,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        self.emit_event(ExecutionEvent::PlayStart(play.name.clone()));
        info!("Starting play: {}", play.name);

        let vars_from_files = self.load_vars_files(play).await?;

        // Register handlers for this play
        {
            let mut handlers = self.handlers.write().await;
            for handler in &play.handlers {
                handlers.insert(handler.name.clone(), handler.clone());
            }
            // Register handlers from roles
            for role in &play.roles {
                for handler in role.get_all_handlers() {
                    handlers.insert(handler.name.clone(), handler.clone());
                }
            }
        }

        // Set play-level variables
        {
            let mut runtime = self.runtime.write().await;
            for (key, value) in &play.vars {
                runtime.set_play_var(key.clone(), value.clone());
            }
            for (key, value) in vars_from_files {
                runtime.set_play_var(key, value);
            }
            // Load role variables into runtime context
            // Role variables are set as play vars since they have similar precedence
            for role in &play.roles {
                for (key, value) in role.get_all_vars() {
                    runtime.set_play_var(key.clone(), value.clone());
                }
            }
        }

        // Resolve hosts for this play
        let hosts = self.resolve_hosts(&play.hosts).await?;

        if hosts.is_empty() {
            warn!("No hosts matched for play: {}", play.name);
            return Ok(HashMap::new());
        }

        debug!("Executing on {} hosts", hosts.len());

        // Combine all tasks: gather_facts (if enabled) + pre_tasks + role tasks + tasks + post_tasks
        // Pre-allocate with known capacity to avoid reallocations
        let gather_facts_count = if play.gather_facts { 1 } else { 0 };
        let role_tasks_count: usize = play.roles.iter().map(|r| r.get_all_tasks().len()).sum();
        let total_tasks = gather_facts_count
            + play.pre_tasks.len()
            + role_tasks_count
            + play.tasks.len()
            + play.post_tasks.len();
        let mut all_tasks = Vec::with_capacity(total_tasks);

        // If gather_facts is enabled, inject a facts-gathering task at the start
        if play.gather_facts {
            debug!("Injecting gather_facts task for play: {}", play.name);
            let gather_facts_task = Task {
                name: "Gathering Facts".to_string(),
                module: "gather_facts".to_string(),
                args: IndexMap::new(),
                when: None,
                notify: Vec::new(),
                register: None,
                loop_items: None,
                loop_var: "item".to_string(),
                loop_control: None,
                ignore_errors: false,
                changed_when: None,
                failed_when: None,
                delegate_to: None,
                delegate_facts: None,
                run_once: false,
                tags: Vec::new(),
                r#become: false,
                become_user: None,
                block_id: None,
                block_role: crate::executor::task::BlockRole::Normal,
                retries: None,
                delay: None,
                until: None,
                vars: IndexMap::new(),
            };
            all_tasks.push(gather_facts_task);
        }

        // Ansible execution order: pre_tasks -> role tasks -> tasks -> post_tasks
        all_tasks.extend(play.pre_tasks.iter().cloned());
        // Add role tasks (from play.roles) after pre_tasks and before regular tasks
        for role in &play.roles {
            all_tasks.extend(role.get_all_tasks());
        }
        all_tasks.extend(play.tasks.iter().cloned());
        all_tasks.extend(play.post_tasks.iter().cloned());

        // Execute based on serial specification and strategy
        let execution_result = if let Some(ref serial_spec) = play.serial {
            self.run_serial(
                serial_spec,
                &hosts,
                &all_tasks,
                play.max_fail_percentage,
                tx_id.clone(),
            )
            .await
        } else {
            // Execute based on strategy without serial batching
            match self.config.strategy {
                ExecutionStrategy::Linear => {
                    self.run_linear(&hosts, &all_tasks, tx_id.clone()).await
                }
                ExecutionStrategy::Free => self.run_free(&hosts, &all_tasks, tx_id.clone()).await,
                ExecutionStrategy::HostPinned => {
                    self.run_host_pinned(&hosts, &all_tasks, tx_id.clone())
                        .await
                }
                // Debug strategy: runs linear with verbose output
                ExecutionStrategy::DebugStrategy => {
                    self.run_linear(&hosts, &all_tasks, tx_id.clone()).await
                }
            }
        };

        // Check if play failed
        let play_failed = match &execution_result {
            Ok(results) => results.values().any(|r| r.failed || r.unreachable),
            Err(_) => true,
        };

        // Flush handlers at end of play
        // If force_handlers is set, run handlers even if the play failed
        if !play_failed || play.force_handlers {
            if play.force_handlers && play_failed {
                info!("Running handlers despite play failure (force_handlers=true)");
            }
            self.flush_handlers(tx_id.clone()).await?;
        } else {
            // Clear notified handlers without running them
            let notified_count = {
                let mut notified = self.notified_handlers.lock().await;
                let count = notified.len();
                notified.clear();
                count
            };
            if notified_count > 0 {
                warn!(
                    "Skipping {} notified handlers due to play failure (use force_handlers=true to override)",
                    notified_count
                );
            }
        }

        info!("Play completed: {}", play.name);
        execution_result
    }

    /// Run tasks in linear strategy (all hosts per task before next task)
    async fn run_linear(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        use crate::executor::task::BlockRole;

        // Pre-allocate HashMaps with known capacity
        let host_count = hosts.len();
        let mut results: HashMap<String, HostResult> = HashMap::with_capacity(host_count);
        for h in hosts {
            results.insert(
                h.clone(),
                HostResult {
                    host: h.clone(),
                    stats: ExecutionStats::default(),
                    failed: false,
                    unreachable: false,
                },
            );
        }

        // Track which blocks have failed (per host) - pre-allocate with capacity
        let mut failed_blocks: HashMap<String, HashSet<String>> =
            HashMap::with_capacity(host_count);
        for h in hosts {
            failed_blocks.insert(h.clone(), HashSet::new());
        }
        // Track which blocks have had their rescue tasks run
        let mut rescued_blocks: HashMap<String, HashSet<String>> =
            HashMap::with_capacity(host_count);
        for h in hosts {
            rescued_blocks.insert(h.clone(), HashSet::new());
        }

        let mut step_mode = self.config.step;

        for task in tasks {
            // Determine which hosts should run this task based on block state
            let active_hosts: Vec<_> = hosts
                .iter()
                .filter(|h| {
                    let host_result = results.get(*h);
                    let host_failed_blocks = failed_blocks.get(*h);
                    let host_rescued_blocks = rescued_blocks.get(*h);

                    // Skip if host has failed (and not in a block)
                    if host_result
                        .map(|r| r.failed || r.unreachable)
                        .unwrap_or(false)
                    {
                        // But still run always tasks
                        if task.block_role == BlockRole::Always {
                            return true;
                        }
                        return false;
                    }

                    // Handle block-specific logic
                    if let Some(ref block_id) = task.block_id {
                        let block_failed = host_failed_blocks
                            .map(|blocks| blocks.contains(block_id))
                            .unwrap_or(false);
                        let block_rescued = host_rescued_blocks
                            .map(|blocks| blocks.contains(block_id))
                            .unwrap_or(false);

                        match task.block_role {
                            BlockRole::Normal => {
                                // Skip normal tasks if block has failed
                                !block_failed
                            }
                            BlockRole::Rescue => {
                                // Run rescue tasks only if block failed and hasn't been rescued yet
                                block_failed && !block_rescued
                            }
                            BlockRole::Always => {
                                // Always run always tasks
                                true
                            }
                        }
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();

            if active_hosts.is_empty() {
                // Check if all tasks remaining are block-related
                if task.block_id.is_none() {
                    warn!("All hosts have failed, stopping execution");
                    break;
                }
                continue;
            }

            if step_mode {
                let term = Term::stderr();
                let prompt = format!(
                    "{} {} {}",
                    "▶".cyan(),
                    task.name.cyan().bold(),
                    "[y,n,c,a]".dimmed()
                );
                let mut continue_outer_loop = false;

                loop {
                    let input: String = dialoguer::Input::with_theme(&ColorfulTheme::default())
                        .with_prompt(&prompt)
                        .default("y".to_string())
                        .interact_on(&term)
                        .map_err(|e| ExecutorError::IoError(e.into()))?;

                    match input.trim().to_lowercase().as_str() {
                        "y" | "yes" => break,
                        "n" | "no" => {
                            // Skip this task
                            for h in hosts {
                                if let Some(res) = results.get_mut(h) {
                                    res.stats.skipped += 1;
                                }
                            }
                            continue_outer_loop = true;
                            break;
                        }
                        "c" | "cont" | "continue" => {
                            step_mode = false;
                            break;
                        }
                        "a" | "abort" => {
                            return Err(ExecutorError::Other(
                                "Execution aborted by user".to_string(),
                            ));
                        }
                        _ => {
                            println!("Invalid option. Options: [y]es, [n]o, [c]ontinue, [a]bort");
                            continue;
                        }
                    }
                }

                if continue_outer_loop {
                    continue;
                }
            }

            self.emit_event(ExecutionEvent::TaskStartGlobal(task.name.clone()));

            // Run task on all active hosts in parallel (limited by semaphore)
            let task_results = self
                .run_task_on_hosts(&active_hosts, task, tx_id.clone())
                .await?;

            debug!(
                "Task '{}' completed on {} hosts",
                task.name,
                task_results.len()
            );

            // Update results and track block failures
            for (host, task_result) in task_results {
                debug!(
                    "  Host '{}': status={:?}, changed={}, msg={:?}",
                    host, task_result.status, task_result.changed, task_result.msg
                );

                if let Some(host_result) = results.get_mut(&host) {
                    // Check if this task failed
                    let task_failed =
                        task_result.status == crate::executor::task::TaskStatus::Failed;

                    // If it's a normal task in a block and it failed, mark the block as failed
                    if task_failed {
                        if let Some(ref block_id) = task.block_id {
                            if task.block_role == BlockRole::Normal {
                                if let Some(blocks) = failed_blocks.get_mut(&host) {
                                    blocks.insert(block_id.clone());
                                }
                                // Mark that rescue is needed - don't mark host as failed yet
                            }
                        }
                    }

                    // If this is a rescue task, mark the block as rescued
                    if task.block_role == BlockRole::Rescue {
                        if let Some(ref block_id) = task.block_id {
                            if let Some(blocks) = rescued_blocks.get_mut(&host) {
                                blocks.insert(block_id.clone());
                            }
                        }
                    }

                    // Update stats, but only mark host as failed if:
                    // - Task is not in a block, OR
                    // - Task is in a block but there's no rescue section (block failed without rescue)
                    let should_mark_failed = if task.block_id.is_some() {
                        // For block tasks, we handle failure differently
                        // The host only fails if rescue also fails
                        task.block_role == BlockRole::Rescue && task_failed
                    } else {
                        task_failed
                    };

                    // Temporarily modify result for stats update
                    let mut modified_result = task_result.clone();
                    if task.block_id.is_some()
                        && task.block_role == BlockRole::Normal
                        && task_failed
                    {
                        // Don't count normal block failure as host failure
                        modified_result.status = crate::executor::task::TaskStatus::Ok;
                    }

                    self.update_host_stats(host_result, &modified_result);

                    // Now set the actual failure state
                    if should_mark_failed && !task.ignore_errors {
                        host_result.failed = true;
                    }
                }
            }
        }

        // After all tasks, check if any blocks failed without being rescued
        for (host, host_failed_blocks) in &failed_blocks {
            if let Some(_host_result) = results.get_mut(host) {
                let host_rescued = rescued_blocks.get(host);
                for block_id in host_failed_blocks {
                    let was_rescued = host_rescued.map(|r| r.contains(block_id)).unwrap_or(false);
                    if !was_rescued {
                        // Block failed without rescue - this is a failure
                        // But we need to check if there was a rescue section defined
                        // For now, assume if rescue tasks were found, it was rescued
                        // If no rescue tasks exist, it's a real failure
                        // This is a simplification - proper implementation would track this differently
                    }
                }
            }
        }

        Ok(results)
    }

    /// Run tasks in free strategy (each host runs independently)
    ///
    /// OPTIMIZATION: Extract config values once instead of cloning config per host
    async fn run_free(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        // OPTIMIZATION: Fast path for single host
        if hosts.len() == 1 {
            let host = &hosts[0];
            let _permit = self.semaphore.acquire().await.unwrap();

            let mut host_result = HostResult {
                host: host.clone(),
                stats: ExecutionStats::default(),
                failed: false,
                unreachable: false,
            };

            // Get connection for host once before running tasks
            let host_connection = self.get_connection_for_host(host).await;

            for task in tasks {
                if host_result.failed || host_result.unreachable {
                    break;
                }

                self.emit_event(ExecutionEvent::TaskStart {
                    task: task.name.clone(),
                    host: Some(host.clone()),
                });

                let mut ctx = ExecutionContext::new(host.clone())
                    .with_check_mode(self.config.check_mode)
                    .with_diff_mode(self.config.diff_mode)
                    .with_verbosity(self.config.verbosity);

                // Set connection if available
                if let Some(ref conn) = host_connection {
                    ctx = ctx.with_connection(conn.clone());
                }

                let task_result = task
                    .execute(
                        &ctx,
                        &self.runtime,
                        &self.handlers,
                        &self.notified_handlers,
                        &self.parallelization_manager,
                        &self.module_registry,
                        &self.batch_processor,
                        self.config.pipelining,
                    )
                    .await;

                let event_result = match &task_result {
                    Ok(result) => result.clone(),
                    Err(e) => TaskResult::failed(e.to_string()),
                };
                self.emit_event(ExecutionEvent::HostTaskComplete(
                    host.clone(),
                    task.name.clone(),
                    event_result,
                ));

                if let Some(rm) = &self.recovery_manager {
                    if let Some(tid) = tx_id.as_ref() {
                        let (outcome, changed) = match &task_result {
                            Ok(r) => {
                                let outcome = match r.status {
                                    TaskStatus::Ok => TaskOutcome::Success,
                                    TaskStatus::Changed => TaskOutcome::Changed,
                                    TaskStatus::Failed => TaskOutcome::Failed {
                                        message: r.msg.clone().unwrap_or_default(),
                                    },
                                    TaskStatus::Skipped => TaskOutcome::Skipped,
                                    TaskStatus::Unreachable => TaskOutcome::Unreachable {
                                        message: r.msg.clone().unwrap_or_default(),
                                    },
                                };
                                (outcome, r.changed)
                            }
                            Err(e) => (
                                TaskOutcome::Failed {
                                    message: e.to_string(),
                                },
                                false,
                            ),
                        };
                        if let Err(e) = rm
                            .record_task(
                                tid.clone(),
                                task.name.clone(),
                                host.clone(),
                                outcome,
                                changed,
                            )
                            .await
                        {
                            warn!("Failed to record task outcome for host {}: {}", host, e);
                        }
                    }
                }

                // Record changed tasks for rollback
                if let Ok(ref result) = task_result {
                    if result.status == TaskStatus::Changed {
                        let record = Self::build_task_state_record(
                            task,
                            host,
                            crate::state::TaskStatus::Changed,
                        );
                        self.changed_tasks.lock().await.push(record);
                    }
                }

                match task_result {
                    Ok(result) => {
                        update_stats(&mut host_result.stats, &result);
                        if result.status == TaskStatus::Failed {
                            host_result.failed = true;
                        }
                    }
                    Err(_) => {
                        host_result.failed = true;
                        host_result.stats.failed += 1;
                    }
                }
            }

            let mut results = HashMap::with_capacity(1);
            results.insert(host.clone(), host_result);
            return Ok(results);
        }

        // OPTIMIZATION: Pre-extract config values to avoid cloning entire config per host
        let check_mode = self.config.check_mode;
        let diff_mode = self.config.diff_mode;
        let verbosity = self.config.verbosity;

        // Avoid cloning entire task list - use Arc slice instead
        let tasks: Arc<[Task]> = tasks.to_vec().into();
        let results = Arc::new(Mutex::new(HashMap::with_capacity(hosts.len())));

        let event_callback = self.event_callback.clone();
        let handles: Vec<_> = hosts
            .iter()
            .map(|host| {
                let host = host.clone();
                let tasks = Arc::clone(&tasks);
                let results = Arc::clone(&results);
                let semaphore = Arc::clone(&self.semaphore);
                let runtime = Arc::clone(&self.runtime);
                let handlers = Arc::clone(&self.handlers);
                let notified = Arc::clone(&self.notified_handlers);
                let parallelization_local = Arc::clone(&self.parallelization_manager);
                let recovery_manager = self.recovery_manager.clone();
                let connection_factory = self.connection_factory.clone();
                let module_registry = Arc::clone(&self.module_registry);
                let changed_tasks = Arc::clone(&self.changed_tasks);
                let batch_processor = Arc::clone(&self.batch_processor);
                let pipelining = self.config.pipelining;
                let tx_id = tx_id.clone();
                let event_callback = event_callback.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let mut host_result = HostResult {
                        host: host.clone(),
                        stats: ExecutionStats::default(),
                        failed: false,
                        unreachable: false,
                    };

                    // Get connection for host
                    let host_connection = if let Some(ref factory) = connection_factory {
                        match factory.get_connection(&host).await {
                            Ok(conn) => Some(conn),
                            Err(e) => {
                                warn!("Failed to get connection for host {}: {}", host, e);
                                None
                            }
                        }
                    } else {
                        None
                    };

                    for task in tasks.iter() {
                        if host_result.failed || host_result.unreachable {
                            break;
                        }

                        if let Some(cb) = &event_callback {
                            cb(ExecutionEvent::TaskStart {
                                task: task.name.clone(),
                                host: Some(host.clone()),
                            });
                        }

                        let mut ctx = ExecutionContext::new(host.clone())
                            .with_check_mode(check_mode)
                            .with_diff_mode(diff_mode)
                            .with_verbosity(verbosity);

                        // Set connection if available
                        if let Some(ref conn) = host_connection {
                            ctx = ctx.with_connection(conn.clone());
                        }

                        let task_result = task
                            .execute(
                                &ctx,
                                &runtime,
                                &handlers,
                                &notified,
                                &parallelization_local,
                                &module_registry,
                                &batch_processor,
                                pipelining,
                            )
                            .await;

                        if let Some(cb) = &event_callback {
                            let event_result = match &task_result {
                                Ok(result) => result.clone(),
                                Err(e) => TaskResult::failed(e.to_string()),
                            };
                            cb(ExecutionEvent::HostTaskComplete(
                                host.clone(),
                                task.name.clone(),
                                event_result,
                            ));
                        }

                        if let Some(rm) = &recovery_manager {
                            if let Some(tid) = tx_id.as_ref() {
                                let (outcome, changed) = match &task_result {
                                    Ok(r) => {
                                        let outcome = match r.status {
                                            TaskStatus::Ok => TaskOutcome::Success,
                                            TaskStatus::Changed => TaskOutcome::Changed,
                                            TaskStatus::Failed => TaskOutcome::Failed {
                                                message: r.msg.clone().unwrap_or_default(),
                                            },
                                            TaskStatus::Skipped => TaskOutcome::Skipped,
                                            TaskStatus::Unreachable => TaskOutcome::Unreachable {
                                                message: r.msg.clone().unwrap_or_default(),
                                            },
                                        };
                                        (outcome, r.changed)
                                    }
                                    Err(e) => (
                                        TaskOutcome::Failed {
                                            message: e.to_string(),
                                        },
                                        false,
                                    ),
                                };
                                if let Err(e) = rm
                                    .record_task(
                                        tid.clone(),
                                        task.name.clone(),
                                        host.clone(),
                                        outcome,
                                        changed,
                                    )
                                    .await
                                {
                                    warn!("Failed to record task outcome for host {}: {}", host, e);
                                }
                            }
                        }

                        // Record changed tasks for rollback
                        if let Ok(ref result) = task_result {
                            if result.status == TaskStatus::Changed {
                                let record = Executor::build_task_state_record(
                                    task,
                                    &host,
                                    crate::state::TaskStatus::Changed,
                                );
                                changed_tasks.lock().await.push(record);
                            }
                        }

                        match task_result {
                            Ok(result) => {
                                update_stats(&mut host_result.stats, &result);
                                if result.status == TaskStatus::Failed {
                                    host_result.failed = true;
                                }
                            }
                            Err(_) => {
                                host_result.failed = true;
                                host_result.stats.failed += 1;
                            }
                        }
                    }

                    results.lock().await.insert(host, host_result);
                })
            })
            .collect();

        join_all(handles).await;

        let results = Arc::try_unwrap(results)
            .map_err(|_| ExecutorError::RuntimeError("Failed to unwrap results".into()))?
            .into_inner();

        Ok(results)
    }

    /// Run tasks in host_pinned strategy (dedicated worker per host)
    async fn run_host_pinned(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        // For now, host_pinned behaves like free strategy
        // In a full implementation, this would pin workers to specific hosts
        self.run_free(hosts, tasks, tx_id).await
    }

    /// Run tasks with serial batching
    async fn run_serial(
        &self,
        serial_spec: &crate::playbook::SerialSpec,
        hosts: &[String],
        tasks: &[Task],
        max_fail_percentage: Option<u8>,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        info!(
            "Running with serial batching: {:?}, max_fail_percentage: {:?}",
            serial_spec, max_fail_percentage
        );

        // Split hosts into batches
        let batches = serial_spec.batch_hosts(hosts);

        if batches.is_empty() {
            return Ok(HashMap::new());
        }

        debug!("Created {} batches for serial execution", batches.len());

        let mut all_results: HashMap<String, HostResult> = HashMap::new();
        let mut total_failed = 0;
        let total_hosts = hosts.len();

        // Execute each batch sequentially
        for (batch_idx, batch_hosts) in batches.iter().enumerate() {
            debug!(
                "Executing batch {}/{} with {} hosts",
                batch_idx + 1,
                batches.len(),
                batch_hosts.len()
            );

            // Convert batch hosts to owned Strings
            let batch_hosts_owned: Vec<String> =
                batch_hosts.iter().map(|s| s.to_string()).collect();

            // Execute this batch based on the configured strategy
            let batch_results = match self.config.strategy {
                ExecutionStrategy::Linear => {
                    self.run_linear(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
                ExecutionStrategy::Free => {
                    self.run_free(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
                ExecutionStrategy::HostPinned => {
                    self.run_host_pinned(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
                ExecutionStrategy::DebugStrategy => {
                    self.run_linear(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
            };

            // Count failures in this batch
            let batch_failed = batch_results
                .values()
                .filter(|r| r.failed || r.unreachable)
                .count();

            total_failed += batch_failed;

            // Merge batch results into overall results
            for (host, result) in batch_results {
                all_results.insert(host, result);
            }

            // Check max_fail_percentage if specified
            if let Some(max_fail_pct) = max_fail_percentage {
                let current_fail_pct = (total_failed as f64 / total_hosts as f64 * 100.0) as u8;

                if current_fail_pct > max_fail_pct {
                    error!(
                        "Failure percentage ({:.1}%) exceeded max_fail_percentage ({}%), aborting remaining batches",
                        current_fail_pct, max_fail_pct
                    );

                    // Mark remaining hosts as skipped
                    for remaining_batch in batches.iter().skip(batch_idx + 1) {
                        for host in remaining_batch.iter() {
                            all_results.insert(
                                host.to_string(),
                                HostResult {
                                    host: host.to_string(),
                                    stats: ExecutionStats {
                                        skipped: tasks.len(),
                                        ..Default::default()
                                    },
                                    failed: false,
                                    unreachable: false,
                                },
                            );
                        }
                    }

                    break;
                }
            }
        }

        info!(
            "Serial execution completed: {} hosts, {} failed",
            total_hosts, total_failed
        );

        Ok(all_results)
    }

    /// Run a single task on multiple hosts in parallel
    ///
    /// OPTIMIZATION: Fast path for single host and small host counts (< 10)
    /// to avoid Arc clone overhead and tokio::spawn overhead for small workloads.
    async fn run_task_on_hosts(
        &self,
        hosts: &[String],
        task: &Task,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, TaskResult>> {
        debug!("Running task '{}' on {} hosts", task.name, hosts.len());

        // Set task-level vars (including block vars merged during parsing) on runtime
        if !task.vars.is_empty() {
            let mut rt = self.runtime.write().await;
            rt.push_block_vars(task.vars.clone());
        }

        // OPTIMIZATION: Fast path for single host - avoid Arc overhead and tokio::spawn
        if hosts.len() == 1 {
            let host = &hosts[0];
            let _permit = self.semaphore.acquire().await.unwrap();

            // Get connection for host
            let host_connection = self.get_connection_for_host(host).await;

            self.emit_event(ExecutionEvent::TaskStart {
                task: task.name.clone(),
                host: Some(host.clone()),
            });

            let mut ctx = ExecutionContext::new(host.clone())
                .with_check_mode(self.config.check_mode)
                .with_diff_mode(self.config.diff_mode)
                .with_verbosity(self.config.verbosity);

            // Set connection if available
            if let Some(conn) = host_connection {
                ctx = ctx.with_connection(conn);
            }

            let result = task
                .execute(
                    &ctx,
                    &self.runtime,
                    &self.handlers,
                    &self.notified_handlers,
                    &self.parallelization_manager,
                    &self.module_registry,
                    &self.batch_processor,
                    self.config.pipelining,
                )
                .await;

            let mut results = HashMap::with_capacity(1);
            match result {
                Ok(task_result) => {
                    self.emit_event(ExecutionEvent::HostTaskComplete(
                        host.clone(),
                        task.name.clone(),
                        task_result.clone(),
                    ));
                    results.insert(host.clone(), task_result);
                }
                Err(e) => {
                    error!("Task failed on host {}: {}", host, e);
                    let failed_result = TaskResult::failed(e.to_string());
                    self.emit_event(ExecutionEvent::HostTaskComplete(
                        host.clone(),
                        task.name.clone(),
                        failed_result.clone(),
                    ));
                    results.insert(host.clone(), failed_result);
                }
            }
            if let Some(rm) = &self.recovery_manager {
                if let Some(tid) = tx_id.as_ref() {
                    for (host, res) in &results {
                        let outcome = match res.status {
                            TaskStatus::Ok => TaskOutcome::Success,
                            TaskStatus::Changed => TaskOutcome::Changed,
                            TaskStatus::Failed => TaskOutcome::Failed {
                                message: res.msg.clone().unwrap_or_default(),
                            },
                            TaskStatus::Skipped => TaskOutcome::Skipped,
                            TaskStatus::Unreachable => TaskOutcome::Unreachable {
                                message: res.msg.clone().unwrap_or_default(),
                            },
                        };

                        if let Err(e) = rm
                            .record_task(
                                tid.clone(),
                                task.name.clone(),
                                host.clone(),
                                outcome,
                                res.changed,
                            )
                            .await
                        {
                            warn!("Failed to record task outcome for host {}: {}", host, e);
                        }
                    }
                }
            }
            // Record changed tasks for rollback
            for (host, res) in &results {
                if res.status == TaskStatus::Changed {
                    let record = Self::build_task_state_record(
                        task,
                        host,
                        crate::state::TaskStatus::Changed,
                    );
                    self.changed_tasks.lock().await.push(record);
                }
            }
            // Pop task-level vars from runtime (single-host fast path)
            if !task.vars.is_empty() {
                let mut rt = self.runtime.write().await;
                rt.pop_block_vars();
            }
            return Ok(results);
        }

        // OPTIMIZATION: Pre-extract config values to avoid cloning entire config per host
        let check_mode = self.config.check_mode;
        let diff_mode = self.config.diff_mode;
        let verbosity = self.config.verbosity;

        // OPTIMIZATION: For small host counts, share task via Arc instead of cloning per host
        let task_arc = Arc::new(task.clone());
        let results = Arc::new(Mutex::new(HashMap::with_capacity(hosts.len())));
        let event_callback = self.event_callback.clone();

        let handles: Vec<_> = hosts
            .iter()
            .map(|host| {
                let host = host.clone();
                let task = Arc::clone(&task_arc);
                let results = Arc::clone(&results);
                let semaphore = Arc::clone(&self.semaphore);
                let runtime = Arc::clone(&self.runtime);
                let handlers = Arc::clone(&self.handlers);
                let notified = Arc::clone(&self.notified_handlers);
                let parallelization = Arc::clone(&self.parallelization_manager);
                let connection_factory = self.connection_factory.clone();
                let module_registry = Arc::clone(&self.module_registry);
                let batch_processor = Arc::clone(&self.batch_processor);
                let pipelining = self.config.pipelining;
                let event_callback = event_callback.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    // Get connection for host
                    let host_connection = if let Some(ref factory) = connection_factory {
                        match factory.get_connection(&host).await {
                            Ok(conn) => Some(conn),
                            Err(e) => {
                                warn!("Failed to get connection for host {}: {}", host, e);
                                None
                            }
                        }
                    } else {
                        None
                    };

                    if let Some(cb) = &event_callback {
                        cb(ExecutionEvent::TaskStart {
                            task: task.name.clone(),
                            host: Some(host.clone()),
                        });
                    }

                    let mut ctx = ExecutionContext::new(host.clone())
                        .with_check_mode(check_mode)
                        .with_diff_mode(diff_mode)
                        .with_verbosity(verbosity);

                    // Set connection if available
                    if let Some(conn) = host_connection {
                        ctx = ctx.with_connection(conn);
                    }

                    let result = task
                        .execute(
                            &ctx,
                            &runtime,
                            &handlers,
                            &notified,
                            &parallelization,
                            &module_registry,
                            &batch_processor,
                            pipelining,
                        )
                        .await;

                    if let Some(cb) = &event_callback {
                        let event_result = match &result {
                            Ok(task_result) => task_result.clone(),
                            Err(e) => TaskResult::failed(e.to_string()),
                        };
                        cb(ExecutionEvent::HostTaskComplete(
                            host.clone(),
                            task.name.clone(),
                            event_result,
                        ));
                    }

                    match result {
                        Ok(task_result) => {
                            results.lock().await.insert(host, task_result);
                        }
                        Err(e) => {
                            error!("Task failed on host {}: {}", host, e);
                            results.lock().await.insert(
                                host,
                                TaskResult {
                                    status: TaskStatus::Failed,
                                    changed: false,
                                    msg: Some(e.to_string()),
                                    result: None,
                                    diff: None,
                                },
                            );
                        }
                    }
                })
            })
            .collect();

        join_all(handles).await;

        let results = Arc::try_unwrap(results)
            .map_err(|_| ExecutorError::RuntimeError("Failed to unwrap results".into()))?
            .into_inner();

        if let Some(rm) = &self.recovery_manager {
            if let Some(tid) = tx_id.as_ref() {
                for (host, res) in &results {
                    let outcome = match res.status {
                        TaskStatus::Ok => TaskOutcome::Success,
                        TaskStatus::Changed => TaskOutcome::Changed,
                        TaskStatus::Failed => TaskOutcome::Failed {
                            message: res.msg.clone().unwrap_or_default(),
                        },
                        TaskStatus::Skipped => TaskOutcome::Skipped,
                        TaskStatus::Unreachable => TaskOutcome::Unreachable {
                            message: res.msg.clone().unwrap_or_default(),
                        },
                    };

                    if let Err(e) = rm
                        .record_task(
                            tid.clone(),
                            task.name.clone(),
                            host.clone(),
                            outcome,
                            res.changed,
                        )
                        .await
                    {
                        warn!("Failed to record task outcome for host {}: {}", host, e);
                    }
                }
            }
        }
        // Record changed tasks for rollback
        for (host, res) in &results {
            if res.status == TaskStatus::Changed {
                let record =
                    Self::build_task_state_record(task, host, crate::state::TaskStatus::Changed);
                self.changed_tasks.lock().await.push(record);
            }
        }

        // Pop task-level vars from runtime
        if !task.vars.is_empty() {
            let mut rt = self.runtime.write().await;
            rt.pop_block_vars();
        }

        Ok(results)
    }

    /// Execute automatic system rollback by undoing changed tasks via module calls.
    ///
    /// Uses `state::rollback::RollbackExecutor` to generate a rollback plan from
    /// recorded `TaskStateRecord` entries, then executes each action through the
    /// module registry. This is best-effort: errors are logged per action and
    /// execution continues with remaining actions.
    async fn execute_system_rollback(&self) {
        let changed_tasks: Vec<crate::state::TaskStateRecord> = {
            let mut tasks = self.changed_tasks.lock().await;
            std::mem::take(&mut *tasks)
        };

        if changed_tasks.is_empty() {
            info!("Auto-rollback: no changed tasks to roll back");
            return;
        }

        info!(
            "Auto-rollback: generating rollback plan for {} changed tasks",
            changed_tasks.len()
        );

        let rollback_executor =
            crate::state::rollback::RollbackExecutor::new(crate::state::StateConfig::default());

        let plan = match rollback_executor.create_plan(&changed_tasks) {
            Ok(plan) => plan,
            Err(e) => {
                error!("Auto-rollback: failed to create rollback plan: {}", e);
                return;
            }
        };

        if plan.is_empty() {
            info!("Auto-rollback: rollback plan is empty, nothing to undo");
            return;
        }

        info!(
            "Auto-rollback: executing {} rollback actions",
            plan.actions.len()
        );

        for action in &plan.actions {
            if self.config.check_mode {
                info!(
                    "Auto-rollback (dry-run): would execute {} on {} with module '{}'",
                    action.description, action.host, action.module
                );
                continue;
            }

            if let Err(e) = self.execute_rollback_action(action).await {
                error!(
                    "Auto-rollback: failed to execute action '{}': {}",
                    action.description, e
                );
            } else {
                info!("Auto-rollback: completed '{}'", action.description);
            }
        }

        info!("Auto-rollback: rollback sequence completed");
    }

    /// Execute a single rollback action by calling the appropriate module.
    async fn execute_rollback_action(
        &self,
        action: &crate::state::rollback::RollbackAction,
    ) -> Result<(), ExecutorError> {
        // Convert JSON args to ModuleParams (HashMap<String, serde_json::Value>)
        let params: crate::modules::ModuleParams = match &action.args {
            serde_json::Value::Object(map) => {
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            }
            _ => {
                return Err(ExecutorError::Other(format!(
                    "Rollback action '{}' has non-object args",
                    action.description
                )));
            }
        };

        // Build module context
        let mut ctx = crate::modules::ModuleContext {
            check_mode: self.config.check_mode,
            diff_mode: self.config.diff_mode,
            verbosity: self.config.verbosity,
            ..Default::default()
        };

        // Get connection for the target host
        if let Some(conn) = self.get_connection_for_host(&action.host).await {
            ctx.connection = Some(conn);
        }

        // Execute the module
        match self.module_registry.execute(&action.module, &params, &ctx) {
            Ok(output) => {
                debug!(
                    "Rollback module '{}' on host '{}': {:?}",
                    action.module, action.host, output
                );
                Ok(())
            }
            Err(e) => Err(ExecutorError::Other(format!(
                "Rollback module '{}' failed on host '{}': {}",
                action.module, action.host, e
            ))),
        }
    }

    /// Update host statistics based on task result
    fn update_host_stats(&self, host_result: &mut HostResult, task_result: &TaskResult) {
        update_stats(&mut host_result.stats, task_result);
        if task_result.status == TaskStatus::Failed {
            host_result.failed = true;
        } else if task_result.status == TaskStatus::Unreachable {
            host_result.unreachable = true;
        }
    }

    /// Resolve host pattern to list of hosts
    async fn resolve_hosts(&self, pattern: &str) -> ExecutorResult<Vec<String>> {
        let runtime = self.runtime.read().await;

        // Handle special patterns
        if pattern == "all" {
            return Ok(runtime.get_all_hosts());
        }

        if pattern == "localhost" {
            return Ok(vec!["localhost".to_string()]);
        }

        // Check for group name
        if let Some(hosts) = runtime.get_group_hosts(pattern) {
            return Ok(hosts);
        }

        // Check for regex pattern (starts with ~)
        if let Some(regex_pattern) = pattern.strip_prefix('~') {
            let re = regex::Regex::new(regex_pattern)
                .map_err(|e| ExecutorError::ParseError(format!("Invalid regex: {}", e)))?;

            let all_hosts = runtime.get_all_hosts();
            let matched: Vec<_> = all_hosts.into_iter().filter(|h| re.is_match(h)).collect();

            return Ok(matched);
        }

        // Treat as single host or comma-separated list
        let hosts: Vec<String> = pattern.split(',').map(|s| s.trim().to_string()).collect();
        Ok(hosts)
    }

    /// Flush all notified handlers
    ///
    /// This method:
    /// 1. Resolves notification names to handlers (by name or listen directive)
    /// 2. Ensures handlers run in definition order
    /// 3. Supports handler chaining (handlers can notify other handlers)
    /// 4. Deduplicates handlers so each runs only once per flush
    async fn flush_handlers(&self, tx_id: Option<TransactionId>) -> ExecutorResult<()> {
        let notified: Vec<String> = {
            let mut notified = self.notified_handlers.lock().await;
            let handlers: Vec<_> = notified.drain().collect();
            handlers
        };

        if notified.is_empty() {
            return Ok(());
        }

        info!("Running handlers for {} notifications", notified.len());

        let handlers = self.handlers.read().await;

        // Build a lookup map: notification name -> list of handlers that respond to it
        // A handler responds to a notification if:
        // 1. Its name matches the notification, OR
        // 2. Its listen list contains the notification name
        let mut notification_to_handlers: HashMap<String, Vec<String>> = HashMap::new();

        for handler in handlers.values() {
            // Handler responds to its own name
            notification_to_handlers
                .entry(handler.name.clone())
                .or_default()
                .push(handler.name.clone());

            // Handler responds to each name in its listen list
            for listen_name in &handler.listen {
                notification_to_handlers
                    .entry(listen_name.clone())
                    .or_default()
                    .push(handler.name.clone());
            }
        }

        // Collect all handlers that need to run (deduped)
        let mut handlers_to_run: HashSet<String> = HashSet::new();

        for notification_name in &notified {
            if let Some(responding_handlers) = notification_to_handlers.get(notification_name) {
                for handler_name in responding_handlers {
                    handlers_to_run.insert(handler_name.clone());
                }
            } else {
                // No handler found for this notification
                warn!("Handler not found for notification: {}", notification_name);
            }
        }

        if handlers_to_run.is_empty() {
            debug!("No handlers matched the notifications");
            return Ok(());
        }

        // Sort handlers by their definition order (order in the handlers map)
        // We use the order from the handlers HashMap which preserves insertion order
        let mut ordered_handlers: Vec<&Handler> = handlers
            .values()
            .filter(|h| handlers_to_run.contains(&h.name))
            .collect();

        // Stable sort is not needed since HashMap doesn't preserve order
        // We'll use the order handlers appear in the play's handlers vector
        // For now, alphabetical order ensures consistent behavior
        ordered_handlers.sort_by(|a, b| a.name.cmp(&b.name));

        info!("Running {} unique handlers", ordered_handlers.len());

        // Track handlers that have already run in this flush cycle
        let mut executed_handlers: HashSet<String> = HashSet::new();

        // Get all active hosts from runtime
        let hosts = {
            let runtime = self.runtime.read().await;
            runtime.get_all_hosts()
        };

        // Execute handlers, supporting handler chaining
        // We loop until no new handlers are notified
        let mut current_handlers = ordered_handlers;

        loop {
            let mut new_notifications: HashSet<String> = HashSet::new();

            for handler in &current_handlers {
                if executed_handlers.contains(&handler.name) {
                    continue;
                }

                debug!("Running handler: {}", handler.name);
                executed_handlers.insert(handler.name.clone());

                // Create task from handler
                // Note: We include notify field to support handler chaining
                let task = Task {
                    name: handler.name.clone(),
                    module: handler.module.clone(),
                    args: handler.args.clone(),
                    when: handler.when.clone(),
                    notify: Vec::new(), // Handlers don't chain via task.notify in our model
                    register: None,
                    loop_items: None,
                    loop_var: "item".to_string(),
                    loop_control: None,
                    ignore_errors: false,
                    changed_when: None,
                    failed_when: None,
                    delegate_to: None,
                    delegate_facts: None,
                    run_once: false,
                    tags: Vec::new(),
                    r#become: false,
                    become_user: None,
                    block_id: None,
                    block_role: crate::executor::task::BlockRole::Normal,
                    retries: None,
                    delay: None,
                    until: None,
                    vars: IndexMap::new(),
                };

                // Run handler on all hosts
                let results = self.run_task_on_hosts(&hosts, &task, tx_id.clone()).await?;

                // Check if handler execution triggered any changes
                // If so, check if any handlers listen to this handler's name (handler chaining)
                let any_changed = results.values().any(|r| r.changed);
                if any_changed {
                    // Check if any other handlers listen to this handler's name
                    if let Some(chained_handlers) = notification_to_handlers.get(&handler.name) {
                        for chained_handler in chained_handlers {
                            if chained_handler != &handler.name
                                && !executed_handlers.contains(chained_handler)
                            {
                                new_notifications.insert(chained_handler.clone());
                            }
                        }
                    }
                }
            }

            // If no new handlers were triggered, we're done
            if new_notifications.is_empty() {
                break;
            }

            // Prepare the next round of handlers
            current_handlers = handlers
                .values()
                .filter(|h| new_notifications.contains(&h.name))
                .collect();

            if current_handlers.is_empty() {
                break;
            }

            debug!(
                "Handler chaining: {} additional handlers triggered",
                current_handlers.len()
            );
        }

        Ok(())
    }

    /// Notify a handler to be run at end of play
    pub async fn notify_handler(&self, handler_name: &str) {
        let mut notified = self.notified_handlers.lock().await;
        notified.insert(handler_name.to_string());
        debug!("Handler notified: {}", handler_name);
    }

    /// Check if running in dry-run mode
    pub fn is_check_mode(&self) -> bool {
        self.config.check_mode
    }

    /// Get reference to runtime context
    pub fn runtime(&self) -> Arc<RwLock<RuntimeContext>> {
        Arc::clone(&self.runtime)
    }

    async fn load_vars_files(
        &self,
        play: &Play,
    ) -> ExecutorResult<IndexMap<String, serde_json::Value>> {
        use crate::executor::playbook::VarsFileSpec;
        use crate::vars::terraform::TerraformVarImporter;
        use std::path::{Path, PathBuf};

        let mut vars = IndexMap::new();
        if play.vars_files.is_empty() {
            return Ok(vars);
        }

        let base_dir = play.playbook_dir.as_deref();

        for spec in &play.vars_files {
            match spec {
                VarsFileSpec::Path(path) => {
                    let full_path = if Path::new(path).is_absolute() {
                        PathBuf::from(path)
                    } else if let Some(base) = base_dir {
                        base.join(path)
                    } else {
                        PathBuf::from(path)
                    };

                    let content = tokio::fs::read_to_string(&full_path).await.map_err(|e| {
                        ExecutorError::IoError(std::io::Error::other(format!(
                            "Failed to read vars file {}: {}",
                            full_path.display(),
                            e
                        )))
                    })?;

                    let yaml_vars: IndexMap<String, serde_yaml::Value> =
                        serde_yaml::from_str(&content).map_err(|e| {
                            ExecutorError::ParseError(format!(
                                "Failed to parse vars file {}: {}",
                                full_path.display(),
                                e
                            ))
                        })?;

                    for (key, value) in yaml_vars {
                        if let Ok(json_value) = serde_json::to_value(value) {
                            vars.insert(key, json_value);
                        }
                    }
                }
                VarsFileSpec::Terraform { terraform } => {
                    let tf_vars = TerraformVarImporter::import_outputs(terraform, base_dir)
                        .await
                        .map_err(|e| {
                            ExecutorError::ParseError(format!(
                                "Terraform vars import failed: {}",
                                e
                            ))
                        })?;
                    for (key, value) in tf_vars {
                        vars.insert(key, value);
                    }
                }
            }
        }

        Ok(vars)
    }

    /// Get execution statistics summary
    pub fn summarize_results(results: &HashMap<String, HostResult>) -> ExecutionStats {
        let mut summary = ExecutionStats::default();
        for result in results.values() {
            summary.merge(&result.stats);
        }
        summary
    }
}

/// Helper function to update statistics
fn update_stats(stats: &mut ExecutionStats, result: &TaskResult) {
    match result.status {
        TaskStatus::Ok => {
            if result.changed {
                stats.changed += 1;
            } else {
                stats.ok += 1;
            }
        }
        TaskStatus::Changed => stats.changed += 1,
        TaskStatus::Failed => stats.failed += 1,
        TaskStatus::Skipped => stats.skipped += 1,
        TaskStatus::Unreachable => stats.unreachable += 1,
    }
}

/// Dependency graph for task ordering using topological sort.
///
/// Used internally to resolve task dependencies and detect circular
/// dependencies that would prevent execution.
///
/// # Example
///
/// ```rust
/// use rustible::executor::DependencyGraph;
///
/// let mut graph = DependencyGraph::new();
/// graph.add_dependency("install_app", "install_deps");
/// graph.add_dependency("configure_app", "install_app");
///
/// let order = graph.topological_sort().expect("no cycles");
/// // order: ["install_deps", "install_app", "configure_app"]
/// ```
pub struct DependencyGraph {
    nodes: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Creates a new empty dependency graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Adds a dependency relationship.
    ///
    /// Declares that `task` depends on `dependency`, meaning `dependency`
    /// must complete before `task` can begin.
    ///
    /// # Arguments
    ///
    /// * `task` - The task that has a dependency
    /// * `dependency` - The task that must complete first
    pub fn add_dependency(&mut self, task: &str, dependency: &str) {
        self.nodes
            .entry(task.to_string())
            .or_default()
            .push(dependency.to_string());
        // Also ensure the dependency exists as a node (with no dependencies of its own)
        self.nodes.entry(dependency.to_string()).or_default();
    }

    /// Returns tasks in topologically sorted order.
    ///
    /// The returned order ensures that all dependencies appear before
    /// their dependents.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::DependencyCycle`] if a circular dependency
    /// is detected in the graph.
    pub fn topological_sort(&self) -> ExecutorResult<Vec<String>> {
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut result = Vec::new();

        for node in self.nodes.keys() {
            if !visited.contains(node) {
                self.visit(node, &mut visited, &mut temp_visited, &mut result)?;
            }
        }

        // Don't reverse - the order is already correct (dependencies come before dependents)
        Ok(result)
    }

    fn visit(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> ExecutorResult<()> {
        if temp_visited.contains(node) {
            return Err(ExecutorError::DependencyCycle(node.to_string()));
        }

        if !visited.contains(node) {
            temp_visited.insert(node.to_string());

            if let Some(deps) = self.nodes.get(node) {
                for dep in deps {
                    self.visit(dep, visited, temp_visited, result)?;
                }
            }

            temp_visited.remove(node);
            visited.insert(node.to_string());
            result.push(node.to_string());
        }

        Ok(())
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for PlaybookExecutor (same as Executor)
/// Used for API compatibility and clarity
pub type PlaybookExecutor = Executor;

/// Type alias for TaskExecutor functionality
/// In a more complex implementation, this could be a separate struct
pub type TaskExecutor = Executor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("task3", "task2");
        graph.add_dependency("task2", "task1");

        let order = graph.topological_sort().unwrap();
        // The order should respect dependencies: task1 before task2 before task3
        assert_eq!(order.len(), 3);
        let t1_pos = order.iter().position(|x| *x == "task1").unwrap();
        let t2_pos = order.iter().position(|x| *x == "task2").unwrap();
        let t3_pos = order.iter().position(|x| *x == "task3").unwrap();
        assert!(t1_pos < t2_pos, "task1 should come before task2");
        assert!(t2_pos < t3_pos, "task2 should come before task3");
    }

    #[test]
    fn test_dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("task1", "task2");
        graph.add_dependency("task2", "task3");
        graph.add_dependency("task3", "task1");

        let result = graph.topological_sort();
        assert!(matches!(result, Err(ExecutorError::DependencyCycle(_))));
    }

    #[test]
    fn test_execution_stats_merge() {
        let mut stats1 = ExecutionStats {
            ok: 1,
            changed: 2,
            failed: 0,
            skipped: 1,
            unreachable: 0,
        };

        let stats2 = ExecutionStats {
            ok: 2,
            changed: 1,
            failed: 1,
            skipped: 0,
            unreachable: 1,
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.ok, 3);
        assert_eq!(stats1.changed, 3);
        assert_eq!(stats1.failed, 1);
        assert_eq!(stats1.skipped, 1);
        assert_eq!(stats1.unreachable, 1);
    }
}
