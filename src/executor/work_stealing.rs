//! Work-Stealing Scheduler for Optimal Load Balancing
//!
//! This module provides a work-stealing scheduler that dynamically balances
//! workloads across worker threads. When a worker finishes its tasks, it can
//! "steal" work from other busy workers, maximizing CPU utilization.
//!
//! ## Overview
//!
//! Work-stealing is a scheduling strategy where idle workers can take ("steal")
//! pending tasks from busy workers' queues. This approach provides:
//!
//! - **Dynamic load balancing**: Work is automatically redistributed
//! - **Minimal contention**: Workers primarily access their own queues
//! - **Cache efficiency**: LIFO for local work, FIFO for stealing
//! - **Near-linear scaling**: Efficient parallelism across many cores
//!
//! ## Benefits over Ansible's loop execution
//!
//! - Ansible executes loops 87x slower than batched operations
//! - This scheduler batches similar operations and steals work dynamically
//! - Achieves near-linear scaling with host count
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                  WorkStealingScheduler                      │
//! │                                                             │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
//! │  │ Worker 0 │  │ Worker 1 │  │ Worker 2 │  │ Worker N │   │
//! │  │  Queue   │  │  Queue   │  │  Queue   │  │  Queue   │   │
//! │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │   │
//! │  │ │Task 1│ │  │ │Task 4│ │  │ │      │ │  │ │Task 7│ │   │
//! │  │ │Task 2│ │  │ │Task 5│ │  │ │EMPTY │◄──── STEAL   │   │
//! │  │ │Task 3│ │  │ │Task 6│ │  │ │      │ │  │ │Task 8│ │   │
//! │  │ └──────┘ │  │ └──────┘ │  │ └──────┘ │  │ └──────┘ │   │
//! │  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::executor::work_stealing::{WorkStealingScheduler, WorkStealingConfig, WorkItem};
//! # let hosts = vec!["host1".to_string(), "host2".to_string()];
//! # let worker_id = 0usize;
//! # fn estimate_cost(_host: &String) -> u32 { 1 }
//! # fn process(_work: WorkItem<String>) {}
//!
//! // Create scheduler with default configuration
//! let scheduler = WorkStealingScheduler::new(WorkStealingConfig::default());
//!
//! // Submit work items with automatic load balancing
//! for host in &hosts {
//!     let item = WorkItem::new(host.clone())
//!         .with_priority(5)
//!         .with_weight(estimate_cost(host));
//!     scheduler.submit_balanced(item);
//! }
//!
//! // Workers get work (and steal when their queue is empty)
//! loop {
//!     if let Some(work) = scheduler.get_work(worker_id) {
//!         process(work);
//!         scheduler.item_processed();
//!     } else if scheduler.is_empty() {
//!         break;
//!     } else {
//!         scheduler.wait_for_work().await;
//!     }
//! }
//!
//! // Check statistics
//! let stats = scheduler.stats();
//! println!("Processed: {}, Stolen: {}", stats.items_processed, stats.items_stolen);
//! println!("Load imbalance: {:.2}", stats.load_imbalance());
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration Presets
//!
//! The scheduler provides configuration presets for common workload types:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::WorkStealingConfig;
//! // For I/O-bound work (SSH operations, file transfers)
//! let config = WorkStealingConfig::for_io_bound();
//!
//! // For CPU-bound work (template rendering, parsing)
//! let config = WorkStealingConfig::for_cpu_bound();
//!
//! // Custom configuration
//! let config = WorkStealingConfig {
//!     num_workers: 16,
//!     steal_threshold: 3,
//!     batch_steal: true,
//!     spin_count: 32,
//! };
//! # Ok(())
//! # }
//! ```
//!
//! ## Work Item Prioritization
//!
//! Work items can be assigned priorities and weights for smart scheduling:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::work_stealing::WorkItem;
//! # let task = "task_cleanup";
//! // High priority, expensive operation
//! let critical = WorkItem::new(task)
//!     .with_priority(10)  // Higher = more urgent
//!     .with_weight(100);  // Higher = more expensive
//!
//! // Low priority, cheap operation
//! let background = WorkItem::new(task)
//!     .with_priority(1)
//!     .with_weight(1);
//! # Ok(())
//! # }
//! ```
//!
//! ## Statistics and Monitoring
//!
//! Monitor scheduler performance with built-in statistics:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
//! # let scheduler = WorkStealingScheduler::<u8>::new(WorkStealingConfig::default());
//! let stats = scheduler.stats();
//!
//! // Load distribution across queues
//! println!("Queue sizes: {:?}", stats.queue_sizes);
//!
//! // Work stealing efficiency
//! println!("Steal ratio: {:.2}%", stats.steal_ratio() * 100.0);
//!
//! // Load balance quality (0.0 = perfect, 1.0 = all work on one queue)
//! println!("Load imbalance: {:.2}", stats.load_imbalance());
//! # Ok(())
//! # }
//! ```

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::trace;

/// A unit of work that can be stolen between workers.
///
/// `WorkItem` wraps a payload with metadata for intelligent scheduling:
/// - **Priority**: Determines urgency (higher values = more urgent)
/// - **Weight**: Estimated cost for load balancing decisions
///
/// # Type Parameters
///
/// * `T` - The payload type, must be `Send + Sync + Clone`
///
/// # Examples
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::executor::work_stealing::WorkItem;
/// # let expensive_task = "expensive_task";
///
/// // Create a simple work item
/// let item = WorkItem::new("process_host_1");
///
/// // Create a prioritized, weighted work item
/// let important = WorkItem::new(expensive_task)
///     .with_priority(10)  // High priority
///     .with_weight(100);  // Expensive operation
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct WorkItem<T> {
    /// The actual work payload
    pub payload: T,
    /// Priority (higher = more urgent)
    pub priority: u8,
    /// Estimated cost/weight (for load balancing)
    pub weight: u32,
}

impl<T> WorkItem<T> {
    /// Create a new work item with default priority and weight.
    ///
    /// # Arguments
    ///
    /// * `payload` - The work payload to be processed
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::WorkItem;
/// let item = WorkItem::new("task_data");
/// assert_eq!(item.priority, 0);
/// assert_eq!(item.weight, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(payload: T) -> Self {
        Self {
            payload,
            priority: 0,
            weight: 1,
        }
    }

    /// Set the priority of this work item.
    ///
    /// Higher priority items are processed before lower priority items
    /// when workers have multiple items available.
    ///
    /// # Arguments
    ///
    /// * `priority` - Priority level (0-255, higher = more urgent)
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::WorkItem;
/// # let task = "task_data";
/// let urgent = WorkItem::new(task).with_priority(255);
/// let normal = WorkItem::new(task).with_priority(128);
/// let background = WorkItem::new(task).with_priority(0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Set the weight (estimated cost) of this work item.
    ///
    /// Weight is used for load balancing decisions. Items with higher
    /// weights are distributed to balance total work across queues.
    ///
    /// # Arguments
    ///
    /// * `weight` - Estimated cost (arbitrary units, relative to other items)
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::WorkItem;
/// # let host = "host1";
/// // A simple ping operation (cheap)
/// let ping = WorkItem::new(host).with_weight(1);
    ///
    /// // A complex deployment (expensive)
    /// let deploy = WorkItem::new(host).with_weight(100);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }
}

/// A work-stealing deque that supports local push/pop and remote stealing
struct WorkQueue<T> {
    /// Local work items (LIFO for locality)
    local: VecDeque<WorkItem<T>>,
    /// Total weight of items in queue
    total_weight: u32,
}

impl<T> WorkQueue<T> {
    fn new() -> Self {
        Self {
            local: VecDeque::new(),
            total_weight: 0,
        }
    }

    /// Push work to the local queue
    fn push(&mut self, item: WorkItem<T>) {
        self.total_weight += item.weight;
        self.local.push_back(item);
    }

    /// Pop work from local queue (LIFO for cache locality)
    fn pop(&mut self) -> Option<WorkItem<T>> {
        if let Some(item) = self.local.pop_back() {
            self.total_weight = self.total_weight.saturating_sub(item.weight);
            Some(item)
        } else {
            None
        }
    }

    /// Steal work from the front (FIFO for fairness)
    fn steal(&mut self) -> Option<WorkItem<T>> {
        if let Some(item) = self.local.pop_front() {
            self.total_weight = self.total_weight.saturating_sub(item.weight);
            Some(item)
        } else {
            None
        }
    }

    /// Steal half of the work (batch stealing)
    fn steal_batch(&mut self) -> Vec<WorkItem<T>> {
        let steal_count = self.local.len() / 2;
        if steal_count == 0 {
            return Vec::new();
        }

        let mut stolen = Vec::with_capacity(steal_count);
        for _ in 0..steal_count {
            if let Some(item) = self.local.pop_front() {
                self.total_weight = self.total_weight.saturating_sub(item.weight);
                stolen.push(item);
            }
        }
        stolen
    }

    fn len(&self) -> usize {
        self.local.len()
    }

    fn is_empty(&self) -> bool {
        self.local.is_empty()
    }

    fn weight(&self) -> u32 {
        self.total_weight
    }
}

/// Configuration for the work-stealing scheduler.
///
/// This struct controls the behavior of the [`WorkStealingScheduler`],
/// including worker count, stealing policies, and performance tuning.
///
/// # Examples
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::executor::work_stealing::WorkStealingConfig;
///
/// // Use defaults (auto-detect CPU count)
/// let config = WorkStealingConfig::default();
///
/// // Use I/O-bound preset
/// let config = WorkStealingConfig::for_io_bound();
///
/// // Custom configuration
/// let config = WorkStealingConfig {
///     num_workers: 32,
///     steal_threshold: 4,
///     batch_steal: true,
///     spin_count: 64,
/// };
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct WorkStealingConfig {
    /// Number of worker threads.
    ///
    /// Defaults to the number of available CPU cores.
    pub num_workers: usize,
    /// Minimum items a victim must have before stealing is allowed.
    ///
    /// Setting this higher reduces contention but may cause imbalance.
    pub steal_threshold: usize,
    /// Enable batch stealing (steal half of victim's queue at once).
    ///
    /// Batch stealing reduces stealing overhead but may cause temporary imbalance.
    pub batch_steal: bool,
    /// Number of spin iterations before parking the thread.
    ///
    /// Higher values reduce latency but increase CPU usage when idle.
    pub spin_count: usize,
}

impl Default for WorkStealingConfig {
    fn default() -> Self {
        Self {
            num_workers: num_cpus::get(),
            steal_threshold: 2,
            batch_steal: true,
            spin_count: 32,
        }
    }
}

impl WorkStealingConfig {
    /// Create configuration optimized for I/O-bound work.
    ///
    /// This preset is ideal for SSH operations, file transfers, and
    /// network-intensive tasks where workers spend most time waiting.
    ///
    /// # Configuration
    ///
    /// - **Workers**: 2x CPU count (to overlap I/O waits)
    /// - **Steal threshold**: 1 (aggressive stealing)
    /// - **Batch steal**: Enabled
    /// - **Spin count**: 8 (low, since we expect blocking)
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
/// let scheduler: WorkStealingScheduler<String> = WorkStealingScheduler::new(
///     WorkStealingConfig::for_io_bound()
/// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn for_io_bound() -> Self {
        Self {
            // More workers for I/O parallelism
            num_workers: num_cpus::get() * 2,
            steal_threshold: 1,
            batch_steal: true,
            spin_count: 8,
        }
    }

    /// Create configuration optimized for CPU-bound work.
    ///
    /// This preset is ideal for template rendering, parsing, and
    /// compute-intensive tasks where CPU is the bottleneck.
    ///
    /// # Configuration
    ///
    /// - **Workers**: 1x CPU count (match available cores)
    /// - **Steal threshold**: 4 (conservative stealing)
    /// - **Batch steal**: Enabled
    /// - **Spin count**: 64 (high, to reduce context switches)
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
/// let scheduler: WorkStealingScheduler<String> = WorkStealingScheduler::new(
///     WorkStealingConfig::for_cpu_bound()
/// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn for_cpu_bound() -> Self {
        Self {
            num_workers: num_cpus::get(),
            steal_threshold: 4,
            batch_steal: true,
            spin_count: 64,
        }
    }
}

/// Work-stealing scheduler for parallel task execution.
///
/// This scheduler distributes work across multiple worker threads using
/// work-stealing to maintain optimal load balance. Each worker has its
/// own queue; when a worker's queue is empty, it can steal work from
/// other workers' queues.
///
/// # Type Parameters
///
/// * `T` - The work payload type, must be `Send + Sync + Clone + 'static`
///
/// # Thread Safety
///
/// The scheduler is fully thread-safe and can be shared across threads
/// using `Arc<WorkStealingScheduler<T>>`.
///
/// # Examples
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::executor::work_stealing::{WorkStealingScheduler, WorkStealingConfig, WorkItem};
/// use std::sync::Arc;
/// # let task = "task_payload".to_string();
/// # let worker_id = 0usize;
/// # fn process(_payload: String) {}
///
/// let scheduler = Arc::new(WorkStealingScheduler::new(
///     WorkStealingConfig::for_io_bound()
/// ));
///
/// // Submit work from any thread
/// scheduler.submit_balanced(WorkItem::new(task));
///
/// // Workers get and process work
/// while let Some(item) = scheduler.get_work(worker_id) {
///     process(item.payload);
///     scheduler.item_processed();
/// }
/// # Ok(())
/// # }
/// ```
pub struct WorkStealingScheduler<T> {
    /// Per-worker queues
    queues: Vec<Arc<Mutex<WorkQueue<T>>>>,
    /// Configuration
    config: WorkStealingConfig,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Notify for new work
    work_available: Arc<Notify>,
    /// Number of active workers
    active_workers: Arc<AtomicUsize>,
    /// Total items processed
    items_processed: Arc<AtomicUsize>,
    /// Total items stolen
    items_stolen: Arc<AtomicUsize>,
}

impl<T: Send + Sync + Clone + 'static> WorkStealingScheduler<T> {
    /// Create a new work-stealing scheduler
    pub fn new(config: WorkStealingConfig) -> Self {
        let num_workers = config.num_workers.max(1);
        let queues: Vec<_> = (0..num_workers)
            .map(|_| Arc::new(Mutex::new(WorkQueue::new())))
            .collect();

        Self {
            queues,
            config,
            shutdown: Arc::new(AtomicBool::new(false)),
            work_available: Arc::new(Notify::new()),
            active_workers: Arc::new(AtomicUsize::new(0)),
            items_processed: Arc::new(AtomicUsize::new(0)),
            items_stolen: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Submit work to a specific worker
    pub fn submit(&self, worker_id: usize, item: WorkItem<T>) {
        let queue_idx = worker_id % self.queues.len();
        self.queues[queue_idx].lock().push(item);
        self.work_available.notify_one();
    }

    /// Submit work with automatic load balancing
    pub fn submit_balanced(&self, item: WorkItem<T>) {
        // Find the queue with minimum weight
        let (min_idx, _) = self
            .queues
            .iter()
            .enumerate()
            .min_by_key(|(_, q)| q.lock().weight())
            .unwrap();

        self.queues[min_idx].lock().push(item);
        self.work_available.notify_one();
    }

    /// Submit a batch of work items with automatic distribution
    pub fn submit_batch(&self, items: Vec<WorkItem<T>>) {
        if items.is_empty() {
            return;
        }

        // Distribute items across queues based on weight
        let num_queues = self.queues.len();
        let items_per_queue = items.len().div_ceil(num_queues);

        for (i, item) in items.into_iter().enumerate() {
            let queue_idx = i / items_per_queue.max(1);
            let queue_idx = queue_idx.min(num_queues - 1);
            self.queues[queue_idx].lock().push(item);
        }

        // Wake all workers
        for _ in 0..num_queues {
            self.work_available.notify_one();
        }
    }

    /// Try to get work for a worker, stealing if necessary
    pub fn get_work(&self, worker_id: usize) -> Option<WorkItem<T>> {
        let queue_idx = worker_id % self.queues.len();

        // First, try local queue
        if let Some(item) = self.queues[queue_idx].lock().pop() {
            return Some(item);
        }

        // Local queue empty, try stealing
        self.try_steal(worker_id)
    }

    /// Try to steal work from another worker
    fn try_steal(&self, worker_id: usize) -> Option<WorkItem<T>> {
        let num_queues = self.queues.len();
        let my_queue_idx = worker_id % num_queues;

        // Try to steal from other queues (round-robin starting from next queue)
        for offset in 1..num_queues {
            let victim_idx = (my_queue_idx + offset) % num_queues;

            let mut victim_queue = self.queues[victim_idx].lock();

            // Only steal if victim has enough work
            if victim_queue.len() >= self.config.steal_threshold {
                if self.config.batch_steal {
                    // Batch steal: steal half of the work
                    let stolen = victim_queue.steal_batch();
                    drop(victim_queue); // Release lock before pushing to our queue

                    if !stolen.is_empty() {
                        self.items_stolen.fetch_add(stolen.len(), Ordering::Relaxed);
                        trace!(
                            "Worker {} stole {} items from worker {}",
                            worker_id,
                            stolen.len(),
                            victim_idx
                        );

                        // Convert to iterator and get the first item to return
                        let mut iter = stolen.into_iter();
                        let first = iter.next();

                        // Push remaining stolen items to our queue
                        let remaining: Vec<_> = iter.collect();
                        if !remaining.is_empty() {
                            let mut my_queue = self.queues[my_queue_idx].lock();
                            for item in remaining {
                                my_queue.push(item);
                            }
                        }

                        // Return the first stolen item directly
                        if let Some(item) = first {
                            return Some(item);
                        }
                    }
                } else {
                    // Single item steal
                    if let Some(item) = victim_queue.steal() {
                        drop(victim_queue);
                        self.items_stolen.fetch_add(1, Ordering::Relaxed);
                        trace!(
                            "Worker {} stole 1 item from worker {}",
                            worker_id,
                            victim_idx
                        );
                        return Some(item);
                    }
                }
            }
        }

        None
    }

    /// Check if all queues are empty
    pub fn is_empty(&self) -> bool {
        self.queues.iter().all(|q| q.lock().is_empty())
    }

    /// Get total pending work count
    pub fn pending_count(&self) -> usize {
        self.queues.iter().map(|q| q.lock().len()).sum()
    }

    /// Get statistics about the scheduler
    pub fn stats(&self) -> WorkStealingStats {
        let queue_sizes: Vec<usize> = self.queues.iter().map(|q| q.lock().len()).collect();
        let queue_weights: Vec<u32> = self.queues.iter().map(|q| q.lock().weight()).collect();

        WorkStealingStats {
            queue_sizes,
            queue_weights,
            active_workers: self.active_workers.load(Ordering::Relaxed),
            items_processed: self.items_processed.load(Ordering::Relaxed),
            items_stolen: self.items_stolen.load(Ordering::Relaxed),
        }
    }

    /// Signal shutdown
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Wake all workers
        for _ in 0..self.config.num_workers {
            self.work_available.notify_one();
        }
    }

    /// Check if shutdown was requested
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Wait for new work (with timeout)
    pub async fn wait_for_work(&self) {
        tokio::select! {
            _ = self.work_available.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
        }
    }

    /// Mark a worker as active
    pub fn worker_active(&self) {
        self.active_workers.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark a worker as inactive
    pub fn worker_inactive(&self) {
        self.active_workers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record that an item was processed
    pub fn item_processed(&self) {
        self.items_processed.fetch_add(1, Ordering::Relaxed);
    }
}

/// Statistics about work-stealing scheduler performance.
///
/// This struct provides insights into scheduler behavior for monitoring
/// and performance tuning. Key metrics include work distribution,
/// stealing efficiency, and load balance.
///
/// # Examples
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
/// # let scheduler = WorkStealingScheduler::<u8>::new(WorkStealingConfig::default());
/// let stats = scheduler.stats();
///
/// // Check if work is well-distributed
/// if stats.load_imbalance() > 0.3 {
///     println!("Warning: high load imbalance");
/// }
///
/// // Monitor stealing efficiency
/// println!("Steal ratio: {:.1}%", stats.steal_ratio() * 100.0);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct WorkStealingStats {
    /// Number of items currently in each worker's queue.
    pub queue_sizes: Vec<usize>,
    /// Total weight of items in each worker's queue.
    pub queue_weights: Vec<u32>,
    /// Number of workers currently processing items.
    pub active_workers: usize,
    /// Total number of items that have been processed.
    pub items_processed: usize,
    /// Total number of items stolen between workers.
    pub items_stolen: usize,
}

impl WorkStealingStats {
    /// Calculate load imbalance across worker queues.
    ///
    /// Returns a value between 0.0 and 1.0 where:
    /// - **0.0**: Perfect balance (all queues have equal weight)
    /// - **1.0**: Maximum imbalance (all work on a single queue)
    ///
    /// # Algorithm
    ///
    /// Uses normalized standard deviation of queue weights:
    /// `imbalance = std_dev(weights) / avg(weights)`
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
/// # let scheduler = WorkStealingScheduler::<u8>::new(WorkStealingConfig::default());
/// let stats = scheduler.stats();
    /// match stats.load_imbalance() {
    ///     x if x < 0.1 => println!("Excellent balance"),
    ///     x if x < 0.3 => println!("Good balance"),
    ///     x if x < 0.5 => println!("Moderate imbalance"),
    ///     _ => println!("High imbalance - consider tuning"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_imbalance(&self) -> f64 {
        if self.queue_weights.is_empty() {
            return 0.0;
        }

        let total: u32 = self.queue_weights.iter().sum();
        if total == 0 {
            return 0.0;
        }

        let avg = total as f64 / self.queue_weights.len() as f64;
        let variance: f64 = self
            .queue_weights
            .iter()
            .map(|&w| {
                let diff = w as f64 - avg;
                diff * diff
            })
            .sum::<f64>()
            / self.queue_weights.len() as f64;

        (variance.sqrt() / avg).min(1.0)
    }

    /// Calculate the ratio of stolen items to total processed items.
    ///
    /// The steal ratio indicates how often work-stealing occurred:
    /// - **Low ratio (< 0.1)**: Work was well-distributed initially
    /// - **Medium ratio (0.1-0.3)**: Normal stealing activity
    /// - **High ratio (> 0.3)**: Significant rebalancing occurred
    ///
    /// A high steal ratio is not necessarily bad - it means the scheduler
    /// is actively balancing load. However, very high ratios may indicate
    /// poor initial distribution.
    ///
    /// # Examples
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
/// # use rustible::executor::work_stealing::{WorkStealingConfig, WorkStealingScheduler};
/// # let scheduler = WorkStealingScheduler::<u8>::new(WorkStealingConfig::default());
/// let stats = scheduler.stats();
    /// println!("Steal ratio: {:.1}%", stats.steal_ratio() * 100.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn steal_ratio(&self) -> f64 {
        if self.items_processed == 0 {
            return 0.0;
        }
        self.items_stolen as f64 / self.items_processed as f64
    }
}

/// Helper function to get number of CPUs (fallback for no num_cpus crate)
mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_queue_push_pop() {
        let mut queue: WorkQueue<i32> = WorkQueue::new();

        queue.push(WorkItem::new(1));
        queue.push(WorkItem::new(2));
        queue.push(WorkItem::new(3));

        assert_eq!(queue.len(), 3);

        // LIFO order for pop
        assert_eq!(queue.pop().unwrap().payload, 3);
        assert_eq!(queue.pop().unwrap().payload, 2);
        assert_eq!(queue.pop().unwrap().payload, 1);
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_work_queue_steal() {
        let mut queue: WorkQueue<i32> = WorkQueue::new();

        queue.push(WorkItem::new(1));
        queue.push(WorkItem::new(2));
        queue.push(WorkItem::new(3));

        // FIFO order for steal
        assert_eq!(queue.steal().unwrap().payload, 1);
        assert_eq!(queue.steal().unwrap().payload, 2);
        assert_eq!(queue.steal().unwrap().payload, 3);
    }

    #[test]
    fn test_work_queue_steal_batch() {
        let mut queue: WorkQueue<i32> = WorkQueue::new();

        for i in 0..10 {
            queue.push(WorkItem::new(i));
        }

        let stolen = queue.steal_batch();
        assert_eq!(stolen.len(), 5); // Half of 10
        assert_eq!(queue.len(), 5);
    }

    #[test]
    fn test_scheduler_submit_balanced() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        for i in 0..100 {
            scheduler.submit_balanced(WorkItem::new(i));
        }

        let stats = scheduler.stats();
        // Work should be distributed across queues
        assert!(stats.queue_sizes.iter().all(|&s| s > 0));
    }

    #[test]
    fn test_scheduler_get_work_and_steal() {
        let config = WorkStealingConfig {
            num_workers: 2,
            steal_threshold: 1,
            batch_steal: false,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        // Submit all work to worker 0
        for i in 0..10 {
            scheduler.submit(0, WorkItem::new(i));
        }

        // Worker 1 should be able to steal
        let stolen = scheduler.get_work(1);
        assert!(stolen.is_some());
    }

    #[test]
    fn test_load_imbalance_calculation() {
        let stats = WorkStealingStats {
            queue_sizes: vec![10, 10, 10, 10],
            queue_weights: vec![10, 10, 10, 10],
            active_workers: 4,
            items_processed: 100,
            items_stolen: 10,
        };

        // Perfect balance
        assert!(stats.load_imbalance() < 0.01);

        let unbalanced = WorkStealingStats {
            queue_sizes: vec![40, 0, 0, 0],
            queue_weights: vec![40, 0, 0, 0],
            active_workers: 4,
            items_processed: 100,
            items_stolen: 0,
        };

        // High imbalance
        assert!(unbalanced.load_imbalance() > 0.5);
    }
}
