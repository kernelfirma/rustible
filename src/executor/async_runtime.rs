//! Optimized async runtime configuration and management
//!
//! This module provides:
//! - Configurable Tokio runtime with tuned thread pools
//! - Proper task spawning patterns with structured concurrency
//! - Backpressure handling via bounded channels and semaphores
//! - Graceful shutdown with timeout support
//! - Runtime metrics collection for observability
//!
//! # Performance Optimizations
//!
//! - **Thread Pool Sizing**: I/O-bound workloads get 2x CPU threads
//! - **Work Stealing**: Enabled for better load distribution
//! - **Parking Optimization**: Tuned park timeout for responsiveness
//! - **Stack Size**: Optimized for SSH connection handling
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::executor::async_runtime::{RuntimeBuilder, RuntimeConfig};
//!
//! let config = RuntimeConfig::for_io_bound();
//! let runtime = RuntimeBuilder::new(config).build()?;
//!
//! runtime.block_on(async {
//!     // Execute async workload
//! });
//!
//! // Graceful shutdown with timeout
//! runtime.shutdown_timeout(std::time::Duration::from_secs(30));
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex as ParkingMutex;
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::{broadcast, mpsc, oneshot, Semaphore, Mutex as AsyncMutex};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

// ============================================================================
// Runtime Configuration
// ============================================================================

/// Configuration for the async runtime.
///
/// Provides presets for common workload types and fine-grained control
/// over thread pool sizing and behavior.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Number of worker threads for the multi-threaded runtime.
    /// If None, uses number of CPU cores.
    pub worker_threads: Option<usize>,

    /// Maximum number of blocking threads.
    /// Default: 512
    pub max_blocking_threads: usize,

    /// Thread stack size in bytes.
    /// Default: 2MB (optimized for SSH connections)
    pub thread_stack_size: usize,

    /// Thread name prefix for worker threads.
    pub thread_name: String,

    /// Enable I/O driver for async I/O operations.
    pub enable_io: bool,

    /// Enable time driver for async timers.
    pub enable_time: bool,

    /// Global task queue size limit for backpressure.
    /// When exceeded, new task submissions will wait.
    pub task_queue_limit: usize,

    /// Timeout for graceful shutdown.
    pub shutdown_timeout: Duration,

    /// Interval for collecting runtime metrics.
    pub metrics_interval: Duration,

    /// Enable detailed runtime metrics collection.
    pub enable_metrics: bool,

    /// Number of event loop ticks before parking.
    /// Lower values = more responsive, higher CPU.
    /// Higher values = less responsive, lower CPU.
    pub event_interval: u32,

    /// Global queue interval for work stealing.
    pub global_queue_interval: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            worker_threads: None,
            max_blocking_threads: 512,
            thread_stack_size: 2 * 1024 * 1024, // 2MB
            thread_name: "rustible-worker".to_string(),
            enable_io: true,
            enable_time: true,
            task_queue_limit: 10_000,
            shutdown_timeout: Duration::from_secs(30),
            metrics_interval: Duration::from_secs(10),
            enable_metrics: true,
            event_interval: 61,
            global_queue_interval: 31,
        }
    }
}

impl RuntimeConfig {
    /// Create configuration optimized for I/O-bound workloads (SSH, network).
    ///
    /// Uses 2x CPU threads to maximize I/O parallelism while waiting for
    /// network responses.
    pub fn for_io_bound() -> Self {
        let cpu_count = num_cpus();
        Self {
            worker_threads: Some(cpu_count * 2),
            max_blocking_threads: 512,
            thread_stack_size: 2 * 1024 * 1024,
            event_interval: 31, // More responsive for I/O
            global_queue_interval: 13,
            ..Default::default()
        }
    }

    /// Create configuration optimized for CPU-bound workloads (templating).
    ///
    /// Uses exactly 1x CPU threads to avoid oversubscription.
    pub fn for_cpu_bound() -> Self {
        let cpu_count = num_cpus();
        Self {
            worker_threads: Some(cpu_count),
            max_blocking_threads: 256,
            thread_stack_size: 4 * 1024 * 1024, // Larger for recursion
            event_interval: 127,                // Less responsive, lower overhead
            global_queue_interval: 61,
            ..Default::default()
        }
    }

    /// Create configuration optimized for mixed workloads.
    ///
    /// Balances between I/O and CPU-bound work.
    pub fn for_mixed() -> Self {
        let cpu_count = num_cpus();
        Self {
            worker_threads: Some((cpu_count * 3) / 2), // 1.5x CPUs
            max_blocking_threads: 384,
            thread_stack_size: 2 * 1024 * 1024,
            event_interval: 61,
            global_queue_interval: 31,
            ..Default::default()
        }
    }

    /// Create a minimal configuration for testing.
    pub fn for_testing() -> Self {
        Self {
            worker_threads: Some(2),
            max_blocking_threads: 4,
            thread_stack_size: 1024 * 1024,
            task_queue_limit: 100,
            shutdown_timeout: Duration::from_secs(5),
            enable_metrics: false,
            ..Default::default()
        }
    }

    /// Set the number of worker threads.
    pub fn with_worker_threads(mut self, count: usize) -> Self {
        self.worker_threads = Some(count);
        self
    }

    /// Set the shutdown timeout.
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }
}

// ============================================================================
// Runtime Metrics
// ============================================================================

/// Metrics collected from the async runtime.
#[derive(Debug, Clone)]
pub struct RuntimeMetrics {
    /// Total number of tasks spawned since runtime start.
    pub tasks_spawned: u64,

    /// Total number of tasks completed since runtime start.
    pub tasks_completed: u64,

    /// Number of currently active tasks.
    pub active_tasks: u64,

    /// Number of tasks waiting due to backpressure.
    pub pending_tasks: u64,

    /// Total number of spawn operations that waited due to backpressure.
    pub backpressure_events: u64,

    /// Average task wait time in microseconds (due to backpressure).
    pub avg_wait_time_us: u64,

    /// Maximum task wait time in microseconds.
    pub max_wait_time_us: u64,

    /// Number of graceful shutdown attempts.
    pub shutdown_attempts: u64,

    /// Number of tasks forcefully cancelled during shutdown.
    pub tasks_cancelled: u64,

    /// Timestamp when metrics were collected.
    pub collected_at: Instant,

    /// Runtime uptime in seconds.
    pub uptime_secs: u64,
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            tasks_spawned: 0,
            tasks_completed: 0,
            active_tasks: 0,
            pending_tasks: 0,
            backpressure_events: 0,
            avg_wait_time_us: 0,
            max_wait_time_us: 0,
            shutdown_attempts: 0,
            tasks_cancelled: 0,
            collected_at: Instant::now(),
            uptime_secs: 0,
        }
    }
}

impl RuntimeMetrics {
    /// Calculate the task completion rate (tasks/second).
    pub fn task_rate(&self) -> f64 {
        if self.uptime_secs == 0 {
            return 0.0;
        }
        self.tasks_completed as f64 / self.uptime_secs as f64
    }

    /// Calculate task success rate (0.0 - 1.0).
    pub fn completion_rate(&self) -> f64 {
        if self.tasks_spawned == 0 {
            return 1.0;
        }
        self.tasks_completed as f64 / self.tasks_spawned as f64
    }
}

/// Thread-safe metrics collector for the runtime.
#[derive(Debug)]
pub struct MetricsCollector {
    tasks_spawned: AtomicU64,
    tasks_completed: AtomicU64,
    active_tasks: AtomicU64,
    pending_tasks: AtomicU64,
    backpressure_events: AtomicU64,
    total_wait_time_us: AtomicU64,
    max_wait_time_us: AtomicU64,
    shutdown_attempts: AtomicU64,
    tasks_cancelled: AtomicU64,
    start_time: Instant,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            tasks_spawned: AtomicU64::new(0),
            tasks_completed: AtomicU64::new(0),
            active_tasks: AtomicU64::new(0),
            pending_tasks: AtomicU64::new(0),
            backpressure_events: AtomicU64::new(0),
            total_wait_time_us: AtomicU64::new(0),
            max_wait_time_us: AtomicU64::new(0),
            shutdown_attempts: AtomicU64::new(0),
            tasks_cancelled: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record a task spawn.
    pub fn record_spawn(&self) {
        self.tasks_spawned.fetch_add(1, Ordering::Relaxed);
        self.active_tasks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a task completion.
    pub fn record_completion(&self) {
        self.tasks_completed.fetch_add(1, Ordering::Relaxed);
        self.active_tasks.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record backpressure wait time.
    pub fn record_backpressure(&self, wait_time: Duration) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
        let wait_us = wait_time.as_micros() as u64;
        self.total_wait_time_us
            .fetch_add(wait_us, Ordering::Relaxed);

        // Update max wait time using CAS loop
        let mut current = self.max_wait_time_us.load(Ordering::Relaxed);
        while wait_us > current {
            match self.max_wait_time_us.compare_exchange_weak(
                current,
                wait_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Record pending task count change.
    pub fn set_pending(&self, count: u64) {
        self.pending_tasks.store(count, Ordering::Relaxed);
    }

    /// Record a shutdown attempt.
    pub fn record_shutdown(&self) {
        self.shutdown_attempts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cancelled tasks count.
    pub fn record_cancelled(&self, count: u64) {
        self.tasks_cancelled.fetch_add(count, Ordering::Relaxed);
    }

    /// Collect current metrics snapshot.
    pub fn collect(&self) -> RuntimeMetrics {
        let tasks_spawned = self.tasks_spawned.load(Ordering::Relaxed);
        let backpressure_events = self.backpressure_events.load(Ordering::Relaxed);
        let total_wait_time = self.total_wait_time_us.load(Ordering::Relaxed);

        let avg_wait_time_us = if backpressure_events > 0 {
            total_wait_time / backpressure_events
        } else {
            0
        };

        RuntimeMetrics {
            tasks_spawned,
            tasks_completed: self.tasks_completed.load(Ordering::Relaxed),
            active_tasks: self.active_tasks.load(Ordering::Relaxed),
            pending_tasks: self.pending_tasks.load(Ordering::Relaxed),
            backpressure_events,
            avg_wait_time_us,
            max_wait_time_us: self.max_wait_time_us.load(Ordering::Relaxed),
            shutdown_attempts: self.shutdown_attempts.load(Ordering::Relaxed),
            tasks_cancelled: self.tasks_cancelled.load(Ordering::Relaxed),
            collected_at: Instant::now(),
            uptime_secs: self.start_time.elapsed().as_secs(),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Backpressure Controller
// ============================================================================

/// Controls backpressure for task spawning.
///
/// Limits the number of concurrent tasks to prevent memory exhaustion
/// and maintain predictable performance under load.
#[derive(Debug)]
pub struct BackpressureController {
    /// Semaphore for limiting concurrent tasks.
    semaphore: Arc<Semaphore>,

    /// Maximum concurrent tasks.
    max_concurrent: usize,

    /// Current pending count for metrics.
    pending: AtomicUsize,

    /// Metrics collector reference.
    metrics: Arc<MetricsCollector>,
}

impl BackpressureController {
    /// Create a new backpressure controller.
    pub fn new(max_concurrent: usize, metrics: Arc<MetricsCollector>) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
            pending: AtomicUsize::new(0),
            metrics,
        }
    }

    /// Acquire a permit, waiting if necessary due to backpressure.
    ///
    /// Returns a guard that releases the permit when dropped.
    pub async fn acquire(&self) -> BackpressureGuard {
        let start = Instant::now();

        // Track pending count
        self.pending.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .set_pending(self.pending.load(Ordering::Relaxed) as u64);

        // Acquire permit
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("Semaphore should not be closed");

        // Update metrics
        let wait_time = start.elapsed();
        if wait_time > Duration::from_micros(100) {
            self.metrics.record_backpressure(wait_time);
        }

        self.pending.fetch_sub(1, Ordering::Relaxed);
        self.metrics
            .set_pending(self.pending.load(Ordering::Relaxed) as u64);

        BackpressureGuard {
            _permit: permit,
            metrics: Arc::clone(&self.metrics),
        }
    }

    /// Try to acquire a permit without waiting.
    ///
    /// Returns None if backpressure is active.
    pub fn try_acquire(&self) -> Option<BackpressureGuard> {
        self.semaphore
            .clone()
            .try_acquire_owned()
            .ok()
            .map(|permit| BackpressureGuard {
                _permit: permit,
                metrics: Arc::clone(&self.metrics),
            })
    }

    /// Get the number of available permits.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Check if backpressure is currently active.
    pub fn is_under_pressure(&self) -> bool {
        // Use saturating math to handle small max_concurrent values
        // Under pressure when available permits are less than 10% of max
        let threshold = self.max_concurrent / 10;
        // For small values where threshold would be 0, consider under pressure when no permits available
        if threshold == 0 {
            self.available() == 0
        } else {
            self.available() < threshold
        }
    }
}

/// Guard that releases a backpressure permit when dropped.
pub struct BackpressureGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    metrics: Arc<MetricsCollector>,
}

impl Drop for BackpressureGuard {
    fn drop(&mut self) {
        self.metrics.record_completion();
    }
}

// ============================================================================
// Graceful Shutdown
// ============================================================================

/// Manages graceful shutdown of the async runtime.
///
/// Uses a CancellationToken to signal shutdown and tracks in-flight
/// tasks to ensure clean termination.
#[derive(Debug, Clone)]
pub struct ShutdownController {
    /// Cancellation token for signaling shutdown.
    token: CancellationToken,

    /// Broadcast channel for shutdown notifications.
    shutdown_tx: broadcast::Sender<()>,

    /// Flag indicating shutdown has been requested.
    is_shutting_down: Arc<AtomicBool>,

    /// Metrics collector reference.
    metrics: Arc<MetricsCollector>,
}

impl ShutdownController {
    /// Create a new shutdown controller.
    pub fn new(metrics: Arc<MetricsCollector>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            token: CancellationToken::new(),
            shutdown_tx,
            is_shutting_down: Arc::new(AtomicBool::new(false)),
            metrics,
        }
    }

    /// Get a child token for a spawned task.
    ///
    /// The child token will be cancelled when the parent shuts down.
    pub fn child_token(&self) -> CancellationToken {
        self.token.child_token()
    }

    /// Check if shutdown has been requested.
    pub fn is_shutting_down(&self) -> bool {
        self.is_shutting_down.load(Ordering::Relaxed)
    }

    /// Subscribe to shutdown notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Signal shutdown to all tasks.
    pub fn shutdown(&self) {
        if self
            .is_shutting_down
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            info!("Initiating graceful shutdown");
            self.metrics.record_shutdown();
            self.token.cancel();
            let _ = self.shutdown_tx.send(());
        }
    }

    /// Wait for cancellation.
    pub async fn cancelled(&self) {
        self.token.cancelled().await
    }
}

// ============================================================================
// Task Spawner with Structured Concurrency
// ============================================================================

/// Result of a spawned task.
pub type TaskResult<T> = Result<T, TaskError>;

/// Error from a spawned task.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Task was cancelled during shutdown")]
    Cancelled,

    #[error("Task panicked: {0}")]
    Panicked(String),

    #[error("Task timed out after {0:?}")]
    Timeout(Duration),

    #[error("Task failed: {0}")]
    Failed(String),
}

/// Options for spawning a task.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Name for debugging/tracing.
    pub name: Option<String>,

    /// Timeout for the task.
    pub timeout: Option<Duration>,

    /// Whether to respect shutdown signals.
    pub respect_shutdown: bool,

    /// Priority hint (higher = more urgent, for future use).
    pub priority: u8,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            name: None,
            timeout: None,
            respect_shutdown: true,
            priority: 128,
        }
    }
}

impl SpawnOptions {
    /// Create options with a task name.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Set task timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set whether to respect shutdown signals.
    pub fn with_shutdown(mut self, respect: bool) -> Self {
        self.respect_shutdown = respect;
        self
    }
}

/// Spawner for tasks with structured concurrency support.
///
/// Provides:
/// - Backpressure handling
/// - Graceful shutdown support
/// - Task timeouts
/// - Metrics collection
pub struct TaskSpawner {
    /// Handle to the runtime.
    handle: Handle,

    /// Backpressure controller.
    backpressure: Arc<BackpressureController>,

    /// Shutdown controller.
    shutdown: ShutdownController,

    /// Metrics collector.
    metrics: Arc<MetricsCollector>,

    /// JoinSet for tracking spawned tasks.
    join_set: Arc<ParkingMutex<JoinSet<()>>>,
}

impl TaskSpawner {
    /// Create a new task spawner.
    pub fn new(handle: Handle, max_concurrent: usize, metrics: Arc<MetricsCollector>) -> Self {
        let backpressure = Arc::new(BackpressureController::new(
            max_concurrent,
            Arc::clone(&metrics),
        ));
        let shutdown = ShutdownController::new(Arc::clone(&metrics));

        Self {
            handle,
            backpressure,
            shutdown,
            metrics,
            join_set: Arc::new(ParkingMutex::new(JoinSet::new())),
        }
    }

    /// Spawn a task with the default options.
    ///
    /// Returns a oneshot receiver for the task result.
    pub async fn spawn<F, T>(&self, future: F) -> oneshot::Receiver<TaskResult<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.spawn_with_options(future, SpawnOptions::default())
            .await
    }

    /// Spawn a task with custom options.
    pub async fn spawn_with_options<F, T>(
        &self,
        future: F,
        options: SpawnOptions,
    ) -> oneshot::Receiver<TaskResult<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();

        // Wait for backpressure permit
        let _guard = self.backpressure.acquire().await;

        // Record spawn
        self.metrics.record_spawn();

        // Get shutdown token if respecting shutdown
        let shutdown_token = if options.respect_shutdown {
            Some(self.shutdown.child_token())
        } else {
            None
        };

        let task_name = options.name.clone();
        let timeout = options.timeout;

        // Spawn the task
        self.handle.spawn(async move {
            let result = async {
                // Apply timeout if specified
                let output = if let Some(timeout_duration) = timeout {
                    match tokio::time::timeout(timeout_duration, future).await {
                        Ok(result) => Ok(result),
                        Err(_) => Err(TaskError::Timeout(timeout_duration)),
                    }
                } else {
                    Ok(future.await)
                };

                output
            };

            // Handle shutdown cancellation
            let final_result = if let Some(token) = shutdown_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        debug!(task = ?task_name, "Task cancelled during shutdown");
                        Err(TaskError::Cancelled)
                    }
                    result = result => result,
                }
            } else {
                result.await
            };

            // Send result, ignore error if receiver dropped
            let _ = tx.send(final_result);
        });

        rx
    }

    /// Spawn a fire-and-forget task (no result tracking).
    pub async fn spawn_detached<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let _guard = self.backpressure.acquire().await;
        self.metrics.record_spawn();

        let shutdown_token = self.shutdown.child_token();
        let metrics = Arc::clone(&self.metrics);

        self.handle.spawn(async move {
            tokio::select! {
                biased;
                _ = shutdown_token.cancelled() => {
                    debug!("Detached task cancelled during shutdown");
                }
                _ = future => {}
            }
            metrics.record_completion();
        });
    }

    /// Spawn multiple tasks and collect their results.
    pub async fn spawn_batch<F, T, I>(&self, futures: I) -> Vec<TaskResult<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        I: IntoIterator<Item = F>,
    {
        let receivers: Vec<_> = {
            let mut receivers = Vec::new();
            for future in futures {
                receivers.push(self.spawn(future).await);
            }
            receivers
        };

        let mut results = Vec::with_capacity(receivers.len());
        for rx in receivers {
            match rx.await {
                Ok(result) => results.push(result),
                Err(_) => results.push(Err(TaskError::Cancelled)),
            }
        }

        results
    }

    /// Get the shutdown controller.
    pub fn shutdown_controller(&self) -> &ShutdownController {
        &self.shutdown
    }

    /// Initiate graceful shutdown.
    pub fn shutdown(&self) {
        self.shutdown.shutdown();
    }

    /// Check if shutdown has been requested.
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.is_shutting_down()
    }

    /// Get current metrics.
    pub fn metrics(&self) -> RuntimeMetrics {
        self.metrics.collect()
    }

    /// Check if backpressure is active.
    pub fn is_under_pressure(&self) -> bool {
        self.backpressure.is_under_pressure()
    }
}

// ============================================================================
// Runtime Builder
// ============================================================================

/// Builder for creating optimized async runtimes.
pub struct RuntimeBuilder {
    config: RuntimeConfig,
}

impl RuntimeBuilder {
    /// Create a new runtime builder with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }

    /// Create a builder with I/O-bound configuration.
    pub fn io_bound() -> Self {
        Self::new(RuntimeConfig::for_io_bound())
    }

    /// Create a builder with CPU-bound configuration.
    pub fn cpu_bound() -> Self {
        Self::new(RuntimeConfig::for_cpu_bound())
    }

    /// Build the Tokio runtime.
    pub fn build(self) -> std::io::Result<Runtime> {
        let mut builder = Builder::new_multi_thread();

        // Set worker threads
        if let Some(workers) = self.config.worker_threads {
            builder.worker_threads(workers);
        }

        // Set blocking threads limit
        builder.max_blocking_threads(self.config.max_blocking_threads);

        // Set thread stack size
        builder.thread_stack_size(self.config.thread_stack_size);

        // Set thread name
        let thread_name = self.config.thread_name.clone();
        builder.thread_name(thread_name);

        // Enable I/O if configured
        if self.config.enable_io {
            builder.enable_io();
        }

        // Enable time if configured
        if self.config.enable_time {
            builder.enable_time();
        }

        // Set event interval for responsiveness tuning
        builder.event_interval(self.config.event_interval);

        // Set global queue interval for work stealing
        builder.global_queue_interval(self.config.global_queue_interval);

        // Build and return
        let runtime = builder.build()?;

        info!(
            workers = ?self.config.worker_threads,
            max_blocking = self.config.max_blocking_threads,
            stack_size = self.config.thread_stack_size,
            "Tokio runtime initialized"
        );

        Ok(runtime)
    }

    /// Build the runtime and create a task spawner.
    pub fn build_with_spawner(self) -> std::io::Result<(Runtime, TaskSpawner)> {
        let task_limit = self.config.task_queue_limit;
        let runtime = self.build()?;
        let metrics = Arc::new(MetricsCollector::new());
        let spawner = TaskSpawner::new(runtime.handle().clone(), task_limit, metrics);

        Ok((runtime, spawner))
    }
}

// ============================================================================
// Bounded Channel Utilities for Backpressure
// ============================================================================

/// A work queue with backpressure support.
///
/// Uses bounded channels to prevent unbounded memory growth
/// when producers outpace consumers.
pub struct BoundedWorkQueue<T> {
    sender: mpsc::Sender<T>,
    receiver: AsyncMutex<mpsc::Receiver<T>>,
    capacity: usize,
}

impl<T> BoundedWorkQueue<T> {
    /// Create a new bounded work queue.
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        Self {
            sender,
            receiver: AsyncMutex::new(receiver),
            capacity,
        }
    }

    /// Send an item to the queue, waiting if full.
    pub async fn send(&self, item: T) -> Result<(), mpsc::error::SendError<T>> {
        self.sender.send(item).await
    }

    /// Try to send an item without waiting.
    pub fn try_send(&self, item: T) -> Result<(), mpsc::error::TrySendError<T>> {
        self.sender.try_send(item)
    }

    /// Receive an item from the queue.
    pub async fn recv(&self) -> Option<T> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }

    /// Get the queue capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if the queue is under pressure (> 80% full).
    pub fn is_under_pressure(&self) -> bool {
        self.sender.capacity() < self.capacity / 5
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get the number of available CPUs.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_defaults() {
        let config = RuntimeConfig::default();
        assert_eq!(config.max_blocking_threads, 512);
        assert_eq!(config.thread_stack_size, 2 * 1024 * 1024);
        assert!(config.enable_io);
        assert!(config.enable_time);
    }

    #[test]
    fn test_runtime_config_io_bound() {
        let config = RuntimeConfig::for_io_bound();
        let cpus = num_cpus();
        assert_eq!(config.worker_threads, Some(cpus * 2));
    }

    #[test]
    fn test_runtime_config_cpu_bound() {
        let config = RuntimeConfig::for_cpu_bound();
        let cpus = num_cpus();
        assert_eq!(config.worker_threads, Some(cpus));
    }

    #[test]
    fn test_metrics_collector() {
        let metrics = MetricsCollector::new();

        metrics.record_spawn();
        metrics.record_spawn();
        metrics.record_completion();

        let snapshot = metrics.collect();
        assert_eq!(snapshot.tasks_spawned, 2);
        assert_eq!(snapshot.tasks_completed, 1);
        assert_eq!(snapshot.active_tasks, 1);
    }

    #[test]
    fn test_metrics_backpressure() {
        let metrics = MetricsCollector::new();

        metrics.record_backpressure(Duration::from_millis(100));
        metrics.record_backpressure(Duration::from_millis(200));

        let snapshot = metrics.collect();
        assert_eq!(snapshot.backpressure_events, 2);
        assert_eq!(snapshot.max_wait_time_us, 200_000);
    }

    #[tokio::test]
    async fn test_backpressure_controller() {
        let metrics = Arc::new(MetricsCollector::new());
        let controller = BackpressureController::new(2, metrics);

        // Acquire two permits
        let _guard1 = controller.acquire().await;
        let _guard2 = controller.acquire().await;

        // Should be under pressure now
        assert!(controller.is_under_pressure());
        assert_eq!(controller.available(), 0);

        // Try acquire should fail
        assert!(controller.try_acquire().is_none());
    }

    #[tokio::test]
    async fn test_shutdown_controller() {
        let metrics = Arc::new(MetricsCollector::new());
        let controller = ShutdownController::new(metrics);

        assert!(!controller.is_shutting_down());

        controller.shutdown();

        assert!(controller.is_shutting_down());
    }

    #[test]
    fn test_runtime_builder() {
        let config = RuntimeConfig::for_testing();
        let result = RuntimeBuilder::new(config).build();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bounded_work_queue() {
        let queue: BoundedWorkQueue<i32> = BoundedWorkQueue::new(10);

        // Send some items
        queue.send(1).await.unwrap();
        queue.send(2).await.unwrap();

        // Receive items
        assert_eq!(queue.recv().await, Some(1));
        assert_eq!(queue.recv().await, Some(2));
    }

    #[test]
    fn test_task_spawner_basic() {
        let runtime = RuntimeBuilder::new(RuntimeConfig::for_testing())
            .build()
            .unwrap();
        let metrics = Arc::new(MetricsCollector::new());
        let spawner = TaskSpawner::new(runtime.handle().clone(), 100, metrics);

        runtime.block_on(async {
            let rx = spawner
                .spawn(async {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    42
                })
                .await;

            let result = rx.await.unwrap().unwrap();
            assert_eq!(result, 42);
        });
    }

    #[test]
    fn test_task_spawner_timeout() {
        let runtime = RuntimeBuilder::new(RuntimeConfig::for_testing())
            .build()
            .unwrap();
        let metrics = Arc::new(MetricsCollector::new());
        let spawner = TaskSpawner::new(runtime.handle().clone(), 100, metrics);

        runtime.block_on(async {
            let rx = spawner
                .spawn_with_options(
                    async {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        42
                    },
                    SpawnOptions::default().with_timeout(Duration::from_millis(50)),
                )
                .await;

            let result = rx.await.unwrap();
            assert!(matches!(result, Err(TaskError::Timeout(_))));
        });
    }

    #[test]
    fn test_task_spawner_shutdown() {
        let runtime = RuntimeBuilder::new(RuntimeConfig::for_testing())
            .build()
            .unwrap();
        let metrics = Arc::new(MetricsCollector::new());
        let spawner = TaskSpawner::new(runtime.handle().clone(), 100, metrics);

        runtime.block_on(async {
            // Spawn a long-running task
            let rx = spawner
                .spawn(async {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    42
                })
                .await;

            // Give task time to start
            tokio::time::sleep(Duration::from_millis(10)).await;

            // Initiate shutdown
            spawner.shutdown();

            // Task should be cancelled
            let result = rx.await.unwrap();
            assert!(matches!(result, Err(TaskError::Cancelled)));
        });
    }

    #[test]
    fn test_spawn_batch() {
        let runtime = RuntimeBuilder::new(RuntimeConfig::for_testing())
            .build()
            .unwrap();
        let metrics = Arc::new(MetricsCollector::new());
        let spawner = TaskSpawner::new(runtime.handle().clone(), 100, metrics);

        runtime.block_on(async {
            let futures: Vec<_> = (0..5)
                .map(|i| async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    i * 2
                })
                .collect();

            let results = spawner.spawn_batch(futures).await;

            assert_eq!(results.len(), 5);
            for (i, result) in results.iter().enumerate() {
                assert_eq!(result.as_ref().unwrap(), &(i * 2));
            }
        });
    }
}
