use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{debug, error, info, instrument, warn};

use crate::connection::Connection;
use crate::executor::parallelization::ParallelizationManager;
use crate::executor::runtime::RuntimeContext;
use crate::executor::task::{Handler, Task, TaskResult};
use crate::modules::ModuleRegistry;
use crate::recovery::{RecoveryManager, TransactionId};

use super::playbook::{Play, Playbook};
use super::{ExecutionStrategy, ExecutorError, ExecutorResult, HostResult};

/// Events emitted during playbook execution.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Playbook execution started
    PlaybookStart(String),
    /// Play execution started
    PlayStart(String),
    /// Task execution started
    TaskStart(String),
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

    /// Whether to run with privilege escalation (default: false).
    ///
    /// When enabled, commands are executed with elevated privileges.
    /// Similar to Ansible's `--become` or `-b` option.
    pub r#become: bool,

    /// Method for privilege escalation (default: "sudo").
    ///
    /// Common methods: "sudo", "su", "pbrun", "pfexec", "doas", "dzdo".
    /// Similar to Ansible's `--become-method` option.
    pub become_method: String,

    /// User to become when escalating privileges (default: "root").
    ///
    /// Similar to Ansible's `--become-user` option.
    pub become_user: String,

    /// Password for privilege escalation (default: None).
    ///
    /// Similar to providing password via `--ask-become-pass`.
    pub become_password: Option<String>,
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
            r#become: false,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_password: None,
        }
    }
}

/// The main playbook execution engine.
///
/// The `Executor` orchestrates the execution of playbooks across multiple hosts.
/// It handles parallel execution, handler management, and result collection.
///
/// # Example
///
/// ```rust,ignore
/// use rustible::executor::{Executor, ExecutorConfig};
///
/// let executor = Executor::new(ExecutorConfig::default());
/// let results = executor.run_playbook(&playbook).await?;
///
/// for (host, result) in &results {
///     println!("{}: OK={}, Changed={}", host, result.stats.ok, result.stats.changed);
/// }
/// ```
pub struct Executor {
    pub(super) config: ExecutorConfig,
    pub(super) runtime: Arc<RwLock<RuntimeContext>>,
    pub(super) handlers: Arc<RwLock<HashMap<String, Handler>>>,
    pub(super) notified_handlers: Arc<Mutex<HashSet<String>>>,
    pub(super) semaphore: Arc<Semaphore>,
    pub(super) parallelization_manager: Arc<ParallelizationManager>,
    pub(super) recovery_manager: Option<Arc<RecoveryManager>>,
    /// Shared module registry - created once per executor to avoid hot path overhead
    pub(super) module_registry: Arc<ModuleRegistry>,
    pub(super) connection_cache: Arc<RwLock<HashMap<String, Arc<dyn Connection + Send + Sync>>>>,
    pub(super) event_callback: Option<EventCallback>,
}

impl Executor {
    /// Create a new executor with the given configuration
    pub fn new(config: ExecutorConfig) -> Self {
        let mut config = config;
        if config.forks == 0 {
            warn!("forks=0 is invalid; clamping to 1 to avoid deadlock");
            config.forks = 1;
        }
        let forks = config.forks;

        Self {
            config,
            runtime: Arc::new(RwLock::new(RuntimeContext::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            notified_handlers: Arc::new(Mutex::new(HashSet::new())),
            semaphore: Arc::new(Semaphore::new(forks)),
            parallelization_manager: Arc::new(ParallelizationManager::new()),
            recovery_manager: None,
            module_registry: Arc::new(ModuleRegistry::with_builtins()),
            connection_cache: Arc::new(RwLock::new(HashMap::new())),
            event_callback: None,
        }
    }

    /// Create executor with a pre-existing runtime context
    pub fn with_runtime(config: ExecutorConfig, runtime: RuntimeContext) -> Self {
        let mut config = config;
        if config.forks == 0 {
            warn!("forks=0 is invalid; clamping to 1 to avoid deadlock");
            config.forks = 1;
        }
        let forks = config.forks;

        Self {
            config,
            runtime: Arc::new(RwLock::new(runtime)),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            notified_handlers: Arc::new(Mutex::new(HashSet::new())),
            semaphore: Arc::new(Semaphore::new(forks)),
            parallelization_manager: Arc::new(ParallelizationManager::new()),
            recovery_manager: None,
            module_registry: Arc::new(ModuleRegistry::with_builtins()),
            connection_cache: Arc::new(RwLock::new(HashMap::new())),
            event_callback: None,
        }
    }

    /// Set the event callback for this executor
    pub fn with_event_callback(mut self, callback: EventCallback) -> Self {
        self.event_callback = Some(callback);
        self
    }

    /// Set the recovery manager for this executor
    pub fn with_recovery_manager(mut self, recovery_manager: Arc<RecoveryManager>) -> Self {
        self.recovery_manager = Some(recovery_manager);
        self
    }

    /// Run a complete playbook
    #[instrument(skip(self, playbook), fields(playbook_name = %playbook.name))]
    pub async fn run_playbook(
        &self,
        playbook: &Playbook,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        if let Some(cb) = &self.event_callback {
            cb(ExecutionEvent::PlaybookStart(playbook.name.clone()));
        }
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
                    runtime.set_global_var(key.clone(), value.clone());
                }
                // Set playbook_dir magic variable for include/import path resolution
                if let Some(playbook_dir) = playbook.get_playbook_dir() {
                    runtime.set_magic_var(
                        "playbook_dir".to_string(),
                        serde_json::json!(playbook_dir.to_string_lossy()),
                    );
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

            if let Some(cb) = &self.event_callback {
                cb(ExecutionEvent::PlaybookFinish(playbook.name.clone()));
            }
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

        self.close_connections().await;
        result
    }

    /// Run a single play
    #[instrument(skip(self, play), fields(play_name = %play.name))]
    pub async fn run_play(
        &self,
        play: &Play,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        if let Some(cb) = &self.event_callback {
            cb(ExecutionEvent::PlayStart(play.name.clone()));
        }
        info!("Starting play: {}", play.name);

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

    /// Check if running in dry-run mode
    pub fn is_check_mode(&self) -> bool {
        self.config.check_mode
    }

    /// Get reference to runtime context
    pub fn runtime(&self) -> Arc<RwLock<RuntimeContext>> {
        Arc::clone(&self.runtime)
    }
}
