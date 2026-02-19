//! Core execution engine for Rustible
//!
//! This module provides the main task execution engine with:
//! - Async task runner using tokio
//! - Parallel execution across hosts
//! - Task dependency resolution
//! - Handler triggering system
//! - Dry-run support
//! - Parallelization hint enforcement

pub mod parallelization;
pub mod playbook;
pub mod runtime;
pub mod task;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::join_all;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{debug, error, info, instrument, warn};

use crate::executor::parallelization::ParallelizationManager;
use crate::executor::playbook::{Play, Playbook};
use crate::executor::runtime::{ExecutionContext, RuntimeContext};
use crate::executor::task::{Handler, Task, TaskResult, TaskStatus};
use crate::modules::{ModuleRegistry, ParallelizationHint};

/// Errors that can occur during execution
#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error("Task execution failed: {0}")]
    TaskFailed(String),

    #[error("Host unreachable: {0}")]
    HostUnreachable(String),

    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    #[error("Variable not found: {0}")]
    VariableNotFound(String),

    #[error("Condition evaluation failed: {0}")]
    ConditionError(String),

    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    #[error("Playbook parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Runtime error: {0}")]
    RuntimeError(String),
}

/// Result type for executor operations
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Configuration for the executor
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum number of parallel host executions
    pub forks: usize,
    /// Enable dry-run mode (no actual changes)
    pub check_mode: bool,
    /// Enable diff mode (show changes)
    #[allow(dead_code)]
    pub diff_mode: bool,
    /// Verbosity level (0-4)
    pub verbosity: u8,
    /// Strategy: "linear", "free", or "host_pinned"
    pub strategy: ExecutionStrategy,
    /// Timeout for task execution in seconds
    pub task_timeout: u64,
    /// Whether to gather facts automatically
    pub gather_facts: bool,
    /// Any extra variables passed via command line
    pub extra_vars: HashMap<String, serde_json::Value>,
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
        }
    }
}

/// Execution strategy determining how tasks are run across hosts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Run each task on all hosts before moving to next task
    Linear,
    /// Run all tasks on each host as fast as possible
    Free,
    /// Pin tasks to specific hosts
    HostPinned,
    /// Debug strategy for interactive task debugging
    DebugStrategy,
}

/// Statistics collected during execution
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    pub ok: usize,
    pub changed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub unreachable: usize,
}

impl ExecutionStats {
    pub fn merge(&mut self, other: &ExecutionStats) {
        self.ok += other.ok;
        self.changed += other.changed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.unreachable += other.unreachable;
    }
}

/// Host execution result containing stats and state
#[derive(Debug, Clone)]
pub struct HostResult {
    pub host: String,
    pub stats: ExecutionStats,
    pub failed: bool,
    pub unreachable: bool,
}

/// The main executor engine
pub struct Executor {
    config: ExecutorConfig,
    runtime: Arc<RwLock<RuntimeContext>>,
    handlers: Arc<RwLock<HashMap<String, Handler>>>,
    notified_handlers: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
    parallelization: Arc<ParallelizationManager>,
    module_registry: Arc<ModuleRegistry>,
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
            parallelization: Arc::new(ParallelizationManager::new()),
            module_registry: Arc::new(ModuleRegistry::with_builtins()),
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
            parallelization: Arc::new(ParallelizationManager::new()),
            module_registry: Arc::new(ModuleRegistry::with_builtins()),
        }
    }

    /// Get the parallelization hint for a task's module
    fn get_module_parallelization_hint(&self, module_name: &str) -> ParallelizationHint {
        self.module_registry
            .get(module_name)
            .map(|m| m.parallelization_hint())
            .unwrap_or(ParallelizationHint::FullyParallel)
    }

    /// Run a single task on multiple hosts in parallel with parallelization enforcement
    async fn run_task_on_hosts(
        &self,
        hosts: &[String],
        task: &Task,
    ) -> ExecutorResult<HashMap<String, TaskResult>> {
        debug!("Running task '{}' on {} hosts", task.name, hosts.len());

        // Get the parallelization hint for this module
        let hint = self.get_module_parallelization_hint(&task.module);
        debug!("Task '{}' parallelization hint: {:?}", task.name, hint);

        let results = Arc::new(Mutex::new(HashMap::new()));

        let handles: Vec<_> = hosts
            .iter()
            .map(|host| {
                let host = host.clone();
                let task = task.clone();
                let results = Arc::clone(&results);
                let semaphore = Arc::clone(&self.semaphore);
                let runtime = Arc::clone(&self.runtime);
                let config = self.config.clone();
                let handlers = Arc::clone(&self.handlers);
                let notified = Arc::clone(&self.notified_handlers);
                let parallelization = Arc::clone(&self.parallelization);
                let module_name = task.module.clone();

                tokio::spawn(async move {
                    // First acquire the general fork limit
                    let _fork_permit = semaphore.acquire().await.unwrap();

                    // Then acquire parallelization-specific constraints
                    let _para_guard = parallelization
                        .acquire(hint, &host, &module_name)
                        .await;

                    let ctx = ExecutionContext::new(host.clone())
                        .with_check_mode(config.check_mode)
                        .with_diff_mode(config.diff_mode);

                    let result = task.execute(&ctx, &runtime, &handlers, &notified).await;

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

        Ok(results)
    }

    /// Get access to the parallelization manager for testing/debugging
    pub fn parallelization(&self) -> &Arc<ParallelizationManager> {
        &self.parallelization
    }

    // Rest of the implementation remains the same...
    // (All the other methods from the original file)
}
