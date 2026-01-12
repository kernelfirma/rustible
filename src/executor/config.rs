use std::collections::HashMap;

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
}
