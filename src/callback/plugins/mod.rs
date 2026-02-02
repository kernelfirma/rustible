//! Callback plugins for customizing Rustible output.
//!
//! This module provides various callback plugins that control how
//! execution progress and results are displayed.
//!
//! # Available Plugins
//!
//! ## Core Output
//! - [`DefaultCallback`] - Standard Ansible-like output with colors
//! - [`MinimalCallback`] - Shows only failures and final recap (ideal for CI/CD)
//! - [`NullCallback`] - Silent callback that produces no output
//! - [`OnelineCallback`] - Compact single-line output for log files
//! - [`SummaryCallback`] - Summary-only output at playbook end
//!
//! ## Visual
//! - [`ProgressCallback`] - Visual progress bars for playbook execution
//! - [`DiffCallback`] - Shows before/after diffs for changed files
//! - [`DenseCallback`] - Compact output for large inventories
//! - [`TreeCallback`] - Hierarchical directory output structure
//!
//! ## Timing & Analysis
//! - [`TimerCallback`] - Execution timing with summary
//! - [`ContextCallback`] - Task context with variables/conditions
//! - [`StatsCallback`] - Comprehensive statistics collection
//! - [`CounterCallback`] - Task counting and tracking
//!
//! ## Filtering
//! - [`SkippyCallback`] - Minimizes skipped task output (ideal for large playbooks)
//! - [`SelectiveCallback`] - Filters output by status, host, or patterns
//! - [`ActionableCallback`] - Only shows changed/failed tasks
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
//! - [`NotificationCallback`] - External notifications (Slack, Email, Webhooks)
//! - [`JUnitCallback`] - JUnit XML output for CI/CD integration
//! - [`MailCallback`] - Email notifications
//! - [`ForkedCallback`] - Parallel execution output
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{MinimalCallback, SkippyCallback, DiffCallback};
//!
//! // Minimal output for CI
//! let minimal = MinimalCallback::new();
//! # let _ = ();
//!
//! // Skippy - hide skipped tasks, show only changes/failures (great for large playbooks)
//! let skippy = SkippyCallback::new();
//! # let _ = ();
//!
//! // Skippy with verbosity - show skipped task names
//! let skippy_verbose = SkippyCallback::with_verbosity(1);
//! # let _ = ();
//!
//! // Show diffs for changed files
//! let diff_callback = DiffCallback::new();
//! # let _ = ();
//!
//! // Combine callbacks with CompositeCallback
//! let composite = CompositeCallback::new()
//!     .with_callback(Box::new(MinimalCallback::new()))
//!     .with_callback(Box::new(DiffCallback::new()));
//! # Ok(())
//! # }
//! ```

// ============================================================================
// Module Declarations
// ============================================================================

// Core output plugins
pub mod default;
pub mod minimal;
mod null;
mod oneline;
mod summary;

// Visual plugins
mod dense;
pub mod diff;
mod progress;
mod tree;

// Timing & analysis plugins
mod context;
mod counter;
mod stats;
mod timer;

// Filtering plugins
mod actionable;
mod full_skip;
mod selective;
mod skippy;

// Logging plugins
mod debug;
mod json;
mod logfile;
pub mod notification;
mod syslog;
mod yaml;

// Integration plugins
mod forked;
mod junit;
pub mod logstash;
mod mail;
pub mod profile_tasks;
pub mod slack;
pub mod splunk;

// ============================================================================
// Default Callback Exports
// ============================================================================

pub use default::{
    DefaultCallback, DefaultCallbackBuilder, DefaultCallbackConfig, HostStats, Verbosity,
};

// ============================================================================
// Core Output Plugin Exports
// ============================================================================

pub use minimal::{MinimalCallback, UnreachableCallback};
pub use null::NullCallback;
pub use oneline::{OnelineCallback, OnelineConfig};
pub use summary::{
    SummaryCallback, SummaryCallbackBuilder, SummaryConfig, SummaryUnreachableCallback,
};

// ============================================================================
// Visual Plugin Exports
// ============================================================================

pub use dense::{DenseCallback, DenseConfig};
pub use diff::{
    count_changes, generate_diff, has_changes, CompositeCallback, DiffCallback, DiffConfig,
};
pub use progress::{ProgressCallback, ProgressCallbackBuilder, ProgressConfig};
pub use tree::{
    TaskMetadata, TaskResultData, TreeCallback, TreeConfig, TreeHostStats, TreeHostSummary,
    TreePlaybookSummary, TreeUnreachableCallback,
};

// ============================================================================
// Timing & Analysis Plugin Exports
// ============================================================================

pub use context::{
    ContextCallback, ContextCallbackBuilder, ContextCallbackConfig, ContextVerbosity,
};
pub use counter::{CounterCallback, CounterCallbackBuilder, CounterConfig};
pub use stats::{
    DurationHistogram, HostStats as StatsHostStats, MemorySnapshot, ModuleClassification,
    ModuleStats, PlayStats, PlaybookStats, StatsCallback, StatsConfig,
};
pub use timer::{TimerCallback, TimerCallbackBuilder, TimerConfig, TimerTaskTiming};

// ============================================================================
// Filtering Plugin Exports
// ============================================================================

pub use actionable::{ActionableCallback, ActionableConfig, ActionableUnreachableCallback};
pub use full_skip::{FullSkipCallback, FullSkipConfig, HostSkipStats, SkipPattern, SkippedTask};
pub use selective::{
    FilterMode, SelectiveBuilder, SelectiveCallback, SelectiveConfig, StatusFilter,
};
pub use skippy::{SkippyCallback, SkippyConfig};

// ============================================================================
// Logging Plugin Exports
// ============================================================================

pub use debug::{DebugCallback, DebugConfig};
pub use json::{
    HostStats as JsonHostStats, JsonCallback, JsonCallbackBuilder, JsonEvent, TaskResultJson,
};
pub use logfile::{
    HostLogStats, LogEntry, LogEvent, LogFileCallback, LogFileConfig, LogFileConfigBuilder,
};
pub use syslog::{
    SeverityMapping, SyslogCallback, SyslogConfig, SyslogConfigBuilder, SyslogError,
    SyslogFacility, SyslogFormat, SyslogResult, SyslogSeverity, SyslogStats,
};
pub use yaml::{YamlCallback, YamlConfig, YamlConfigBuilder};

// ============================================================================
// Integration Plugin Exports
// ============================================================================

pub use forked::{
    ForkedCallback, ForkedCallbackBuilder, ForkedConfig, ForkedUnreachableCallback, HostState,
};
pub use junit::JUnitCallback;
pub use junit::UnreachableCallback as JUnitUnreachableCallback;
pub use mail::{MailCallback, MailConfig, MailConfigBuilder, MailUnreachableCallback, TlsMode};
pub use notification::{
    EmailConfig, FailureDetail, HostStatsSummary, NotificationCallback, NotificationConfig,
    NotificationPayload, NotificationStatus, SlackConfig, WebhookConfig,
};

// Slack callback
pub use slack::{
    SlackCallback, SlackCallbackConfig, SlackCallbackConfigBuilder, SlackError, SlackResult,
};

// Logstash callback
pub use logstash::{
    LogstashCallback, LogstashConfig, LogstashConfigBuilder, LogstashError, LogstashProtocol,
    LogstashResult,
};

// Profile tasks callback
pub use profile_tasks::{
    AggregatedTaskTiming, HostTaskTiming, HostTiming, PerformanceRecommendation,
    ProfileTasksCallback, ProfileTasksCallbackBuilder, ProfileTasksConfig, RecommendationSeverity,
    SortOrder, TaskTiming,
};

// Splunk callback
pub use splunk::{SplunkCallback, SplunkConfig, SplunkConfigBuilder, SplunkError, SplunkResult};

// ============================================================================
// Trait Re-exports
// ============================================================================

pub use crate::traits::ExecutionCallback;
