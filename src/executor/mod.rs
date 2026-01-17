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
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::Playbook;
//! # let playbook = Playbook::parse(r#"- hosts: all
//! #   tasks:
//! #     - name: Ping
//! #       ping: {}
//! # "#, None)?;
//! use rustible::executor::{Executor, ExecutorConfig, ExecutionStrategy};
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

/// Work-stealing scheduler for optimal load balancing.
pub mod work_stealing;

/// Declarative resource graph model for Terraform-like workflows.
pub mod resource_graph;

mod context;
mod core;
mod dependency_graph;
mod errors;
mod handler_manager;
mod results;
mod strategies;
mod strategy;
mod task_executor;

// Re-exports for commonly used types from enhancement modules
pub use async_runtime::{RuntimeConfig, RuntimeMetrics, SpawnOptions, TaskSpawner};
pub use async_task::{AsyncConfig, AsyncJobInfo, AsyncJobStatus, AsyncTaskManager};
pub use batch_processor::{BatchConfig, BatchProcessor, BatchResult, BatchStrategy};
pub use condition::{Condition, ConditionContext, ConditionEvaluator};
pub use dependency::{
    DependencyError, DependencyGraph as AdvancedDependencyGraph, DependencyKind, DependencyNode,
};
pub use fact_pipeline::{FactPipeline, FactPipelineConfig, FactResult};
pub use host_pinned::{HostPinnedConfig, HostPinnedExecutor, HostPinnedPool};
pub use pipeline::{ExecutionPipeline, PipelineConfig, TaskOptimizationHints};
pub use playbook::{Play, Playbook};
pub use register::{FailedTaskInfo, LoopResults, RegisteredResultExt};
pub use throttle::{ThrottleConfig, ThrottleManager, ThrottleStats};
pub use work_stealing::{WorkItem, WorkStealingConfig, WorkStealingScheduler, WorkStealingStats};
pub use resource_graph::{
    AttributeChange, GraphNode, Resource, ResourceAction, ResourceGraph, ResourceGraphError,
    ResourceGraphFile, ResourceGraphResult, ResourceLifecycle, ResourceOutput, ResourcePlan,
};

pub use core::{EventCallback, ExecutionEvent, Executor, ExecutorConfig};
pub use dependency_graph::DependencyGraph;
pub use errors::{ExecutorError, ExecutorResult};
pub use results::{ExecutionStats, HostResult};
pub use strategy::ExecutionStrategy;

/// Type alias for PlaybookExecutor (same as Executor)
/// Used for API compatibility and clarity
pub type PlaybookExecutor = Executor;

/// Type alias for TaskExecutor functionality
/// In a more complex implementation, this could be a separate struct
pub type TaskExecutor = Executor;
