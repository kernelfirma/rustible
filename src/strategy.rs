//! Execution strategies for Rustible
//!
//! This module provides different execution strategies that control how tasks
//! are distributed and executed across hosts. Each strategy offers different
//! trade-offs between predictability, throughput, and debugging capabilities.
//!
//! # Built-in Strategies
//!
//! - **Linear**: Run each task on all hosts before moving to the next task.
//!   This provides predictable execution order and is the default strategy.
//!
//! - **Free**: Each host runs independently at maximum speed. Tasks are not
//!   synchronized across hosts, providing maximum throughput.
//!
//! - **HostPinned**: Dedicated workers per host for optimal connection reuse
//!   and cache locality.
//!
//! - **Debug**: Step-by-step execution with detailed logging and optional
//!   breakpoints for troubleshooting playbooks.
//!
//! # Custom Strategies
//!
//! Custom strategies can be implemented using the [`StrategyPlugin`] trait.
//! This allows for specialized execution patterns tailored to specific use cases.
//!
//! # Example
//!
//! ```rust
//! use rustible::strategy::{Strategy, StrategyConfig};
//!
//! // Use the default linear strategy
//! let strategy = Strategy::default();
//! assert_eq!(strategy, Strategy::Linear);
//!
//! // Configure a custom strategy
//! let config = StrategyConfig::new(Strategy::Free)
//!     .with_batch_size(10)
//!     .with_fail_fast(true);
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// Core Strategy Enum
// ============================================================================

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
/// | Debug | Step-by-step with logging | Troubleshooting |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Strategy {
    /// Linear - run each task on all hosts before next task.
    ///
    /// This is the default strategy and provides predictable execution order.
    /// Task N completes on all hosts before task N+1 begins on any host.
    /// Useful when task ordering across hosts matters.
    #[default]
    Linear,

    /// Free - each host runs independently as fast as possible.
    ///
    /// Each host proceeds through the task list at its own pace.
    /// Provides maximum throughput but less predictable ordering.
    /// Best for independent tasks where host synchronization isn't needed.
    Free,

    /// Host pinned - dedicated worker per host.
    ///
    /// Similar to `Free` but optimizes for connection reuse and
    /// cache locality by keeping the same worker for each host.
    /// Best for long-running playbooks with many tasks per host.
    HostPinned,

    /// Debug - step-by-step execution with detailed logging.
    ///
    /// Executes tasks one at a time with verbose output.
    /// Supports optional breakpoints and state inspection.
    /// Best for troubleshooting failing playbooks.
    Debug,
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Linear => write!(f, "linear"),
            Self::Free => write!(f, "free"),
            Self::HostPinned => write!(f, "host_pinned"),
            Self::Debug => write!(f, "debug"),
        }
    }
}

impl std::str::FromStr for Strategy {
    type Err = StrategyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "linear" => Ok(Self::Linear),
            "free" => Ok(Self::Free),
            "host_pinned" | "host-pinned" | "hostpinned" => Ok(Self::HostPinned),
            "debug" => Ok(Self::Debug),
            _ => Err(StrategyError::UnknownStrategy(s.to_string())),
        }
    }
}

impl Strategy {
    /// Returns all available built-in strategies.
    pub const fn all() -> &'static [Strategy] {
        &[
            Strategy::Linear,
            Strategy::Free,
            Strategy::HostPinned,
            Strategy::Debug,
        ]
    }

    /// Returns a brief description of the strategy.
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Linear => "Run each task on all hosts before next task",
            Self::Free => "Each host runs independently at maximum speed",
            Self::HostPinned => "Dedicated worker per host for connection reuse",
            Self::Debug => "Step-by-step execution with detailed logging",
        }
    }

    /// OPTIMIZATION: Quick strategy selection for small workloads.
    ///
    /// For small workloads (< 10 hosts and < 10 tasks), returns a recommended
    /// strategy without complex analysis. This avoids overhead of strategy
    /// selection logic for trivial cases.
    ///
    /// Returns `None` if the workload is large enough to warrant analysis.
    #[inline]
    pub fn quick_select_for_small_workload(host_count: usize, task_count: usize) -> Option<Self> {
        // For very small workloads, Linear is optimal - avoids overhead
        if host_count <= 1 || task_count <= 1 {
            return Some(Self::Linear);
        }

        // For small workloads (< 10 hosts, < 10 tasks), use simple heuristic
        if host_count < 10 && task_count < 10 {
            // Free strategy has lowest overhead for small parallel execution
            return Some(Self::Free);
        }

        // Large workload - needs proper analysis
        None
    }

    /// Check if this is a small workload that benefits from fast-path execution.
    #[inline]
    pub fn is_small_workload(host_count: usize, task_count: usize) -> bool {
        host_count < 10 && task_count < 10
    }

    /// Select optimal strategy based on workload characteristics.
    ///
    /// Uses heuristics to choose the best strategy for the given workload.
    pub fn select_optimal(characteristics: &WorkloadCharacteristics) -> Self {
        // For single host or single task, linear is always best
        if characteristics.host_count <= 1 || characteristics.task_count <= 1 {
            return Self::Linear;
        }

        // Debug mode for troubleshooting
        if characteristics.debug_mode {
            return Self::Debug;
        }

        // High failure rates benefit from linear (easier to track)
        if characteristics.expected_failure_rate > 0.3 {
            return Self::Linear;
        }

        // Tasks with dependencies must use linear
        if characteristics.has_dependencies {
            return Self::Linear;
        }

        // Many hosts with independent tasks benefit from free
        if characteristics.host_count > 10 && characteristics.avg_task_duration_ms > 100 {
            return Self::Free;
        }

        // Many short tasks with many hosts - host_pinned reduces context switching
        if characteristics.host_count > 20
            && characteristics.task_count > 50
            && characteristics.avg_task_duration_ms < 50
        {
            return Self::HostPinned;
        }

        // Large workloads with long-running tasks benefit from host_pinned
        if characteristics.host_count > 30 && characteristics.avg_task_duration_ms > 500 {
            return Self::HostPinned;
        }

        // Default to linear for predictability
        Self::Linear
    }

    /// Returns true if this strategy supports parallel execution.
    pub const fn is_parallel(&self) -> bool {
        match self {
            Self::Linear => true, // Parallel within each task
            Self::Free => true,
            Self::HostPinned => true,
            Self::Debug => false, // Serial execution for debugging
        }
    }

    /// Returns true if this strategy provides predictable ordering.
    pub const fn is_ordered(&self) -> bool {
        match self {
            Self::Linear => true,
            Self::Free => false,
            Self::HostPinned => false,
            Self::Debug => true,
        }
    }

    /// Returns the recommended maximum forks for this strategy.
    pub fn recommended_forks(&self, host_count: usize) -> usize {
        match self {
            Self::Linear => host_count.min(50),
            Self::Free => host_count.min(100),
            Self::HostPinned => host_count, // One worker per host
            Self::Debug => 1,               // Serial execution
        }
    }
}

// ============================================================================
// Strategy Configuration
// ============================================================================

/// Configuration options for strategy execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyConfig {
    /// The execution strategy to use.
    pub strategy: Strategy,

    /// Batch size for serial execution (only applies to Linear).
    #[serde(default)]
    pub batch_size: Option<usize>,

    /// Whether to stop on first failure.
    #[serde(default)]
    pub fail_fast: bool,

    /// Maximum concurrent hosts.
    #[serde(default)]
    pub max_concurrent: Option<usize>,

    /// Timeout per task in seconds.
    #[serde(default)]
    pub task_timeout: Option<u64>,

    /// Delay between batches in milliseconds.
    #[serde(default)]
    pub batch_delay_ms: Option<u64>,

    /// Enable verbose logging for debug strategy.
    #[serde(default)]
    pub verbose: bool,

    /// Breakpoint task names for debug strategy.
    #[serde(default)]
    pub breakpoints: Vec<String>,

    /// Custom parameters for plugin strategies.
    #[serde(default)]
    pub custom_params: HashMap<String, serde_json::Value>,
}

impl StrategyConfig {
    /// Create a new configuration with the specified strategy.
    pub fn new(strategy: Strategy) -> Self {
        Self {
            strategy,
            ..Default::default()
        }
    }

    /// Set the batch size.
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = Some(size);
        self
    }

    /// Enable fail-fast mode.
    pub fn with_fail_fast(mut self, enabled: bool) -> Self {
        self.fail_fast = enabled;
        self
    }

    /// Set maximum concurrent hosts.
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = Some(max);
        self
    }

    /// Set task timeout.
    pub fn with_task_timeout(mut self, seconds: u64) -> Self {
        self.task_timeout = Some(seconds);
        self
    }

    /// Set batch delay.
    pub fn with_batch_delay(mut self, ms: u64) -> Self {
        self.batch_delay_ms = Some(ms);
        self
    }

    /// Enable verbose output.
    pub fn with_verbose(mut self, enabled: bool) -> Self {
        self.verbose = enabled;
        self
    }

    /// Add breakpoints for debug strategy.
    pub fn with_breakpoints(mut self, breakpoints: Vec<String>) -> Self {
        self.breakpoints = breakpoints;
        self
    }

    /// Set a custom parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.custom_params.insert(key.into(), value);
        self
    }

    /// Get the effective max concurrent hosts.
    pub fn effective_max_concurrent(&self, host_count: usize) -> usize {
        self.max_concurrent
            .unwrap_or_else(|| self.strategy.recommended_forks(host_count))
    }

    /// Get the effective batch size.
    pub fn effective_batch_size(&self, host_count: usize) -> usize {
        self.batch_size.unwrap_or(host_count)
    }
}

// ============================================================================
// Workload Characteristics
// ============================================================================

/// Characteristics of a workload used for strategy selection.
#[derive(Debug, Clone, Default)]
pub struct WorkloadCharacteristics {
    /// Number of target hosts.
    pub host_count: usize,

    /// Number of tasks to execute.
    pub task_count: usize,

    /// Average task duration in milliseconds.
    pub avg_task_duration_ms: u64,

    /// Expected failure rate (0.0 to 1.0).
    pub expected_failure_rate: f64,

    /// Whether tasks have dependencies.
    pub has_dependencies: bool,

    /// Whether this is a debug/troubleshooting run.
    pub debug_mode: bool,

    /// Whether connection reuse is beneficial.
    pub benefits_from_connection_reuse: bool,

    /// Whether hosts have varying performance.
    pub heterogeneous_hosts: bool,
}

impl WorkloadCharacteristics {
    /// Create new workload characteristics.
    pub fn new(host_count: usize, task_count: usize) -> Self {
        Self {
            host_count,
            task_count,
            ..Default::default()
        }
    }

    /// Set the average task duration.
    pub fn with_avg_duration(mut self, ms: u64) -> Self {
        self.avg_task_duration_ms = ms;
        self
    }

    /// Set the expected failure rate.
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.expected_failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set whether tasks have dependencies.
    pub fn with_dependencies(mut self, has_deps: bool) -> Self {
        self.has_dependencies = has_deps;
        self
    }

    /// Enable debug mode.
    pub fn with_debug_mode(mut self, enabled: bool) -> Self {
        self.debug_mode = enabled;
        self
    }

    /// Calculate optimal batch size for serial execution.
    pub fn optimal_batch_size(&self) -> usize {
        // Start with sqrt of hosts for balanced batching
        let base = (self.host_count as f64).sqrt().ceil() as usize;

        // Adjust based on failure rate (smaller batches with higher failure rates)
        let adjusted = if self.expected_failure_rate > 0.1 {
            (base as f64 * (1.0 - self.expected_failure_rate)).ceil() as usize
        } else {
            base
        };

        // Ensure at least 1, at most host_count
        adjusted.max(1).min(self.host_count)
    }

    /// Estimate total execution time in milliseconds.
    pub fn estimate_duration_ms(&self, strategy: Strategy) -> u64 {
        let total_work =
            self.host_count as u64 * self.task_count as u64 * self.avg_task_duration_ms;

        match strategy {
            Strategy::Linear => {
                // Tasks are serialized, but hosts run in parallel per task
                self.task_count as u64 * self.avg_task_duration_ms
            }
            Strategy::Free | Strategy::HostPinned => {
                // Hosts run in parallel, estimate based on slowest host
                let parallelism = self.host_count.max(1) as u64;
                total_work / parallelism
            }
            Strategy::Debug => {
                // Serial execution with overhead
                total_work + (self.task_count as u64 * 100) // 100ms debug overhead per task
            }
        }
    }
}

// ============================================================================
// Strategy Plugin Interface
// ============================================================================

/// Result type for strategy operations.
pub type StrategyResult<T> = Result<T, StrategyError>;

/// Errors that can occur during strategy execution.
#[derive(Debug, Clone)]
pub enum StrategyError {
    /// Unknown strategy name.
    UnknownStrategy(String),

    /// Plugin initialization failed.
    InitializationFailed(String),

    /// Execution failed.
    ExecutionFailed(String),

    /// Configuration error.
    ConfigurationError(String),

    /// Plugin not found.
    PluginNotFound(String),

    /// Timeout exceeded.
    Timeout(Duration),

    /// Cancelled by user.
    Cancelled,
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownStrategy(name) => write!(f, "Unknown strategy: {}", name),
            Self::InitializationFailed(msg) => write!(f, "Strategy initialization failed: {}", msg),
            Self::ExecutionFailed(msg) => write!(f, "Strategy execution failed: {}", msg),
            Self::ConfigurationError(msg) => write!(f, "Strategy configuration error: {}", msg),
            Self::PluginNotFound(name) => write!(f, "Strategy plugin not found: {}", name),
            Self::Timeout(duration) => write!(f, "Strategy timeout after {:?}", duration),
            Self::Cancelled => write!(f, "Strategy execution cancelled"),
        }
    }
}

impl std::error::Error for StrategyError {}

/// Context provided to strategy plugins during execution.
#[derive(Debug, Clone)]
pub struct StrategyContext {
    /// Configuration for the strategy.
    pub config: StrategyConfig,

    /// Workload characteristics.
    pub workload: WorkloadCharacteristics,

    /// Start time of execution.
    pub start_time: Instant,

    /// Whether execution should be cancelled.
    pub cancelled: bool,
}

impl StrategyContext {
    /// Create a new strategy context.
    pub fn new(config: StrategyConfig, workload: WorkloadCharacteristics) -> Self {
        Self {
            config,
            workload,
            start_time: Instant::now(),
            cancelled: false,
        }
    }

    /// Get elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Check if timeout has been exceeded.
    pub fn is_timeout(&self) -> bool {
        if let Some(timeout) = self.config.task_timeout {
            self.elapsed() > Duration::from_secs(timeout)
        } else {
            false
        }
    }

    /// Cancel execution.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Check if execution is cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Trait for custom strategy plugins.
///
/// Implement this trait to create custom execution strategies with
/// specialized behavior.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::strategy::{
///     StrategyContext, StrategyOutput, StrategyPlugin, StrategyResult, TaskRunner,
/// };
/// use std::sync::Arc;
/// use async_trait::async_trait;
///
/// struct RollingUpdateStrategy {
///     update_percent: f64,
///     verify_health: bool,
/// }
///
/// #[async_trait]
/// impl StrategyPlugin for RollingUpdateStrategy {
///     fn name(&self) -> &str {
///         "rolling_update"
///     }
///
///     fn description(&self) -> &str {
///         "Rolling update with health checks"
///     }
///
///     async fn execute(
///         &self,
///         ctx: &StrategyContext,
///         hosts: &[String],
///         task_runner: Arc<dyn TaskRunner>,
///     ) -> StrategyResult<StrategyOutput> {
///         // Custom execution logic
///         Ok(StrategyOutput::default())
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait StrategyPlugin: Send + Sync {
    /// Returns the unique name of the strategy.
    fn name(&self) -> &str;

    /// Returns a description of the strategy.
    fn description(&self) -> &str;

    /// Returns the version of the plugin.
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Initialize the plugin with configuration.
    fn initialize(&mut self, _config: &StrategyConfig) -> StrategyResult<()> {
        Ok(())
    }

    /// Validate that the strategy can handle the given workload.
    fn validate(&self, _workload: &WorkloadCharacteristics) -> StrategyResult<()> {
        Ok(())
    }

    /// Execute the strategy.
    ///
    /// This is the main entry point for strategy execution.
    async fn execute(
        &self,
        ctx: &StrategyContext,
        hosts: &[String],
        task_runner: Arc<dyn TaskRunner>,
    ) -> StrategyResult<StrategyOutput>;

    /// Called when execution is cancelled.
    fn on_cancel(&self) {}

    /// Called when execution completes.
    fn on_complete(&self, _output: &StrategyOutput) {}

    /// Returns metrics collected during execution.
    fn metrics(&self) -> HashMap<String, f64> {
        HashMap::new()
    }
}

/// Interface for running tasks within a strategy.
///
/// This trait abstracts the actual task execution, allowing strategies
/// to control ordering and parallelism without knowing implementation details.
#[async_trait]
pub trait TaskRunner: Send + Sync {
    /// Run a single task on a single host.
    async fn run_task(&self, host: &str, task_index: usize) -> TaskRunResult;

    /// Run a single task on multiple hosts in parallel.
    async fn run_task_parallel(&self, hosts: &[String], task_index: usize) -> Vec<TaskRunResult>;

    /// Run all tasks on a single host sequentially.
    async fn run_host(&self, host: &str) -> HostRunResult;

    /// Get the total number of tasks.
    fn task_count(&self) -> usize;

    /// Get task name by index.
    fn task_name(&self, index: usize) -> Option<&str>;

    /// Check if a task should be skipped for a host.
    fn should_skip(&self, host: &str, task_index: usize) -> bool;
}

/// Result of running a single task.
#[derive(Debug, Clone)]
pub struct TaskRunResult {
    /// Host the task ran on.
    pub host: String,

    /// Task index.
    pub task_index: usize,

    /// Whether the task succeeded.
    pub success: bool,

    /// Whether the task made changes.
    pub changed: bool,

    /// Whether the task was skipped.
    pub skipped: bool,

    /// Duration of task execution.
    pub duration: Duration,

    /// Error message if failed.
    pub error: Option<String>,
}

impl TaskRunResult {
    /// Create a successful result.
    pub fn success(host: impl Into<String>, task_index: usize, changed: bool) -> Self {
        Self {
            host: host.into(),
            task_index,
            success: true,
            changed,
            skipped: false,
            duration: Duration::ZERO,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failed(host: impl Into<String>, task_index: usize, error: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            task_index,
            success: false,
            changed: false,
            skipped: false,
            duration: Duration::ZERO,
            error: Some(error.into()),
        }
    }

    /// Create a skipped result.
    pub fn skipped(host: impl Into<String>, task_index: usize) -> Self {
        Self {
            host: host.into(),
            task_index,
            success: true,
            changed: false,
            skipped: true,
            duration: Duration::ZERO,
            error: None,
        }
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

/// Result of running all tasks on a host.
#[derive(Debug, Clone)]
pub struct HostRunResult {
    /// Host that was executed.
    pub host: String,

    /// Individual task results.
    pub task_results: Vec<TaskRunResult>,

    /// Total duration.
    pub duration: Duration,

    /// Whether the host failed.
    pub failed: bool,

    /// Whether the host was unreachable.
    pub unreachable: bool,
}

impl HostRunResult {
    /// Create a new host result.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            task_results: Vec::new(),
            duration: Duration::ZERO,
            failed: false,
            unreachable: false,
        }
    }

    /// Add a task result.
    pub fn add_result(&mut self, result: TaskRunResult) {
        if !result.success && !result.skipped {
            self.failed = true;
        }
        self.task_results.push(result);
    }

    /// Get statistics.
    pub fn stats(&self) -> HostStats {
        let mut stats = HostStats::default();
        for result in &self.task_results {
            if result.skipped {
                stats.skipped += 1;
            } else if result.success {
                if result.changed {
                    stats.changed += 1;
                } else {
                    stats.ok += 1;
                }
            } else {
                stats.failed += 1;
            }
        }
        stats
    }
}

/// Statistics for a single host.
#[derive(Debug, Clone, Default)]
pub struct HostStats {
    /// Tasks that completed without changes.
    pub ok: usize,
    /// Tasks that made changes.
    pub changed: usize,
    /// Tasks that failed.
    pub failed: usize,
    /// Tasks that were skipped.
    pub skipped: usize,
}

impl HostStats {
    /// Merge with another stats.
    pub fn merge(&mut self, other: &HostStats) {
        self.ok += other.ok;
        self.changed += other.changed;
        self.failed += other.failed;
        self.skipped += other.skipped;
    }

    /// Total tasks.
    pub fn total(&self) -> usize {
        self.ok + self.changed + self.failed + self.skipped
    }
}

/// Output from strategy execution.
#[derive(Debug, Clone)]
pub struct StrategyOutput {
    /// Results per host.
    pub host_results: HashMap<String, HostRunResult>,

    /// Total duration.
    pub duration: Duration,

    /// Whether execution was successful.
    pub success: bool,

    /// Aggregate statistics.
    pub stats: HostStats,

    /// Custom metrics from the strategy.
    pub metrics: HashMap<String, f64>,
}

impl StrategyOutput {
    /// Create a new output.
    pub fn new() -> Self {
        Self {
            host_results: HashMap::new(),
            duration: Duration::ZERO,
            success: true,
            stats: HostStats::default(),
            metrics: HashMap::new(),
        }
    }

    /// Add a host result.
    pub fn add_host_result(&mut self, result: HostRunResult) {
        if result.failed || result.unreachable {
            self.success = false;
        }
        self.stats.merge(&result.stats());
        self.host_results.insert(result.host.clone(), result);
    }

    /// Set duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Add a metric.
    pub fn with_metric(mut self, name: impl Into<String>, value: f64) -> Self {
        self.metrics.insert(name.into(), value);
        self
    }
}

impl Default for StrategyOutput {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Strategy Registry
// ============================================================================

/// Registry for strategy plugins.
pub struct StrategyRegistry {
    plugins: HashMap<String, Arc<dyn StrategyPlugin>>,
}

impl StrategyRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Register a plugin.
    pub fn register(&mut self, plugin: Arc<dyn StrategyPlugin>) {
        self.plugins.insert(plugin.name().to_string(), plugin);
    }

    /// Get a plugin by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn StrategyPlugin>> {
        self.plugins.get(name).cloned()
    }

    /// List all registered plugins.
    pub fn list(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a plugin is registered.
    pub fn has(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }
}

impl Default for StrategyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in Strategy Plugins
// ============================================================================

/// Linear strategy plugin implementation.
pub struct LinearStrategyPlugin;

#[async_trait]
impl StrategyPlugin for LinearStrategyPlugin {
    fn name(&self) -> &str {
        "linear"
    }

    fn description(&self) -> &str {
        "Run each task on all hosts before moving to the next task"
    }

    async fn execute(
        &self,
        ctx: &StrategyContext,
        hosts: &[String],
        task_runner: Arc<dyn TaskRunner>,
    ) -> StrategyResult<StrategyOutput> {
        let start = Instant::now();
        let mut output = StrategyOutput::new();

        // Initialize host results
        for host in hosts {
            output
                .host_results
                .insert(host.clone(), HostRunResult::new(host));
        }

        let task_count = task_runner.task_count();

        // Execute each task on all hosts before moving to next task
        for task_idx in 0..task_count {
            if ctx.is_cancelled() {
                return Err(StrategyError::Cancelled);
            }

            if ctx.is_timeout() {
                return Err(StrategyError::Timeout(ctx.elapsed()));
            }

            // Run task on all active hosts in parallel
            let active_hosts: Vec<_> = hosts
                .iter()
                .filter(|h| {
                    output
                        .host_results
                        .get(*h)
                        .map(|r| !r.failed && !r.unreachable)
                        .unwrap_or(true)
                })
                .cloned()
                .collect();

            if active_hosts.is_empty() {
                break;
            }

            let results = task_runner.run_task_parallel(&active_hosts, task_idx).await;

            for result in results {
                if let Some(host_result) = output.host_results.get_mut(&result.host) {
                    host_result.add_result(result);
                }
            }

            // Check fail_fast
            if ctx.config.fail_fast {
                let any_failed = output.host_results.values().any(|r| r.failed);
                if any_failed {
                    output.success = false;
                    break;
                }
            }
        }

        // Calculate stats
        output.stats = HostStats::default();
        for result in output.host_results.values() {
            output.stats.merge(&result.stats());
        }

        output.duration = start.elapsed();
        output.success = !output
            .host_results
            .values()
            .any(|r| r.failed || r.unreachable);

        Ok(output)
    }
}

/// Free strategy plugin implementation.
pub struct FreeStrategyPlugin;

#[async_trait]
impl StrategyPlugin for FreeStrategyPlugin {
    fn name(&self) -> &str {
        "free"
    }

    fn description(&self) -> &str {
        "Each host runs independently at maximum speed"
    }

    async fn execute(
        &self,
        ctx: &StrategyContext,
        hosts: &[String],
        task_runner: Arc<dyn TaskRunner>,
    ) -> StrategyResult<StrategyOutput> {
        let start = Instant::now();
        let mut output = StrategyOutput::new();

        // Run all hosts in parallel
        let mut handles = Vec::with_capacity(hosts.len());

        for host in hosts {
            let host = host.clone();
            let runner = Arc::clone(&task_runner);
            let fail_fast = ctx.config.fail_fast;

            handles.push(tokio::spawn(async move {
                let mut host_result = HostRunResult::new(&host);
                let host_start = Instant::now();

                for task_idx in 0..runner.task_count() {
                    if fail_fast && host_result.failed {
                        break;
                    }

                    let result = runner.run_task(&host, task_idx).await;
                    host_result.add_result(result);
                }

                host_result.duration = host_start.elapsed();
                host_result
            }));
        }

        // Collect results
        for handle in handles {
            match handle.await {
                Ok(result) => {
                    output.add_host_result(result);
                }
                Err(e) => {
                    // Task panicked or was cancelled
                    output.success = false;
                    output.metrics.insert("join_errors".to_string(), 1.0);
                    tracing::error!("Host task failed: {}", e);
                }
            }
        }

        output.duration = start.elapsed();
        Ok(output)
    }
}

/// Debug strategy plugin implementation.
pub struct DebugStrategyPlugin;

#[async_trait]
impl StrategyPlugin for DebugStrategyPlugin {
    fn name(&self) -> &str {
        "debug"
    }

    fn description(&self) -> &str {
        "Step-by-step execution with detailed logging"
    }

    async fn execute(
        &self,
        ctx: &StrategyContext,
        hosts: &[String],
        task_runner: Arc<dyn TaskRunner>,
    ) -> StrategyResult<StrategyOutput> {
        let start = Instant::now();
        let mut output = StrategyOutput::new();

        // Initialize host results
        for host in hosts {
            output
                .host_results
                .insert(host.clone(), HostRunResult::new(host));
        }

        let task_count = task_runner.task_count();

        // Execute sequentially for debugging
        for task_idx in 0..task_count {
            if ctx.is_cancelled() {
                tracing::info!("[DEBUG] Execution cancelled");
                return Err(StrategyError::Cancelled);
            }

            let task_name = task_runner.task_name(task_idx).unwrap_or("unknown");
            tracing::info!(
                "[DEBUG] Task {}/{}: {}",
                task_idx + 1,
                task_count,
                task_name
            );

            // Check for breakpoints
            if ctx.config.breakpoints.contains(&task_name.to_string()) {
                tracing::info!("[DEBUG] BREAKPOINT: {}", task_name);
                // In a real implementation, this would pause execution
            }

            for host in hosts {
                if ctx.is_cancelled() {
                    return Err(StrategyError::Cancelled);
                }

                // Check if host is still active
                if output
                    .host_results
                    .get(host)
                    .map(|r| r.failed || r.unreachable)
                    .unwrap_or(false)
                {
                    tracing::debug!("[DEBUG] Skipping {} (host failed)", host);
                    continue;
                }

                tracing::debug!("[DEBUG] Running on host: {}", host);

                let result = task_runner.run_task(host, task_idx).await;

                if ctx.config.verbose {
                    tracing::info!(
                        "[DEBUG] {} - {}: success={}, changed={}, duration={:?}",
                        host,
                        task_name,
                        result.success,
                        result.changed,
                        result.duration
                    );
                }

                if let Some(host_result) = output.host_results.get_mut(host) {
                    host_result.add_result(result);
                }
            }
        }

        // Calculate stats
        output.stats = HostStats::default();
        for result in output.host_results.values() {
            output.stats.merge(&result.stats());
        }

        output.duration = start.elapsed();
        output.success = !output
            .host_results
            .values()
            .any(|r| r.failed || r.unreachable);

        tracing::info!(
            "[DEBUG] Execution complete: ok={}, changed={}, failed={}, duration={:?}",
            output.stats.ok,
            output.stats.changed,
            output.stats.failed,
            output.duration
        );

        Ok(output)
    }
}

// ============================================================================
// Benchmarking Support
// ============================================================================

/// Metrics collected during strategy execution.
#[derive(Debug, Clone, Default)]
pub struct StrategyMetrics {
    /// Total execution time.
    pub total_duration: Duration,

    /// Time spent on each task.
    pub task_durations: Vec<Duration>,

    /// Time spent on each host.
    pub host_durations: HashMap<String, Duration>,

    /// Number of parallel executions.
    pub parallel_executions: usize,

    /// Number of serial executions.
    pub serial_executions: usize,

    /// Average task duration.
    pub avg_task_duration: Duration,

    /// Maximum task duration.
    pub max_task_duration: Duration,

    /// Minimum task duration.
    pub min_task_duration: Duration,

    /// Throughput (tasks per second).
    pub throughput: f64,

    /// Efficiency ratio (actual parallel vs theoretical).
    pub efficiency: f64,
}

impl StrategyMetrics {
    /// Calculate metrics from strategy output.
    pub fn from_output(output: &StrategyOutput, host_count: usize, task_count: usize) -> Self {
        let mut metrics = Self {
            total_duration: output.duration,
            ..Default::default()
        };

        // Collect durations
        for (host, result) in &output.host_results {
            metrics.host_durations.insert(host.clone(), result.duration);

            for task_result in &result.task_results {
                metrics.task_durations.push(task_result.duration);
            }
        }

        // Calculate statistics
        if !metrics.task_durations.is_empty() {
            let sum: Duration = metrics.task_durations.iter().sum();
            metrics.avg_task_duration = sum / metrics.task_durations.len() as u32;
            metrics.max_task_duration = metrics
                .task_durations
                .iter()
                .max()
                .copied()
                .unwrap_or_default();
            metrics.min_task_duration = metrics
                .task_durations
                .iter()
                .min()
                .copied()
                .unwrap_or_default();
        }

        // Calculate throughput
        let total_tasks = host_count * task_count;
        if output.duration.as_secs_f64() > 0.0 {
            metrics.throughput = total_tasks as f64 / output.duration.as_secs_f64();
        }

        // Calculate efficiency
        let theoretical_parallel = output.duration.as_secs_f64() * host_count as f64;
        let actual_serial: f64 = metrics
            .host_durations
            .values()
            .map(|d| d.as_secs_f64())
            .sum();
        if actual_serial > 0.0 {
            metrics.efficiency = theoretical_parallel / actual_serial;
        }

        metrics
    }

    /// Format metrics as a table.
    pub fn to_table(&self) -> String {
        format!(
            "Strategy Metrics:\n\
             ├─ Total Duration: {:?}\n\
             ├─ Throughput: {:.2} tasks/sec\n\
             ├─ Efficiency: {:.1}%\n\
             ├─ Task Durations:\n\
             │  ├─ Average: {:?}\n\
             │  ├─ Maximum: {:?}\n\
             │  └─ Minimum: {:?}\n\
             └─ Host Count: {}",
            self.total_duration,
            self.throughput,
            self.efficiency * 100.0,
            self.avg_task_duration,
            self.max_task_duration,
            self.min_task_duration,
            self.host_durations.len()
        )
    }
}

/// Benchmark runner for comparing strategies.
pub struct StrategyBenchmark {
    /// Strategies to benchmark.
    strategies: Vec<Strategy>,

    /// Number of iterations per strategy.
    iterations: usize,

    /// Warmup iterations.
    warmup_iterations: usize,
}

impl StrategyBenchmark {
    /// Create a new benchmark runner.
    pub fn new() -> Self {
        Self {
            strategies: Strategy::all().to_vec(),
            iterations: 10,
            warmup_iterations: 2,
        }
    }

    /// Set strategies to benchmark.
    pub fn with_strategies(mut self, strategies: Vec<Strategy>) -> Self {
        self.strategies = strategies;
        self
    }

    /// Set number of iterations.
    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }

    /// Set warmup iterations.
    pub fn with_warmup(mut self, warmup: usize) -> Self {
        self.warmup_iterations = warmup;
        self
    }

    /// Run benchmark and return results.
    pub async fn run<F, Fut>(&self, runner: F) -> HashMap<Strategy, BenchmarkResult>
    where
        F: Fn(Strategy) -> Fut,
        Fut: std::future::Future<Output = Duration>,
    {
        let mut results = HashMap::new();

        for strategy in &self.strategies {
            let mut durations = Vec::with_capacity(self.iterations);

            // Warmup
            for _ in 0..self.warmup_iterations {
                let _ = runner(*strategy).await;
            }

            // Actual benchmark
            for _ in 0..self.iterations {
                let duration = runner(*strategy).await;
                durations.push(duration);
            }

            let result = BenchmarkResult::from_durations(*strategy, &durations);
            results.insert(*strategy, result);
        }

        results
    }
}

impl Default for StrategyBenchmark {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of benchmarking a strategy.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Strategy that was benchmarked.
    pub strategy: Strategy,

    /// Number of iterations.
    pub iterations: usize,

    /// Mean duration.
    pub mean: Duration,

    /// Median duration.
    pub median: Duration,

    /// Standard deviation.
    pub std_dev: Duration,

    /// Minimum duration.
    pub min: Duration,

    /// Maximum duration.
    pub max: Duration,

    /// All durations.
    pub durations: Vec<Duration>,
}

impl BenchmarkResult {
    /// Calculate benchmark result from durations.
    pub fn from_durations(strategy: Strategy, durations: &[Duration]) -> Self {
        let iterations = durations.len();
        let mut sorted: Vec<_> = durations.to_vec();
        sorted.sort();

        let sum: Duration = durations.iter().sum();
        let mean = sum / iterations as u32;
        let median = sorted[iterations / 2];
        let min = sorted.first().copied().unwrap_or_default();
        let max = sorted.last().copied().unwrap_or_default();

        // Calculate standard deviation
        let mean_nanos = mean.as_nanos() as f64;
        let variance: f64 = durations
            .iter()
            .map(|d| {
                let diff = d.as_nanos() as f64 - mean_nanos;
                diff * diff
            })
            .sum::<f64>()
            / iterations as f64;
        let std_dev = Duration::from_nanos(variance.sqrt() as u64);

        Self {
            strategy,
            iterations,
            mean,
            median,
            std_dev,
            min,
            max,
            durations: durations.to_vec(),
        }
    }

    /// Format result as a single line.
    pub fn to_line(&self) -> String {
        format!(
            "{}: mean={:?}, median={:?}, std_dev={:?}, min={:?}, max={:?} ({} iterations)",
            self.strategy,
            self.mean,
            self.median,
            self.std_dev,
            self.min,
            self.max,
            self.iterations
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_default() {
        assert_eq!(Strategy::default(), Strategy::Linear);
    }

    #[test]
    fn test_strategy_display() {
        assert_eq!(Strategy::Linear.to_string(), "linear");
        assert_eq!(Strategy::Free.to_string(), "free");
        assert_eq!(Strategy::HostPinned.to_string(), "host_pinned");
        assert_eq!(Strategy::Debug.to_string(), "debug");
    }

    #[test]
    fn test_strategy_from_str() {
        assert_eq!("linear".parse::<Strategy>().unwrap(), Strategy::Linear);
        assert_eq!("free".parse::<Strategy>().unwrap(), Strategy::Free);
        assert_eq!(
            "host_pinned".parse::<Strategy>().unwrap(),
            Strategy::HostPinned
        );
        assert_eq!(
            "host-pinned".parse::<Strategy>().unwrap(),
            Strategy::HostPinned
        );
        assert_eq!("debug".parse::<Strategy>().unwrap(), Strategy::Debug);
        assert!("unknown".parse::<Strategy>().is_err());
    }

    #[test]
    fn test_strategy_quick_select() {
        // Single host -> Linear
        assert_eq!(
            Strategy::quick_select_for_small_workload(1, 10),
            Some(Strategy::Linear)
        );

        // Single task -> Linear
        assert_eq!(
            Strategy::quick_select_for_small_workload(10, 1),
            Some(Strategy::Linear)
        );

        // Small workload -> Free
        assert_eq!(
            Strategy::quick_select_for_small_workload(5, 5),
            Some(Strategy::Free)
        );

        // Large workload -> None
        assert_eq!(Strategy::quick_select_for_small_workload(20, 20), None);
    }

    #[test]
    fn test_strategy_select_optimal() {
        // Single host
        let chars = WorkloadCharacteristics::new(1, 10);
        assert_eq!(Strategy::select_optimal(&chars), Strategy::Linear);

        // Debug mode
        let chars = WorkloadCharacteristics::new(10, 10).with_debug_mode(true);
        assert_eq!(Strategy::select_optimal(&chars), Strategy::Debug);

        // High failure rate
        let chars = WorkloadCharacteristics::new(10, 10).with_failure_rate(0.5);
        assert_eq!(Strategy::select_optimal(&chars), Strategy::Linear);

        // Dependencies
        let chars = WorkloadCharacteristics::new(10, 10).with_dependencies(true);
        assert_eq!(Strategy::select_optimal(&chars), Strategy::Linear);

        // Many hosts with long tasks
        let chars = WorkloadCharacteristics::new(20, 10).with_avg_duration(200);
        assert_eq!(Strategy::select_optimal(&chars), Strategy::Free);
    }

    #[test]
    fn test_strategy_config_builder() {
        let config = StrategyConfig::new(Strategy::Free)
            .with_batch_size(10)
            .with_fail_fast(true)
            .with_max_concurrent(5)
            .with_task_timeout(60)
            .with_verbose(true);

        assert_eq!(config.strategy, Strategy::Free);
        assert_eq!(config.batch_size, Some(10));
        assert!(config.fail_fast);
        assert_eq!(config.max_concurrent, Some(5));
        assert_eq!(config.task_timeout, Some(60));
        assert!(config.verbose);
    }

    #[test]
    fn test_workload_characteristics() {
        let chars = WorkloadCharacteristics::new(100, 50)
            .with_avg_duration(100)
            .with_failure_rate(0.1);

        assert_eq!(chars.host_count, 100);
        assert_eq!(chars.task_count, 50);
        assert_eq!(chars.avg_task_duration_ms, 100);
        assert!((chars.expected_failure_rate - 0.1).abs() < f64::EPSILON);

        // Test optimal batch size
        let batch_size = chars.optimal_batch_size();
        assert!(batch_size >= 1);
        assert!(batch_size <= 100);
    }

    #[test]
    fn test_host_stats_merge() {
        let mut stats1 = HostStats {
            ok: 5,
            changed: 3,
            failed: 1,
            skipped: 2,
        };

        let stats2 = HostStats {
            ok: 2,
            changed: 1,
            failed: 0,
            skipped: 1,
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.ok, 7);
        assert_eq!(stats1.changed, 4);
        assert_eq!(stats1.failed, 1);
        assert_eq!(stats1.skipped, 3);
    }

    #[test]
    fn test_task_run_result() {
        let success = TaskRunResult::success("host1", 0, true);
        assert!(success.success);
        assert!(success.changed);
        assert!(!success.skipped);

        let failed = TaskRunResult::failed("host1", 0, "error");
        assert!(!failed.success);
        assert!(failed.error.is_some());

        let skipped = TaskRunResult::skipped("host1", 0);
        assert!(skipped.success);
        assert!(skipped.skipped);
    }

    #[test]
    fn test_strategy_registry() {
        let mut registry = StrategyRegistry::new();

        assert!(!registry.has("linear"));

        registry.register(Arc::new(LinearStrategyPlugin));

        assert!(registry.has("linear"));
        assert!(registry.get("linear").is_some());
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_benchmark_result() {
        let durations = vec![
            Duration::from_millis(100),
            Duration::from_millis(110),
            Duration::from_millis(90),
            Duration::from_millis(105),
            Duration::from_millis(95),
        ];

        let result = BenchmarkResult::from_durations(Strategy::Linear, &durations);

        assert_eq!(result.strategy, Strategy::Linear);
        assert_eq!(result.iterations, 5);
        assert_eq!(result.min, Duration::from_millis(90));
        assert_eq!(result.max, Duration::from_millis(110));
    }
}
