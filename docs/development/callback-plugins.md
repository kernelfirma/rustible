---
summary: Guide to implementing custom callback plugins for logging, metrics, notifications, or custom output formatting.
read_when: You need to customize execution output, integrate with external systems, or collect execution metrics.
---

# Creating Callback Plugins

This guide explains how to create custom callback plugins for Rustible. Callback plugins receive notifications about execution events and can be used for logging, metrics, notifications, or custom output formatting.

## Overview

A callback plugin in Rustible is a struct that implements the `ExecutionCallback` trait. Callbacks are notified of various lifecycle events during playbook execution:

- Playbook start/end
- Play start/end
- Task start/complete
- Handler triggers
- Fact gathering

## Built-in Callback Plugins

Rustible includes an extensive collection of built-in callback plugins:

### Core Output
| Plugin | Description |
|--------|-------------|
| `DefaultCallback` | Standard Ansible-like output with colors |
| `MinimalCallback` | Only failures and final recap (ideal for CI/CD) |
| `SummaryCallback` | Silent execution, comprehensive summary at end |
| `NullCallback` | No output (for testing) |

### Visual
| Plugin | Description |
|--------|-------------|
| `ProgressCallback` | Visual progress bars |
| `DiffCallback` | Before/after diffs for changed files |
| `DenseCallback` | Compact output format |
| `OnelineCallback` | One line per task |
| `TreeCallback` | Tree-structured hierarchical output |

### Timing & Analysis
| Plugin | Description |
|--------|-------------|
| `TimerCallback` | Execution timing with summary |
| `ContextCallback` | Task context with variables/conditions |
| `StatsCallback` | Detailed statistics collection |
| `CounterCallback` | Task counting and tracking |

### Filtering
| Plugin | Description |
|--------|-------------|
| `SelectiveCallback` | Filter by status, host, or patterns |
| `SkippyCallback` | Hide skipped tasks |
| `ActionableCallback` | Only changed/failed tasks |
| `FullSkipCallback` | Detailed skip analysis |

### Logging
| Plugin | Description |
|--------|-------------|
| `JsonCallback` | JSON-formatted output |
| `YamlCallback` | YAML-formatted output |
| `LogFileCallback` | File-based logging |
| `SyslogCallback` | System syslog integration |
| `DebugCallback` | Debug output for development |

### Integration
| Plugin | Description |
|--------|-------------|
| `JUnitCallback` | JUnit XML reports for CI/CD |
| `MailCallback` | Email notifications |
| `ForkedCallback` | Parallel execution output |

## ExecutionCallback Trait

The core callback trait is defined in `src/traits.rs`:

```rust
#[async_trait]
pub trait ExecutionCallback: Send + Sync {
    /// Called when a playbook starts.
    async fn on_playbook_start(&self, name: &str) {
        let _ = name;
    }

    /// Called when a playbook ends.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    /// Called when a play starts.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let _ = (name, hosts);
    }

    /// Called when a play ends.
    async fn on_play_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    /// Called when a task starts.
    async fn on_task_start(&self, name: &str, host: &str) {
        let _ = (name, host);
    }

    /// Called when a task completes.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let _ = result;
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        let _ = name;
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let _ = (host, facts);
    }
}
```

## ExecutionResult Structure

The `ExecutionResult` struct provides information about completed tasks:

```rust
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// The host this was executed on
    pub host: String,

    /// Name of the task
    pub task_name: String,

    /// The module result
    pub result: ModuleResult,

    /// Duration of execution
    pub duration: std::time::Duration,

    /// Handlers to notify
    pub notify: Vec<String>,
}
```

And `ModuleResult`:

```rust
#[derive(Debug, Clone)]
pub struct ModuleResult {
    /// Whether execution was successful
    pub success: bool,

    /// Whether changes were made
    pub changed: bool,

    /// Human-readable message
    pub message: String,

    /// Whether task was skipped
    pub skipped: bool,

    /// Additional output data
    pub data: Option<serde_json::Value>,

    /// Warnings generated
    pub warnings: Vec<String>,
}
```

## Creating a Simple Callback Plugin

Here's a complete example of a custom callback plugin:

```rust
// src/callback/plugins/my_callback.rs

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// A custom callback that tracks execution metrics
#[derive(Debug)]
pub struct MetricsCallback {
    /// Total tasks executed
    task_count: AtomicUsize,

    /// Failed tasks
    failed_count: AtomicUsize,

    /// Changed tasks
    changed_count: AtomicUsize,

    /// Start time
    start_time: Arc<RwLock<Option<Instant>>>,

    /// Playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
}

impl MetricsCallback {
    /// Create a new metrics callback
    pub fn new() -> Self {
        Self {
            task_count: AtomicUsize::new(0),
            failed_count: AtomicUsize::new(0),
            changed_count: AtomicUsize::new(0),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
        }
    }

    /// Get total task count
    pub fn task_count(&self) -> usize {
        self.task_count.load(Ordering::SeqCst)
    }

    /// Get failed task count
    pub fn failed_count(&self) -> usize {
        self.failed_count.load(Ordering::SeqCst)
    }

    /// Get changed task count
    pub fn changed_count(&self) -> usize {
        self.changed_count.load(Ordering::SeqCst)
    }
}

impl Default for MetricsCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MetricsCallback {
    fn clone(&self) -> Self {
        // Clone shares state for consistent metrics
        Self {
            task_count: AtomicUsize::new(self.task_count.load(Ordering::SeqCst)),
            failed_count: AtomicUsize::new(self.failed_count.load(Ordering::SeqCst)),
            changed_count: AtomicUsize::new(self.changed_count.load(Ordering::SeqCst)),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
        }
    }
}

#[async_trait]
impl ExecutionCallback for MetricsCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Record start time
        let mut start_time = self.start_time.write().await;
        *start_time = Some(Instant::now());

        // Store playbook name
        let mut playbook_name = self.playbook_name.write().await;
        *playbook_name = Some(name.to_string());

        println!(
            "\n{} Starting playbook: {}",
            "[METRICS]".bright_blue(),
            name.bold()
        );
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let start_time = self.start_time.read().await;
        let duration = start_time
            .map(|t| t.elapsed())
            .unwrap_or_default();

        let status = if success {
            "SUCCESS".green().bold()
        } else {
            "FAILED".red().bold()
        };

        println!(
            "\n{} Playbook {} completed with status: {}",
            "[METRICS]".bright_blue(),
            name.bold(),
            status
        );

        println!(
            "{} Tasks: {} total, {} changed, {} failed",
            "[METRICS]".bright_blue(),
            self.task_count().to_string().cyan(),
            self.changed_count().to_string().yellow(),
            self.failed_count().to_string().red()
        );

        println!(
            "{} Duration: {:.2}s",
            "[METRICS]".bright_blue(),
            duration.as_secs_f64()
        );
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        println!(
            "{} Play '{}' starting on {} hosts",
            "[METRICS]".bright_blue(),
            name.cyan(),
            hosts.len()
        );
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let status = if success { "completed" } else { "failed" };
        println!(
            "{} Play '{}' {}",
            "[METRICS]".bright_blue(),
            name.cyan(),
            status
        );
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        println!(
            "{} Task '{}' starting on {}",
            "[METRICS]".bright_blue().dimmed(),
            name,
            host
        );
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Increment task count
        self.task_count.fetch_add(1, Ordering::SeqCst);

        // Track result types
        if !result.result.success {
            self.failed_count.fetch_add(1, Ordering::SeqCst);
        } else if result.result.changed {
            self.changed_count.fetch_add(1, Ordering::SeqCst);
        }

        // Format status indicator
        let status = if result.result.skipped {
            "SKIPPED".cyan()
        } else if !result.result.success {
            "FAILED".red().bold()
        } else if result.result.changed {
            "CHANGED".yellow()
        } else {
            "OK".green()
        };

        println!(
            "{} Task '{}' on {}: {} ({:.2}ms)",
            "[METRICS]".bright_blue(),
            result.task_name,
            result.host,
            status,
            result.duration.as_millis()
        );
    }

    async fn on_handler_triggered(&self, name: &str) {
        println!(
            "{} Handler triggered: {}",
            "[METRICS]".bright_blue().dimmed(),
            name
        );
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        println!(
            "{} Facts gathered for {}",
            "[METRICS]".bright_blue().dimmed(),
            host
        );
    }
}
```

## Using the Callback Prelude

The callback system provides a convenient prelude for imports:

```rust
use rustible::callback::prelude::*;

// Create callbacks
let default = DefaultCallback::new();
let minimal = MinimalCallback::new();
let progress = ProgressCallback::new();

// Combine multiple callbacks
let composite = CompositeCallback::new()
    .with_callback(Box::new(ProgressCallback::new()))
    .with_callback(Box::new(DiffCallback::new()));
```

## Callback Configuration

Callbacks can be configured using `CallbackConfig`:

```rust
use rustible::callback::config::{CallbackConfig, PluginConfig};

let mut config = CallbackConfig::default();

// Enable specific plugins
config.enabled_plugins = vec!["minimal".to_string(), "timer".to_string()];

// Configure plugin options
let mut timer_config = PluginConfig::enabled();
timer_config.set_option("show_per_task", true);
config.plugins.insert("timer".to_string(), timer_config);

// Global settings
config.use_colors = true;
config.show_diff = true;
config.show_task_timing = true;
```

## Plugin Factory

Use the `PluginFactory` to create plugins by name:

```rust
use rustible::callback::factory::{PluginFactory, PluginRegistry};
use rustible::callback::config::CallbackConfig;

// Create plugin by name
let plugin = PluginFactory::create("minimal", &CallbackConfig::default())?;

// Create with custom configuration
let config = CallbackConfig::default();
let plugin = PluginFactory::create("progress", &config)?;

// List available plugins
for info in PluginFactory::available_plugins() {
    println!("{}: {}", info.name, info.description);
}
```

## Registering Custom Plugins

Register your custom plugin with the `PluginRegistry`:

```rust
use rustible::callback::factory::PluginRegistry;
use std::sync::Arc;

let mut registry = PluginRegistry::with_builtins();

// Register custom plugin
registry.register("metrics", |config| {
    Ok(Arc::new(MetricsCallback::new()) as Arc<dyn ExecutionCallback>)
});

// Create the custom plugin
let plugin = registry.create("metrics", &CallbackConfig::default())?;
```

## Extending for Unreachable Hosts

For handling unreachable hosts, implement an additional trait:

```rust
/// Trait for handling unreachable hosts
#[async_trait]
pub trait UnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl UnreachableCallback for MetricsCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        self.failed_count.fetch_add(1, Ordering::SeqCst);

        println!(
            "{} Host {} unreachable during '{}': {}",
            "[METRICS]".bright_blue(),
            host.red().bold(),
            task_name,
            error
        );
    }
}
```

## Composite Callbacks

Combine multiple callbacks together:

```rust
use rustible::callback::CompositeCallback;

let composite = CompositeCallback::new()
    .with_callback(Box::new(ProgressCallback::new()))
    .with_callback(Box::new(TimerCallback::new()))
    .with_callback(Box::new(MetricsCallback::new()));

// Use as a single callback
executor.with_callback(Box::new(composite));
```

## Type Aliases

Rustible provides convenient type aliases for callbacks:

```rust
/// A boxed callback for dynamic dispatch
pub type BoxedCallback = Box<dyn ExecutionCallback>;

/// A shared callback wrapped in Arc for thread-safe shared ownership
pub type SharedCallback = std::sync::Arc<dyn ExecutionCallback>;
```

## Best Practices

### 1. Thread Safety

Use atomic types and locks for shared state:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct SafeCallback {
    counter: AtomicUsize,
    data: Arc<RwLock<HashMap<String, String>>>,
}

#[async_trait]
impl ExecutionCallback for SafeCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Atomic increment
        self.counter.fetch_add(1, Ordering::SeqCst);

        // Lock for complex data
        let mut data = self.data.write().await;
        data.insert(result.host.clone(), result.result.message.clone());
    }
}
```

### 2. Non-blocking Output

Avoid blocking operations in callbacks:

```rust
#[async_trait]
impl ExecutionCallback for FileLogCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Use async file operations
        let log_line = format!(
            "{}: {} - {}\n",
            result.host,
            result.task_name,
            result.result.message
        );

        // Don't block - spawn a task if needed
        let file_path = self.file_path.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&file_path)
                .await
                .and_then(|mut f| async move {
                    use tokio::io::AsyncWriteExt;
                    f.write_all(log_line.as_bytes()).await
                })
                .await
            {
                eprintln!("Failed to write log: {}", e);
            }
        });
    }
}
```

### 3. Configurable Output

Make callbacks configurable:

```rust
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub show_task_details: bool,
    pub show_timing: bool,
    pub use_colors: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            show_task_details: true,
            show_timing: true,
            use_colors: true,
        }
    }
}

#[derive(Debug)]
pub struct ConfigurableMetricsCallback {
    config: MetricsConfig,
    // ... other fields
}

impl ConfigurableMetricsCallback {
    pub fn new() -> Self {
        Self::with_config(MetricsConfig::default())
    }

    pub fn with_config(config: MetricsConfig) -> Self {
        Self {
            config,
            // ... initialize other fields
        }
    }
}
```

### 4. Builder Pattern

Use builders for complex configuration:

```rust
pub struct MetricsCallbackBuilder {
    show_task_details: bool,
    show_timing: bool,
    use_colors: bool,
}

impl MetricsCallbackBuilder {
    pub fn new() -> Self {
        Self {
            show_task_details: true,
            show_timing: true,
            use_colors: true,
        }
    }

    pub fn show_task_details(mut self, show: bool) -> Self {
        self.show_task_details = show;
        self
    }

    pub fn show_timing(mut self, show: bool) -> Self {
        self.show_timing = show;
        self
    }

    pub fn use_colors(mut self, use_colors: bool) -> Self {
        self.use_colors = use_colors;
        self
    }

    pub fn build(self) -> MetricsCallback {
        MetricsCallback::with_config(MetricsConfig {
            show_task_details: self.show_task_details,
            show_timing: self.show_timing,
            use_colors: self.use_colors,
        })
    }
}
```

## Testing Callback Plugins

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_result(
        host: &str,
        task_name: &str,
        success: bool,
        changed: bool,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success,
                changed,
                message: "test message".to_string(),
                skipped: false,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_metrics_callback_counts_tasks() {
        let callback = MetricsCallback::new();

        callback.on_playbook_start("test").await;

        let ok_result = create_test_result("host1", "task1", true, false);
        callback.on_task_complete(&ok_result).await;

        let changed_result = create_test_result("host1", "task2", true, true);
        callback.on_task_complete(&changed_result).await;

        let failed_result = create_test_result("host2", "task1", false, false);
        callback.on_task_complete(&failed_result).await;

        assert_eq!(callback.task_count(), 3);
        assert_eq!(callback.changed_count(), 1);
        assert_eq!(callback.failed_count(), 1);
    }

    #[tokio::test]
    async fn test_metrics_callback_clone_shares_state() {
        let callback1 = MetricsCallback::new();
        let callback2 = callback1.clone();

        callback1.on_playbook_start("test").await;

        let result = create_test_result("host1", "task1", true, false);
        callback1.on_task_complete(&result).await;

        // Both should see the same state
        assert_eq!(callback1.task_count(), callback2.task_count());
    }

    #[tokio::test]
    async fn test_unreachable_callback() {
        let callback = MetricsCallback::new();

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        assert_eq!(callback.failed_count(), 1);
    }
}
```

## Directory Structure

Place your callback plugin in the appropriate location:

```
src/callback/
├── mod.rs           # Main callback module with re-exports
├── config.rs        # Callback configuration
├── factory.rs       # Plugin factory and registry
├── types.rs         # Event types and context structs
├── manager.rs       # Callback manager
└── plugins/
    ├── mod.rs       # Plugin module with re-exports
    ├── minimal.rs   # Minimal callback
    ├── null.rs      # Null callback
    ├── progress.rs  # Progress callback
    ├── timer.rs     # Timer callback
    └── my_callback.rs  # Your custom callback
```

## Summary

1. Implement the `ExecutionCallback` trait with event handlers
2. Use thread-safe primitives for shared state (atomic types, RwLock)
3. Make callbacks configurable with config structs or builders
4. Consider implementing `UnreachableCallback` for host failure handling
5. Register custom plugins with `PluginRegistry`
6. Use `CompositeCallback` to combine multiple callbacks
7. Write comprehensive tests for all event handlers
