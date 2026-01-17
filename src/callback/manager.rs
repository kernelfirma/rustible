//! Callback Manager for Rustible's Plugin System
//!
//! This module provides the [`CallbackManager`] which orchestrates multiple callback
//! plugins during playbook execution. It supports:
//!
//! - Plugin priorities (stdout plugins run first)
//! - Async event dispatch to all registered plugins
//! - Graceful error handling (one plugin failure doesn't stop others)
//! - Runtime plugin registration/deregistration
//! - Thread-safe concurrent execution
//!
//! # Architecture
//!
//! ```text
//! +---------------------------------------------------------------------+
//! |                       CallbackManager                                |
//! |                                                                      |
//! |  +-------------------------------------------------------------+    |
//! |  |                    Plugin Registry                           |    |
//! |  |  +----------+  +----------+  +----------+  +----------+     |    |
//! |  |  | stdout   |  |  json    |  |  timer   |  |  custom  |     |    |
//! |  |  | priority |  | priority |  | priority |  | priority |     |    |
//! |  |  |   100    |  |   200    |  |   300    |  |   500    |     |    |
//! |  |  +----------+  +----------+  +----------+  +----------+     |    |
//! |  +-------------------------------------------------------------+    |
//! |                              |                                       |
//! |                              v                                       |
//! |  +-------------------------------------------------------------+    |
//! |  |               Event Dispatcher (async)                       |    |
//! |  |                                                              |    |
//! |  |  - Sorts plugins by priority                                 |    |
//! |  |  - Dispatches events to all plugins                          |    |
//! |  |  - Captures errors without stopping pipeline                 |    |
//! |  |  - Aggregates results and error reports                      |    |
//! |  +-------------------------------------------------------------+    |
//! |                              |                                       |
//! |                              v                                       |
//! |  +-------------------------------------------------------------+    |
//! |  |                  Error Handler                               |    |
//! |  |  - Logs plugin errors                                        |    |
//! |  |  - Continues with remaining plugins                          |    |
//! |  |  - Provides error aggregation                                |    |
//! |  +-------------------------------------------------------------+    |
//! +---------------------------------------------------------------------+
//! ```
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::manager::{CallbackManager, PluginPriority};
//! use rustible::callback::prelude::*;
//! use std::sync::Arc;
//!
//! // Create manager
//! let manager = CallbackManager::new();
//!
//! // Register plugins with priorities
//! manager
//!     .register("stdout", Arc::new(DefaultCallback::new()), PluginPriority::STDOUT)
//!     .await;
//! manager
//!     .register("json", Arc::new(JsonCallback::new()), PluginPriority::LOGGING)
//!     .await;
//!
//! // Dispatch events
//! manager.on_playbook_start("my_playbook").await;
//! # let _ = ();
//! manager.on_playbook_end("my_playbook", true).await;
//! # Ok(())
//! # }
//! ```

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tracing::{debug, error, trace, warn};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Plugin Priority System
// ============================================================================

/// Priority levels for callback plugins.
///
/// Lower values execute first. The priority system ensures that critical
/// output plugins (like stdout) run before logging or analytics plugins.
///
/// # Standard Priority Levels
///
/// | Level | Value | Use Case |
/// |-------|-------|----------|
/// | `STDOUT` | 100 | Console output (runs first) |
/// | `LOGGING` | 200 | File and structured logging |
/// | `NORMAL` | 500 | General purpose plugins |
/// | `METRICS` | 700 | Analytics and metrics |
/// | `CLEANUP` | 900 | Finalization tasks |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginPriority(pub i32);

impl PluginPriority {
    /// Highest priority - stdout/stderr output plugins.
    /// These should run first to provide immediate user feedback.
    pub const STDOUT: Self = Self(100);

    /// High priority - essential logging plugins.
    pub const LOGGING: Self = Self(200);

    /// Normal priority - default for most plugins.
    pub const NORMAL: Self = Self(500);

    /// Low priority - metrics and analytics plugins.
    pub const METRICS: Self = Self(700);

    /// Lowest priority - cleanup and finalization plugins.
    pub const CLEANUP: Self = Self(900);

    /// Create a custom priority level.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// # use rustible::callback::manager::PluginPriority;
    /// // Create a priority between LOGGING and NORMAL
    /// let priority = PluginPriority::custom(350);
    /// # Ok(())
    /// # }
    /// ```
    pub const fn custom(value: i32) -> Self {
        Self(value)
    }

    /// Returns the numeric priority value.
    pub const fn value(&self) -> i32 {
        self.0
    }
}

impl Default for PluginPriority {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl PartialOrd for PluginPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PluginPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower values have higher priority (run first)
        self.0.cmp(&other.0)
    }
}

// ============================================================================
// Plugin Error Handling
// ============================================================================

/// Error information from a plugin execution.
///
/// When a plugin fails during event dispatch, this struct captures
/// the context of the failure without stopping other plugins.
#[derive(Debug, Clone)]
pub struct PluginError {
    /// Name of the plugin that failed
    pub plugin_name: String,
    /// The event that triggered the error
    pub event: String,
    /// Error message
    pub message: String,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Plugin '{}' failed on event '{}': {}",
            self.plugin_name, self.event, self.message
        )
    }
}

impl std::error::Error for PluginError {}

/// Result of dispatching an event to all plugins.
///
/// This provides insight into how the dispatch went, including
/// success counts, skipped plugins, and any errors.
#[derive(Debug, Default)]
pub struct DispatchResult {
    /// Number of plugins that successfully handled the event
    pub success_count: usize,
    /// Number of plugins that were skipped (disabled)
    pub skipped_count: usize,
    /// Errors from plugins that failed
    pub errors: Vec<PluginError>,
}

impl DispatchResult {
    /// Returns true if all plugins succeeded (no errors).
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if any plugins failed.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns the total number of plugins that received the event.
    pub fn total_dispatched(&self) -> usize {
        self.success_count + self.errors.len()
    }

    /// Returns the error count.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

// ============================================================================
// Plugin Registration Entry
// ============================================================================

/// Internal registration entry for a plugin.
struct PluginEntry {
    /// The callback plugin
    plugin: Arc<dyn ExecutionCallback>,
    /// Plugin priority for ordering
    priority: PluginPriority,
    /// Whether this plugin is enabled
    enabled: bool,
}

impl std::fmt::Debug for PluginEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginEntry")
            .field("plugin", &"<ExecutionCallback>")
            .field("priority", &self.priority)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl PluginEntry {
    fn new(plugin: Arc<dyn ExecutionCallback>, priority: PluginPriority) -> Self {
        Self {
            plugin,
            priority,
            enabled: true,
        }
    }
}

// ============================================================================
// Callback Manager
// ============================================================================

/// Thread-safe manager for callback plugins.
///
/// The `CallbackManager` provides:
///
/// - **Plugin Registration**: Add/remove plugins at runtime
/// - **Priority Ordering**: Plugins execute in priority order
/// - **Error Isolation**: One plugin failure doesn't stop others
/// - **Thread Safety**: Safe for concurrent access via `RwLock`
/// - **Enable/Disable**: Toggle plugins without removing them
///
/// # Example
///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// use rustible::callback::manager::{CallbackManager, PluginPriority};
    /// use std::sync::Arc;
    ///
    /// let manager = CallbackManager::new();
    ///
    /// // Register with explicit priority
    /// manager
    ///     .register("stdout", Arc::new(DefaultCallback::new()), PluginPriority::STDOUT)
    ///     .await;
    ///
    /// // Register with default priority
    /// manager
    ///     .register_default("custom", Arc::new(MinimalCallback::new()))
    ///     .await;
///
/// // Dispatch events
/// let result = manager.on_playbook_start("deploy").await;
/// if !result.is_success() {
///     for err in &result.errors {
///         eprintln!("Plugin error: {}", err);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct CallbackManager {
    /// Registered plugins indexed by name
    plugins: RwLock<HashMap<String, PluginEntry>>,
    /// Cached priority-sorted plugin names for dispatch
    sorted_plugins: RwLock<Vec<String>>,
    /// Whether dispatch is currently paused
    paused: RwLock<bool>,
}

impl CallbackManager {
    /// Creates a new empty callback manager.
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            sorted_plugins: RwLock::new(Vec::new()),
            paused: RwLock::new(false),
        }
    }

    // ========================================================================
    // Plugin Registration
    // ========================================================================

    /// Registers a new callback plugin with a specific priority.
    ///
    /// If a plugin with the same name already exists, it will be replaced.
    ///
    /// # Arguments
    ///
    /// * `name` - Unique identifier for the plugin
    /// * `plugin` - The callback plugin implementation
    /// * `priority` - Execution priority (lower runs first)
    ///
    /// # Returns
    ///
    /// `true` if this is a new plugin, `false` if an existing plugin was replaced.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// # use rustible::callback::manager::{CallbackManager, PluginPriority};
    /// use rustible::callback::prelude::*;
    /// let manager = CallbackManager::new();
    /// let plugin = DefaultCallback::new();
    /// manager
    ///     .register("stdout", Arc::new(plugin), PluginPriority::STDOUT)
    ///     .await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register(
        &self,
        name: &str,
        plugin: Arc<dyn ExecutionCallback>,
        priority: PluginPriority,
    ) -> bool {
        let is_new;
        {
            let mut plugins = self.plugins.write();
            is_new = !plugins.contains_key(name);
            plugins.insert(name.to_string(), PluginEntry::new(plugin, priority));
        }

        self.update_sorted_plugins();

        debug!(
            plugin = %name,
            priority = priority.0,
            is_new = is_new,
            "Plugin registered"
        );

        is_new
    }

    /// Registers a new callback plugin with default (NORMAL) priority.
    ///
    /// This is a convenience method for plugins that don't need priority control.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// # use rustible::callback::manager::CallbackManager;
    /// use rustible::callback::prelude::*;
    /// let manager = CallbackManager::new();
    /// let plugin = MinimalCallback::new();
    /// manager.register_default("my_plugin", Arc::new(plugin)).await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register_default(&self, name: &str, plugin: Arc<dyn ExecutionCallback>) -> bool {
        self.register(name, plugin, PluginPriority::NORMAL).await
    }

    /// Deregisters a callback plugin by name.
    ///
    /// # Returns
    ///
    /// The removed plugin if it existed, or `None` if no plugin had that name.
    pub async fn deregister(&self, name: &str) -> Option<Arc<dyn ExecutionCallback>> {
        let entry = {
            let mut plugins = self.plugins.write();
            plugins.remove(name)
        };

        if let Some(entry) = entry {
            self.update_sorted_plugins();
            debug!(plugin = %name, "Plugin deregistered");
            Some(entry.plugin)
        } else {
            warn!(plugin = %name, "Attempted to deregister unknown plugin");
            None
        }
    }

    /// Returns the number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.read().len()
    }

    /// Returns a list of registered plugin names.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.read().keys().cloned().collect()
    }

    /// Checks if a plugin is registered.
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.read().contains_key(name)
    }

    /// Gets the priority of a registered plugin.
    pub fn get_priority(&self, name: &str) -> Option<PluginPriority> {
        self.plugins.read().get(name).map(|e| e.priority)
    }

    // ========================================================================
    // Plugin Enable/Disable
    // ========================================================================

    /// Enables a plugin by name.
    ///
    /// Enabled plugins receive events during dispatch.
    ///
    /// # Returns
    ///
    /// `true` if the plugin was found and enabled, `false` otherwise.
    pub fn enable_plugin(&self, name: &str) -> bool {
        let mut plugins = self.plugins.write();
        if let Some(entry) = plugins.get_mut(name) {
            entry.enabled = true;
            debug!(plugin = %name, "Plugin enabled");
            true
        } else {
            false
        }
    }

    /// Disables a plugin by name.
    ///
    /// Disabled plugins are skipped during event dispatch but remain registered.
    ///
    /// # Returns
    ///
    /// `true` if the plugin was found and disabled, `false` otherwise.
    pub fn disable_plugin(&self, name: &str) -> bool {
        let mut plugins = self.plugins.write();
        if let Some(entry) = plugins.get_mut(name) {
            entry.enabled = false;
            debug!(plugin = %name, "Plugin disabled");
            true
        } else {
            false
        }
    }

    /// Checks if a plugin is enabled.
    pub fn is_plugin_enabled(&self, name: &str) -> bool {
        self.plugins
            .read()
            .get(name)
            .map(|e| e.enabled)
            .unwrap_or(false)
    }

    // ========================================================================
    // Dispatch Control
    // ========================================================================

    /// Pauses event dispatch to all plugins.
    ///
    /// While paused, all dispatch methods return empty results.
    pub fn pause(&self) {
        *self.paused.write() = true;
        debug!("Callback dispatch paused");
    }

    /// Resumes event dispatch to all plugins.
    pub fn resume(&self) {
        *self.paused.write() = false;
        debug!("Callback dispatch resumed");
    }

    /// Checks if dispatch is currently paused.
    pub fn is_paused(&self) -> bool {
        *self.paused.read()
    }

    // ========================================================================
    // Internal Helpers
    // ========================================================================

    /// Updates the sorted plugin list based on priority.
    fn update_sorted_plugins(&self) {
        let plugins = self.plugins.read();
        let mut sorted: Vec<(String, PluginPriority)> = plugins
            .iter()
            .map(|(name, entry)| (name.clone(), entry.priority))
            .collect();

        // Sort by priority (lower values first)
        sorted.sort_by(|a, b| a.1.cmp(&b.1));

        let names: Vec<String> = sorted.into_iter().map(|(name, _)| name).collect();
        *self.sorted_plugins.write() = names;
    }

    /// Gets plugins in priority order for dispatch.
    fn get_ordered_plugins(&self) -> Vec<(String, Arc<dyn ExecutionCallback>, bool)> {
        let sorted = self.sorted_plugins.read();
        let plugins = self.plugins.read();

        sorted
            .iter()
            .filter_map(|name| {
                plugins
                    .get(name)
                    .map(|entry| (name.clone(), Arc::clone(&entry.plugin), entry.enabled))
            })
            .collect()
    }

    /// OPTIMIZATION: Get count of enabled plugins without full allocation
    fn enabled_plugin_count(&self) -> usize {
        let plugins = self.plugins.read();
        plugins.values().filter(|e| e.enabled).count()
    }

    // ========================================================================
    // Event Dispatch - ExecutionCallback Methods
    // ========================================================================

    /// Dispatches `on_playbook_start` event to all enabled plugins.
    ///
    /// Plugins are called in priority order. Errors are captured but don't
    /// stop dispatch to remaining plugins.
    ///
    /// OPTIMIZATION: Fast path for single plugin avoids tokio::spawn overhead
    pub async fn on_playbook_start(&self, name: &str) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();
        let enabled_count = plugins.iter().filter(|(_, _, e)| *e).count();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_playbook_start");

            // OPTIMIZATION: Direct call for single plugin, spawn for multiple
            // This avoids tokio::spawn overhead for the common single-plugin case
            if enabled_count == 1 {
                plugin.on_playbook_start(name).await;
                result.success_count += 1;
            } else {
                let dispatch_result = {
                    let plugin = Arc::clone(&plugin);
                    let name = name.to_string();
                    tokio::spawn(async move {
                        plugin.on_playbook_start(&name).await;
                    })
                    .await
                };

                match dispatch_result {
                    Ok(()) => result.success_count += 1,
                    Err(e) => {
                        let err = PluginError {
                            plugin_name: plugin_name.clone(),
                            event: "on_playbook_start".to_string(),
                            message: e.to_string(),
                        };
                        error!(%err, "Plugin error");
                        result.errors.push(err);
                    }
                }
            }
        }

        result
    }

    /// Dispatches `on_playbook_end` event to all enabled plugins.
    pub async fn on_playbook_end(&self, name: &str, success: bool) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_playbook_end");

            let dispatch_result = {
                let plugin = Arc::clone(&plugin);
                let name = name.to_string();
                tokio::spawn(async move {
                    plugin.on_playbook_end(&name, success).await;
                })
                .await
            };

            match dispatch_result {
                Ok(()) => result.success_count += 1,
                Err(e) => {
                    let err = PluginError {
                        plugin_name: plugin_name.clone(),
                        event: "on_playbook_end".to_string(),
                        message: e.to_string(),
                    };
                    error!(%err, "Plugin error");
                    result.errors.push(err);
                }
            }
        }

        result
    }

    /// Dispatches `on_play_start` event to all enabled plugins.
    pub async fn on_play_start(&self, name: &str, hosts: &[String]) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_play_start");

            let dispatch_result = {
                let plugin = Arc::clone(&plugin);
                let name = name.to_string();
                let hosts = hosts.to_vec();
                tokio::spawn(async move {
                    plugin.on_play_start(&name, &hosts).await;
                })
                .await
            };

            match dispatch_result {
                Ok(()) => result.success_count += 1,
                Err(e) => {
                    let err = PluginError {
                        plugin_name: plugin_name.clone(),
                        event: "on_play_start".to_string(),
                        message: e.to_string(),
                    };
                    error!(%err, "Plugin error");
                    result.errors.push(err);
                }
            }
        }

        result
    }

    /// Dispatches `on_play_end` event to all enabled plugins.
    pub async fn on_play_end(&self, name: &str, success: bool) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_play_end");

            let dispatch_result = {
                let plugin = Arc::clone(&plugin);
                let name = name.to_string();
                tokio::spawn(async move {
                    plugin.on_play_end(&name, success).await;
                })
                .await
            };

            match dispatch_result {
                Ok(()) => result.success_count += 1,
                Err(e) => {
                    let err = PluginError {
                        plugin_name: plugin_name.clone(),
                        event: "on_play_end".to_string(),
                        message: e.to_string(),
                    };
                    error!(%err, "Plugin error");
                    result.errors.push(err);
                }
            }
        }

        result
    }

    /// Dispatches `on_task_start` event to all enabled plugins.
    ///
    /// OPTIMIZATION: Fast path for single plugin avoids tokio::spawn overhead
    pub async fn on_task_start(&self, name: &str, host: &str) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();
        let enabled_count = plugins.iter().filter(|(_, _, e)| *e).count();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_task_start");

            // OPTIMIZATION: Direct call for single plugin, spawn for multiple
            if enabled_count == 1 {
                plugin.on_task_start(name, host).await;
                result.success_count += 1;
            } else {
                let dispatch_result = {
                    let plugin = Arc::clone(&plugin);
                    let name = name.to_string();
                    let host = host.to_string();
                    tokio::spawn(async move {
                        plugin.on_task_start(&name, &host).await;
                    })
                    .await
                };

                match dispatch_result {
                    Ok(()) => result.success_count += 1,
                    Err(e) => {
                        let err = PluginError {
                            plugin_name: plugin_name.clone(),
                            event: "on_task_start".to_string(),
                            message: e.to_string(),
                        };
                        error!(%err, "Plugin error");
                        result.errors.push(err);
                    }
                }
            }
        }

        result
    }

    /// Dispatches `on_task_complete` event to all enabled plugins.
    ///
    /// OPTIMIZATION: Fast path for single plugin avoids tokio::spawn and clone overhead
    pub async fn on_task_complete(&self, exec_result: &ExecutionResult) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();
        let enabled_count = plugins.iter().filter(|(_, _, e)| *e).count();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, task = %exec_result.task_name, "Dispatching on_task_complete");

            // OPTIMIZATION: Direct call for single plugin, spawn for multiple
            if enabled_count == 1 {
                plugin.on_task_complete(exec_result).await;
                result.success_count += 1;
            } else {
                let dispatch_result = {
                    let plugin = Arc::clone(&plugin);
                    let exec_result = exec_result.clone();
                    tokio::spawn(async move {
                        plugin.on_task_complete(&exec_result).await;
                    })
                    .await
                };

                match dispatch_result {
                    Ok(()) => result.success_count += 1,
                    Err(e) => {
                        let err = PluginError {
                            plugin_name: plugin_name.clone(),
                            event: "on_task_complete".to_string(),
                            message: e.to_string(),
                        };
                        error!(%err, "Plugin error");
                        result.errors.push(err);
                    }
                }
            }
        }

        result
    }

    /// Dispatches `on_handler_triggered` event to all enabled plugins.
    pub async fn on_handler_triggered(&self, name: &str) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_handler_triggered");

            let dispatch_result = {
                let plugin = Arc::clone(&plugin);
                let name = name.to_string();
                tokio::spawn(async move {
                    plugin.on_handler_triggered(&name).await;
                })
                .await
            };

            match dispatch_result {
                Ok(()) => result.success_count += 1,
                Err(e) => {
                    let err = PluginError {
                        plugin_name: plugin_name.clone(),
                        event: "on_handler_triggered".to_string(),
                        message: e.to_string(),
                    };
                    error!(%err, "Plugin error");
                    result.errors.push(err);
                }
            }
        }

        result
    }

    /// Dispatches `on_facts_gathered` event to all enabled plugins.
    pub async fn on_facts_gathered(&self, host: &str, facts: &Facts) -> DispatchResult {
        if *self.paused.read() {
            return DispatchResult::default();
        }

        let mut result = DispatchResult::default();
        let plugins = self.get_ordered_plugins();
        let facts_clone = facts.clone();

        for (plugin_name, plugin, enabled) in plugins {
            if !enabled {
                result.skipped_count += 1;
                continue;
            }

            trace!(plugin = %plugin_name, "Dispatching on_facts_gathered");

            let dispatch_result = {
                let plugin = Arc::clone(&plugin);
                let host = host.to_string();
                let facts = facts_clone.clone();
                tokio::spawn(async move {
                    plugin.on_facts_gathered(&host, &facts).await;
                })
                .await
            };

            match dispatch_result {
                Ok(()) => result.success_count += 1,
                Err(e) => {
                    let err = PluginError {
                        plugin_name: plugin_name.clone(),
                        event: "on_facts_gathered".to_string(),
                        message: e.to_string(),
                    };
                    error!(%err, "Plugin error");
                    result.errors.push(err);
                }
            }
        }

        result
    }
}

// ============================================================================
// Implement ExecutionCallback for CallbackManager
// ============================================================================

/// Implement `ExecutionCallback` so `CallbackManager` can be used as a callback.
///
/// This allows the manager to be passed anywhere a single callback is expected,
/// delegating to all registered plugins.
#[async_trait]
impl ExecutionCallback for CallbackManager {
    async fn on_playbook_start(&self, name: &str) {
        let _ = CallbackManager::on_playbook_start(self, name).await;
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let _ = CallbackManager::on_playbook_end(self, name, success).await;
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let _ = CallbackManager::on_play_start(self, name, hosts).await;
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let _ = CallbackManager::on_play_end(self, name, success).await;
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let _ = CallbackManager::on_task_start(self, name, host).await;
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let _ = CallbackManager::on_task_complete(self, result).await;
    }

    async fn on_handler_triggered(&self, name: &str) {
        let _ = CallbackManager::on_handler_triggered(self, name).await;
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let _ = CallbackManager::on_facts_gathered(self, host, facts).await;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use crate::traits::ModuleResult;

    /// Test plugin for verification
    #[derive(Debug, Default)]
    struct TestPlugin {
        call_count: AtomicU32,
    }

    impl TestPlugin {
        fn new() -> Self {
            Self::default()
        }

        fn calls(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ExecutionCallback for TestPlugin {
        async fn on_playbook_start(&self, _name: &str) {
            self.call_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn on_task_complete(&self, _result: &ExecutionResult) {
            self.call_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        assert!(
            manager
                .register(
                    "test",
                    plugin.clone() as Arc<dyn ExecutionCallback>,
                    PluginPriority::NORMAL
                )
                .await
        );
        assert_eq!(manager.plugin_count(), 1);
        assert!(manager.has_plugin("test"));
    }

    #[tokio::test]
    async fn test_plugin_deregistration() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;
        assert!(manager.has_plugin("test"));

        let removed = manager.deregister("test").await;
        assert!(removed.is_some());
        assert!(!manager.has_plugin("test"));
        assert_eq!(manager.plugin_count(), 0);
    }

    #[tokio::test]
    async fn test_event_dispatch() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        let result = manager.on_playbook_start("test_playbook").await;
        assert!(result.is_success());
        assert_eq!(result.success_count, 1);
        assert_eq!(plugin.calls(), 1);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let manager = CallbackManager::new();

        let low = Arc::new(TestPlugin::new());
        let high = Arc::new(TestPlugin::new());
        let normal = Arc::new(TestPlugin::new());

        // Register in non-priority order
        manager
            .register(
                "low",
                low.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::CLEANUP,
            )
            .await;
        manager
            .register(
                "high",
                high.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::STDOUT,
            )
            .await;
        manager
            .register(
                "normal",
                normal.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        let ordered = manager.get_ordered_plugins();
        assert_eq!(ordered[0].0, "high");
        assert_eq!(ordered[1].0, "normal");
        assert_eq!(ordered[2].0, "low");
    }

    #[tokio::test]
    async fn test_plugin_enable_disable() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;
        assert!(manager.is_plugin_enabled("test"));

        manager.disable_plugin("test");
        assert!(!manager.is_plugin_enabled("test"));

        // Dispatch should skip disabled plugins
        let result = manager.on_playbook_start("test").await;
        assert_eq!(result.skipped_count, 1);
        assert_eq!(result.success_count, 0);
        assert_eq!(plugin.calls(), 0);

        manager.enable_plugin("test");
        assert!(manager.is_plugin_enabled("test"));

        let result = manager.on_playbook_start("test").await;
        assert_eq!(result.success_count, 1);
        assert_eq!(plugin.calls(), 1);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        manager.pause();
        assert!(manager.is_paused());

        let result = manager.on_playbook_start("test").await;
        assert_eq!(result.success_count, 0);
        assert_eq!(plugin.calls(), 0);

        manager.resume();
        assert!(!manager.is_paused());

        let result = manager.on_playbook_start("test").await;
        assert_eq!(result.success_count, 1);
        assert_eq!(plugin.calls(), 1);
    }

    #[tokio::test]
    async fn test_multiple_plugins() {
        let manager = CallbackManager::new();

        let plugin1 = Arc::new(TestPlugin::new());
        let plugin2 = Arc::new(TestPlugin::new());
        let plugin3 = Arc::new(TestPlugin::new());

        manager
            .register(
                "p1",
                plugin1.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;
        manager
            .register(
                "p2",
                plugin2.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;
        manager
            .register(
                "p3",
                plugin3.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        let result = manager.on_playbook_start("test").await;
        assert_eq!(result.success_count, 3);
        assert_eq!(plugin1.calls(), 1);
        assert_eq!(plugin2.calls(), 1);
        assert_eq!(plugin3.calls(), 1);
    }

    #[tokio::test]
    async fn test_thread_safety() {
        let manager = Arc::new(CallbackManager::new());
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        let mut handles = Vec::new();

        for _ in 0..10 {
            let mgr = Arc::clone(&manager);
            handles.push(tokio::spawn(async move {
                mgr.on_playbook_start("test").await;
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(plugin.calls(), 10);
    }

    #[tokio::test]
    async fn test_task_complete_dispatch() {
        let manager = CallbackManager::new();
        let plugin = Arc::new(TestPlugin::new());

        manager
            .register(
                "test",
                plugin.clone() as Arc<dyn ExecutionCallback>,
                PluginPriority::NORMAL,
            )
            .await;

        let result = ExecutionResult {
            host: "localhost".to_string(),
            task_name: "test_task".to_string(),
            result: ModuleResult::ok("Success"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        let dispatch_result = manager.on_task_complete(&result).await;
        assert!(dispatch_result.is_success());
        assert_eq!(plugin.calls(), 1);
    }

    #[tokio::test]
    async fn test_replacing_existing_plugin() {
        let manager = CallbackManager::new();

        let plugin1 = Arc::new(TestPlugin::new());
        let plugin2 = Arc::new(TestPlugin::new());

        assert!(
            manager
                .register(
                    "test",
                    plugin1.clone() as Arc<dyn ExecutionCallback>,
                    PluginPriority::STDOUT
                )
                .await
        ); // New
        assert!(
            !manager
                .register(
                    "test",
                    plugin2.clone() as Arc<dyn ExecutionCallback>,
                    PluginPriority::NORMAL
                )
                .await
        ); // Replacement

        assert_eq!(manager.plugin_count(), 1);
        assert_eq!(manager.get_priority("test"), Some(PluginPriority::NORMAL));
    }

    #[tokio::test]
    async fn test_dispatch_result_helpers() {
        let mut result = DispatchResult::default();
        assert!(result.is_success());
        assert!(!result.has_errors());
        assert_eq!(result.total_dispatched(), 0);

        result.success_count = 2;
        result.skipped_count = 1;
        assert!(result.is_success());
        assert_eq!(result.total_dispatched(), 2);

        result.errors.push(PluginError {
            plugin_name: "test".to_string(),
            event: "test".to_string(),
            message: "error".to_string(),
        });
        assert!(!result.is_success());
        assert!(result.has_errors());
        assert_eq!(result.error_count(), 1);
        assert_eq!(result.total_dispatched(), 3);
    }
}
