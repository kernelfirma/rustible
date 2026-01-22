//! Callback Plugin System for Rustible Execution Events
//!
//! This module provides the callback infrastructure for receiving and
//! handling execution events during playbook runs. Callbacks can be used
//! to customize output, collect metrics, integrate with logging systems,
//! or extend Rustible's functionality.
//!
//! # Architecture
//!
//! The callback system consists of several key components:
//!
//! 1. **[`ExecutionCallback`]** trait: Core trait for receiving execution events
//! 2. **Built-in Plugins**: Extensive plugin collection in the [`plugins`] submodule
//!
//! # Available Plugins
//!
//! ## Core Output
//! - [`DefaultCallback`] - Standard Ansible-like output with colors
//! - [`MinimalCallback`] - Only failures and recap (ideal for CI/CD)
//! - [`SummaryCallback`] - Silent execution, comprehensive summary at end
//! - [`NullCallback`] - No output (useful for testing)
//!
//! ## Visual
//! - [`ProgressCallback`] - Visual progress bars
//! - [`DiffCallback`] - Before/after diffs for changed files
//! - [`DenseCallback`] - Compact output format
//! - [`OnelineCallback`] - One line per task
//! - [`TreeCallback`] - Tree-structured hierarchical output
//!
//! ## Timing & Analysis
//! - [`TimerCallback`] - Execution timing with summary
//! - [`ContextCallback`] - Task context with variables/conditions
//! - [`StatsCallback`] - Detailed statistics collection
//! - [`CounterCallback`] - Task counting and tracking
//!
//! ## Filtering
//! - [`SelectiveCallback`] - Filter by status, host, or patterns
//! - [`SkippyCallback`] - Hide skipped tasks
//! - [`ActionableCallback`] - Only changed/failed tasks
//! - [`FullSkipCallback`] - Detailed skip analysis
//!
//! ## Logging
//! - [`JsonCallback`] - JSON-formatted output
//! - [`YamlCallback`] - YAML-formatted output
//! - [`LogFileCallback`] - File-based logging
//! - [`SyslogCallback`] - System syslog integration
//! - [`DebugCallback`] - Debug output for development
//!
//! ## Integration
//! - [`JUnitCallback`] - JUnit XML reports for CI/CD
//! - [`MailCallback`] - Email notifications
//! - [`ForkedCallback`] - Parallel execution output
//!
//! # Quick Start with Prelude
//!
//! Use the [`prelude`] module for convenient imports:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::prelude::*;
//!
//! // Default Ansible-like output
//! let default = DefaultCallback::new();
//!
//! // Minimal for CI/CD
//! let minimal = MinimalCallback::new();
//!
//! // Progress bars for interactive use
//! let progress = ProgressCallback::new();
//!
//! // Combine multiple callbacks
//! let composite = CompositeCallback::new()
//!     .with_callback(Box::new(ProgressCallback::new()))
//!     .with_callback(Box::new(DiffCallback::new()));
//! # Ok(())
//! # }
//! ```
//!
//! # Creating Custom Callbacks
//!
//! Implement [`ExecutionCallback`] to create custom callbacks:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::prelude::*;
//! use std::sync::atomic::{AtomicUsize, Ordering};
//!
//! #[derive(Debug)]
//! struct MetricsCallback {
//!     task_count: AtomicUsize,
//! }
//!
//! #[async_trait]
//! impl ExecutionCallback for MetricsCallback {
//!     async fn on_task_complete(&self, result: &ExecutionResult) {
//!         self.task_count.fetch_add(1, Ordering::SeqCst);
//!         println!("Completed: {} on {} ({:?})",
//!             result.task_name,
//!             result.host,
//!             result.duration);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`ExecutionCallback`]: crate::traits::ExecutionCallback

pub mod config;
pub mod factory;
pub mod manager;
pub mod plugins;
pub mod types;

// ============================================================================
// Plugin Re-exports (Flat access for convenience)
// ============================================================================

// Core output plugins
pub use plugins::NullCallback;
pub use plugins::{
    DefaultCallback, DefaultCallbackBuilder, DefaultCallbackConfig, HostStats, Verbosity,
};
pub use plugins::{MinimalCallback, UnreachableCallback};
pub use plugins::{
    SummaryCallback, SummaryCallbackBuilder, SummaryConfig, SummaryUnreachableCallback,
};

// Visual plugins
pub use plugins::{count_changes, generate_diff, has_changes};
pub use plugins::{CompositeCallback, DiffCallback, DiffConfig};
pub use plugins::{DenseCallback, DenseConfig};
pub use plugins::{OnelineCallback, OnelineConfig};
pub use plugins::{ProgressCallback, ProgressCallbackBuilder, ProgressConfig};
pub use plugins::{
    TaskMetadata, TaskResultData, TreeCallback, TreeConfig, TreeHostStats, TreeHostSummary,
    TreePlaybookSummary, TreeUnreachableCallback,
};

// Timing & analysis plugins
pub use plugins::StatsHostStats;
pub use plugins::{
    ContextCallback, ContextCallbackBuilder, ContextCallbackConfig, ContextVerbosity,
};
pub use plugins::{CounterCallback, CounterCallbackBuilder, CounterConfig};
pub use plugins::{
    DurationHistogram, MemorySnapshot, ModuleClassification, ModuleStats, PlayStats, PlaybookStats,
    StatsCallback, StatsConfig,
};
pub use plugins::{TimerCallback, TimerCallbackBuilder, TimerConfig, TimerTaskTiming};

// Filtering plugins
pub use plugins::{ActionableCallback, ActionableConfig, ActionableUnreachableCallback};
pub use plugins::{FilterMode, SelectiveBuilder, SelectiveCallback, SelectiveConfig, StatusFilter};
pub use plugins::{FullSkipCallback, FullSkipConfig, HostSkipStats, SkipPattern, SkippedTask};
pub use plugins::{SkippyCallback, SkippyConfig};

// Logging plugins
pub use plugins::JsonHostStats;
pub use plugins::{DebugCallback, DebugConfig};
pub use plugins::{
    HostLogStats, LogEntry, LogEvent, LogFileCallback, LogFileConfig, LogFileConfigBuilder,
};
pub use plugins::{JsonCallback, JsonCallbackBuilder, JsonEvent, TaskResultJson};
pub use plugins::{
    SeverityMapping, SyslogCallback, SyslogConfig, SyslogConfigBuilder, SyslogError,
    SyslogFacility, SyslogFormat, SyslogResult, SyslogSeverity, SyslogStats,
};
pub use plugins::{YamlCallback, YamlConfig, YamlConfigBuilder};

// Integration plugins
pub use plugins::{
    ForkedCallback, ForkedCallbackBuilder, ForkedConfig, ForkedUnreachableCallback, HostState,
};
pub use plugins::{JUnitCallback, JUnitUnreachableCallback};
pub use plugins::{MailCallback, MailConfig, MailConfigBuilder, MailUnreachableCallback, TlsMode};

// ============================================================================
// Type Aliases
// ============================================================================

/// A boxed callback for dynamic dispatch.
///
/// Use this when you need to store callbacks in a collection with different types.
pub type BoxedCallback = Box<dyn crate::traits::ExecutionCallback>;

/// A shared callback wrapped in Arc for thread-safe shared ownership.
///
/// This is the recommended pattern for callbacks used across multiple tasks.
pub type SharedCallback = std::sync::Arc<dyn crate::traits::ExecutionCallback>;

// ============================================================================
// Prelude Module
// ============================================================================

/// Convenient re-exports for callback development and usage.
///
/// This prelude provides everything needed to work with the callback system:
///
/// - **Core Traits**: [`ExecutionCallback`], [`ExecutionResult`], [`ModuleResult`]
/// - **Output Plugins**: [`DefaultCallback`], [`MinimalCallback`], [`SummaryCallback`], [`NullCallback`]
/// - **Visual Plugins**: [`ProgressCallback`], [`DiffCallback`], [`CompositeCallback`]
/// - **Timing Plugins**: [`TimerCallback`], [`ContextCallback`], [`StatsCallback`]
/// - **Filtering Plugins**: [`SelectiveCallback`], [`SkippyCallback`], [`ActionableCallback`]
/// - **Logging Plugins**: [`JsonCallback`], [`YamlCallback`], [`LogFileCallback`], [`SyslogCallback`]
/// - **Integration Plugins**: [`JUnitCallback`], [`MailCallback`]
/// - **Type Aliases**: [`SharedCallback`], [`BoxedCallback`]
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::prelude::*;
///
/// // Create and configure callbacks
/// let default = DefaultCallback::new();
/// let timer = TimerCallback::summary_only();
///
/// // Combine them
/// let composite = CompositeCallback::new()
///     .with_callback(Box::new(default))
///     .with_callback(Box::new(timer));
/// # Ok(())
/// # }
/// ```
///
/// [`ExecutionCallback`]: crate::traits::ExecutionCallback
/// [`ExecutionResult`]: crate::traits::ExecutionResult
/// [`ModuleResult`]: crate::traits::ModuleResult
pub mod prelude {
    // ========================================================================
    // Core Traits
    // ========================================================================

    pub use crate::traits::ExecutionCallback;
    pub use crate::traits::ExecutionResult;
    pub use crate::traits::ModuleResult;

    // ========================================================================
    // Core Output Plugins
    // ========================================================================

    pub use super::DefaultCallback;
    pub use super::DefaultCallbackBuilder;
    pub use super::DefaultCallbackConfig;
    pub use super::HostStats;
    pub use super::MinimalCallback;
    pub use super::NullCallback;
    pub use super::SummaryCallback;
    pub use super::SummaryCallbackBuilder;
    pub use super::SummaryConfig;
    pub use super::UnreachableCallback;
    pub use super::Verbosity;

    // ========================================================================
    // Visual Plugins
    // ========================================================================

    pub use super::CompositeCallback;
    pub use super::DenseCallback;
    pub use super::DenseConfig;
    pub use super::DiffCallback;
    pub use super::DiffConfig;
    pub use super::OnelineCallback;
    pub use super::OnelineConfig;
    pub use super::ProgressCallback;
    pub use super::ProgressCallbackBuilder;
    pub use super::ProgressConfig;
    pub use super::TreeCallback;
    pub use super::TreeConfig;
    pub use super::{count_changes, generate_diff, has_changes};

    // ========================================================================
    // Timing & Analysis Plugins
    // ========================================================================

    pub use super::ContextCallback;
    pub use super::ContextCallbackBuilder;
    pub use super::ContextCallbackConfig;
    pub use super::ContextVerbosity;
    pub use super::CounterCallback;
    pub use super::CounterConfig;
    pub use super::StatsCallback;
    pub use super::StatsConfig;
    pub use super::TimerCallback;
    pub use super::TimerCallbackBuilder;
    pub use super::TimerConfig;
    pub use super::TimerTaskTiming;

    // ========================================================================
    // Filtering Plugins
    // ========================================================================

    pub use super::ActionableCallback;
    pub use super::ActionableConfig;
    pub use super::FilterMode;
    pub use super::FullSkipCallback;
    pub use super::FullSkipConfig;
    pub use super::SelectiveBuilder;
    pub use super::SelectiveCallback;
    pub use super::SelectiveConfig;
    pub use super::SkippedTask;
    pub use super::SkippyCallback;
    pub use super::SkippyConfig;
    pub use super::StatusFilter;

    // ========================================================================
    // Logging Plugins
    // ========================================================================

    pub use super::DebugCallback;
    pub use super::DebugConfig;
    pub use super::JsonCallback;
    pub use super::JsonCallbackBuilder;
    pub use super::LogFileCallback;
    pub use super::LogFileConfig;
    pub use super::SyslogCallback;
    pub use super::SyslogConfig;
    pub use super::SyslogFacility;
    pub use super::SyslogSeverity;
    pub use super::YamlCallback;
    pub use super::YamlConfig;

    // ========================================================================
    // Integration Plugins
    // ========================================================================

    pub use super::ForkedCallback;
    pub use super::ForkedConfig;
    pub use super::JUnitCallback;
    pub use super::MailCallback;
    pub use super::MailConfig;

    // ========================================================================
    // Type Aliases
    // ========================================================================

    pub use super::BoxedCallback;
    pub use super::SharedCallback;

    // ========================================================================
    // Common Dependencies
    // ========================================================================

    pub use async_trait::async_trait;
    pub use std::sync::Arc;
}
