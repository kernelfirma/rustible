//! Diff callback plugin for Rustible.
//!
//! This plugin displays before/after diffs for changed files, providing
//! visibility into exactly what changes are being made during execution.
//!
//! # Features
//!
//! - **Unified diff format**: Standard unified diff output for familiarity
//! - **Color-coded output**: Green for additions, red for deletions
//! - **Composable**: Can be combined with other callbacks via `CompositeCallback`
//! - **Respects --diff flag**: Only shows diffs when diff mode is enabled
//!
//! # Example Output
//!
//! ```text
//! TASK [Update nginx config] ****************************************************
//! --- before: /etc/nginx/nginx.conf
//! +++ after: /etc/nginx/nginx.conf
//! @@ -10,7 +10,7 @@
//!      server {
//!          listen 80;
//! -        server_name old.example.com;
//! +        server_name new.example.com;
//!          root /var/www/html;
//!      }
//!  }
//! changed: [webserver1]
//! ```

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use tokio::sync::RwLock;

use crate::executor::task::TaskDiff;
use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult, ModuleDiff};

/// Configuration for the diff callback plugin.
#[derive(Debug, Clone)]
pub struct DiffConfig {
    /// Number of context lines to show around changes (default: 3)
    pub context_lines: usize,
    /// Whether to use color in output (default: true)
    pub use_color: bool,
    /// Whether to show line numbers (default: true)
    pub show_line_numbers: bool,
    /// Maximum lines to display per diff (default: 100, 0 = unlimited)
    pub max_lines: usize,
    /// Whether diff mode is enabled (respects --diff flag)
    pub enabled: bool,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            context_lines: 3,
            use_color: true,
            show_line_numbers: true,
            max_lines: 100,
            enabled: true,
        }
    }
}

impl DiffConfig {
    /// Create a new config with diff mode enabled.
    pub fn enabled() -> Self {
        Self::default()
    }

    /// Create a new config with diff mode disabled.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Set the number of context lines.
    pub fn with_context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    /// Enable or disable color output.
    pub fn with_color(mut self, use_color: bool) -> Self {
        self.use_color = use_color;
        self
    }

    /// Enable or disable line numbers.
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Set maximum lines to display.
    pub fn with_max_lines(mut self, max: usize) -> Self {
        self.max_lines = max;
        self
    }
}

/// Statistics for diff output tracking.
#[derive(Debug, Clone, Default)]
struct DiffStats {
    /// Number of files with diffs shown
    files_diffed: u32,
    /// Total lines added
    lines_added: u32,
    /// Total lines removed
    lines_removed: u32,
}

/// Diff callback plugin that displays before/after changes.
///
/// This callback is designed to provide visibility into what changes
/// are being made during playbook execution, particularly for file
/// modifications.
///
/// # Design Principles
///
/// 1. **Unified Format**: Uses standard unified diff format
/// 2. **Color Coded**: Green for additions, red for deletions
/// 3. **Composable**: Works with `CompositeCallback` for combined output
/// 4. **Configurable**: Context lines, colors, line numbers
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{DiffCallback, DiffConfig};
///
/// // Basic usage with defaults
/// let callback = DiffCallback::new();
///
/// // With custom configuration
/// let config = DiffConfig::default()
///     .with_context_lines(5)
///     .with_color(true);
/// let callback = DiffCallback::with_config(config);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct DiffCallback {
    /// Configuration for diff output
    config: DiffConfig,
    /// Statistics for tracking diffs
    stats: Arc<RwLock<DiffStats>>,
    /// Current task name for context
    current_task: Arc<RwLock<Option<String>>>,
    /// Playbook start time
    start_time: Arc<RwLock<Option<Instant>>>,
}

impl DiffCallback {
    /// Creates a new diff callback with default configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = DiffCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(DiffConfig::default())
    }

    /// Creates a new diff callback with custom configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let config = DiffConfig::default().with_context_lines(5);
    /// let callback = DiffCallback::with_config(config);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_config(config: DiffConfig) -> Self {
        Self {
            config,
            stats: Arc::new(RwLock::new(DiffStats::default())),
            current_task: Arc::new(RwLock::new(None)),
            start_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Creates a callback that only shows diffs (enabled mode).
    pub fn enabled() -> Self {
        Self::with_config(DiffConfig::enabled())
    }

    /// Creates a callback that suppresses diffs (disabled mode).
    pub fn disabled() -> Self {
        Self::with_config(DiffConfig::disabled())
    }

    /// Returns whether diff mode is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Set whether diff mode is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Generates and prints a unified diff between before and after content.
    ///
    /// Returns the number of lines added and removed.
    fn print_unified_diff(
        &self,
        before: &str,
        after: &str,
        before_header: Option<&str>,
        after_header: Option<&str>,
    ) -> (u32, u32) {
        let diff = TextDiff::from_lines(before, after);

        let before_label = before_header.unwrap_or("before");
        let after_label = after_header.unwrap_or("after");

        let mut lines_added = 0u32;
        let mut lines_removed = 0u32;
        let mut output_lines = 0usize;

        // Print diff header
        if self.config.use_color {
            println!("{} {}", "---".red(), before_label.red());
            println!("{} {}", "+++".green(), after_label.green());
        } else {
            println!("--- {}", before_label);
            println!("+++ {}", after_label);
        }

        // Generate unified diff with context
        let unified = diff.unified_diff();

        for hunk in unified.iter_hunks() {
            // Print hunk header using the header() method
            let header = hunk.header();
            if self.config.use_color {
                println!("{}", header.to_string().cyan());
            } else {
                println!("{}", header);
            }

            for change in hunk.iter_changes() {
                // Check max lines limit
                if self.config.max_lines > 0 && output_lines >= self.config.max_lines {
                    let remaining_changes = hunk.iter_changes().count();
                    if self.config.use_color {
                        println!(
                            "{}",
                            format!("... ({} more changes)", remaining_changes).bright_black()
                        );
                    } else {
                        println!("... ({} more changes)", remaining_changes);
                    }
                    break;
                }

                let line = change.value();
                let line_content = line.strip_suffix('\n').unwrap_or(line);

                match change.tag() {
                    ChangeTag::Delete => {
                        lines_removed += 1;
                        output_lines += 1;
                        if self.config.use_color {
                            println!("{}{}", "-".red(), line_content.red());
                        } else {
                            println!("-{}", line_content);
                        }
                    }
                    ChangeTag::Insert => {
                        lines_added += 1;
                        output_lines += 1;
                        if self.config.use_color {
                            println!("{}{}", "+".green(), line_content.green());
                        } else {
                            println!("+{}", line_content);
                        }
                    }
                    ChangeTag::Equal => {
                        output_lines += 1;
                        println!(" {}", line_content);
                    }
                }
            }
        }

        (lines_added, lines_removed)
    }

    /// Formats a task diff for display.
    async fn display_task_diff(&self, diff: &TaskDiff) {
        if !self.config.enabled {
            return;
        }

        let before = diff.before.as_deref().unwrap_or("");
        let after = diff.after.as_deref().unwrap_or("");

        // Skip if no actual changes
        if before == after {
            return;
        }

        let before_header = diff.before_header.as_deref();
        let after_header = diff.after_header.as_deref();

        let (added, removed) = self.print_unified_diff(before, after, before_header, after_header);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.files_diffed += 1;
        stats.lines_added += added;
        stats.lines_removed += removed;
    }

    /// Formats a module diff for display.
    #[allow(dead_code)]
    async fn display_module_diff(&self, diff: &ModuleDiff) {
        if !self.config.enabled {
            return;
        }

        // Print description if available
        if !diff.description.is_empty() {
            if self.config.use_color {
                println!("{}: {}", "Diff".bright_white().bold(), diff.description);
            } else {
                println!("Diff: {}", diff.description);
            }
        }

        let before = diff.before.as_deref().unwrap_or("");
        let after = diff.after.as_deref().unwrap_or("");

        // Skip if no actual content to diff
        if before.is_empty() && after.is_empty() {
            return;
        }

        let (added, removed) = self.print_unified_diff(before, after, None, None);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.files_diffed += 1;
        stats.lines_added += added;
        stats.lines_removed += removed;
    }

    /// Returns statistics about diffs shown during execution.
    pub async fn get_stats(&self) -> (u32, u32, u32) {
        let stats = self.stats.read().await;
        (stats.files_diffed, stats.lines_added, stats.lines_removed)
    }

    /// Prints a summary of diff statistics.
    pub async fn print_summary(&self) {
        if !self.config.enabled {
            return;
        }

        let stats = self.stats.read().await;

        if stats.files_diffed > 0 {
            println!();
            if self.config.use_color {
                println!(
                    "{}: {} file(s), {} insertion(s)({}), {} deletion(s)({})",
                    "Diff Summary".bright_white().bold(),
                    stats.files_diffed,
                    stats.lines_added,
                    "+".green(),
                    stats.lines_removed,
                    "-".red(),
                );
            } else {
                println!(
                    "Diff Summary: {} file(s), {} insertion(s)(+), {} deletion(s)(-)",
                    stats.files_diffed, stats.lines_added, stats.lines_removed,
                );
            }
        }
    }
}

impl Default for DiffCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DiffCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            stats: Arc::clone(&self.stats),
            current_task: Arc::clone(&self.current_task),
            start_time: Arc::clone(&self.start_time),
        }
    }
}

#[async_trait]
impl ExecutionCallback for DiffCallback {
    async fn on_playbook_start(&self, name: &str) {
        let mut start_time = self.start_time.write().await;
        *start_time = Some(Instant::now());

        // Reset stats for new playbook
        let mut stats = self.stats.write().await;
        *stats = DiffStats::default();

        let _ = name;
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        // Print diff summary at the end
        self.print_summary().await;
    }

    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {
        // No action needed for diff callback
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        // No action needed for diff callback
    }

    async fn on_task_start(&self, name: &str, _host: &str) {
        let mut current = self.current_task.write().await;
        *current = Some(name.to_string());
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        if !self.config.enabled {
            return;
        }

        // Check if the result has diff data
        if let Some(ref data) = result.result.data {
            // Try to extract diff from module result data
            if let Some(diff_obj) = data.get("diff") {
                if let Ok(task_diff) = serde_json::from_value::<TaskDiff>(diff_obj.clone()) {
                    self.display_task_diff(&task_diff).await;
                }
            }

            // Also check for before/after fields directly
            let before = data.get("before").and_then(|v| v.as_str());
            let after = data.get("after").and_then(|v| v.as_str());

            if before.is_some() || after.is_some() {
                let task_diff = TaskDiff {
                    before: before.map(String::from),
                    after: after.map(String::from),
                    before_header: data
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("before: {}", p)),
                    after_header: data
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("after: {}", p)),
                };
                self.display_task_diff(&task_diff).await;
            }
        }
    }

    async fn on_handler_triggered(&self, _name: &str) {
        // No action needed for diff callback
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // No action needed for diff callback
    }
}

/// A composite callback that combines multiple callbacks.
///
/// This allows using the DiffCallback alongside other callbacks
/// like the DefaultCallback or MinimalCallback.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{DiffCallback, CompositeCallback};
/// use rustible::callback::MinimalCallback;
///
/// let composite = CompositeCallback::new()
///     .with_callback(Box::new(MinimalCallback::new()))
///     .with_callback(Box::new(DiffCallback::new()));
/// # Ok(())
/// # }
/// ```
pub struct CompositeCallback {
    callbacks: Vec<Box<dyn ExecutionCallback>>,
}

impl std::fmt::Debug for CompositeCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeCallback")
            .field("callbacks_count", &self.callbacks.len())
            .finish()
    }
}

impl CompositeCallback {
    /// Creates a new empty composite callback.
    pub fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    /// Adds a callback to the composite.
    pub fn with_callback(mut self, callback: Box<dyn ExecutionCallback>) -> Self {
        self.callbacks.push(callback);
        self
    }

    /// Adds a callback to the composite (mutable version).
    pub fn add_callback(&mut self, callback: Box<dyn ExecutionCallback>) {
        self.callbacks.push(callback);
    }
}

impl Default for CompositeCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for CompositeCallback {
    async fn on_playbook_start(&self, name: &str) {
        for callback in &self.callbacks {
            callback.on_playbook_start(name).await;
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        for callback in &self.callbacks {
            callback.on_playbook_end(name, success).await;
        }
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        for callback in &self.callbacks {
            callback.on_play_start(name, hosts).await;
        }
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        for callback in &self.callbacks {
            callback.on_play_end(name, success).await;
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        for callback in &self.callbacks {
            callback.on_task_start(name, host).await;
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        for callback in &self.callbacks {
            callback.on_task_complete(result).await;
        }
    }

    async fn on_handler_triggered(&self, name: &str) {
        for callback in &self.callbacks {
            callback.on_handler_triggered(name).await;
        }
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        for callback in &self.callbacks {
            callback.on_facts_gathered(host, facts).await;
        }
    }
}

/// Helper function to generate a unified diff string.
///
/// This can be used independently of the callback for generating
/// diff output programmatically.
///
/// # Arguments
///
/// * `before` - The original content
/// * `after` - The modified content
/// * `context_lines` - Number of context lines (default: 3)
///
/// # Returns
///
/// A string containing the unified diff output.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::generate_diff;
///
/// let before = "line1\nline2\nline3";
/// let after = "line1\nmodified\nline3";
/// let diff = generate_diff(before, after, 3);
/// println!("{}", diff);
/// # Ok(())
/// # }
/// ```
pub fn generate_diff(before: &str, after: &str, _context_lines: usize) -> String {
    let diff = TextDiff::from_lines(before, after);
    let mut output = String::new();

    output.push_str("--- before\n");
    output.push_str("+++ after\n");

    let unified = diff.unified_diff();

    for hunk in unified.iter_hunks() {
        // Use Display trait for the header
        output.push_str(&format!("{}\n", hunk.header()));

        for change in hunk.iter_changes() {
            let line = change.value();
            let line_content = line.strip_suffix('\n').unwrap_or(line);

            match change.tag() {
                ChangeTag::Delete => {
                    output.push_str(&format!("-{}\n", line_content));
                }
                ChangeTag::Insert => {
                    output.push_str(&format!("+{}\n", line_content));
                }
                ChangeTag::Equal => {
                    output.push_str(&format!(" {}\n", line_content));
                }
            }
        }
    }

    output
}

/// Helper function to check if two strings have differences.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::has_changes;
///
/// assert!(has_changes("old", "new"));
/// assert!(!has_changes("same", "same"));
/// # Ok(())
/// # }
/// ```
pub fn has_changes(before: &str, after: &str) -> bool {
    before != after
}

/// Counts the number of lines added and removed between two strings.
///
/// # Returns
///
/// A tuple of (lines_added, lines_removed).
pub fn count_changes(before: &str, after: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(before, after);
    let mut added = 0;
    let mut removed = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }

    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;

    #[test]
    fn test_diff_config_default() {
        let config = DiffConfig::default();
        assert_eq!(config.context_lines, 3);
        assert!(config.use_color);
        assert!(config.show_line_numbers);
        assert_eq!(config.max_lines, 100);
        assert!(config.enabled);
    }

    #[test]
    fn test_diff_config_builder() {
        let config = DiffConfig::default()
            .with_context_lines(5)
            .with_color(false)
            .with_line_numbers(false)
            .with_max_lines(50);

        assert_eq!(config.context_lines, 5);
        assert!(!config.use_color);
        assert!(!config.show_line_numbers);
        assert_eq!(config.max_lines, 50);
    }

    #[test]
    fn test_diff_config_enabled_disabled() {
        let enabled = DiffConfig::enabled();
        assert!(enabled.enabled);

        let disabled = DiffConfig::disabled();
        assert!(!disabled.enabled);
    }

    #[test]
    fn test_diff_callback_creation() {
        let callback = DiffCallback::new();
        assert!(callback.is_enabled());

        let callback = DiffCallback::enabled();
        assert!(callback.is_enabled());

        let callback = DiffCallback::disabled();
        assert!(!callback.is_enabled());
    }

    #[test]
    fn test_generate_diff() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nmodified\nline3\n";

        let diff = generate_diff(before, after, 3);

        assert!(diff.contains("--- before"));
        assert!(diff.contains("+++ after"));
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified"));
    }

    #[test]
    fn test_generate_diff_no_changes() {
        let content = "line1\nline2\nline3\n";
        let diff = generate_diff(content, content, 3);

        // Should still have headers but no change markers
        assert!(diff.contains("--- before"));
        assert!(diff.contains("+++ after"));
        // No hunks when content is identical
    }

    #[test]
    fn test_has_changes() {
        assert!(has_changes("old", "new"));
        assert!(!has_changes("same", "same"));
        assert!(has_changes("", "something"));
        assert!(has_changes("something", ""));
    }

    #[test]
    fn test_count_changes() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nmodified\nline3\nnew_line\n";

        let (added, removed) = count_changes(before, after);

        assert_eq!(removed, 1); // line2 removed
        assert_eq!(added, 2); // modified + new_line added
    }

    #[test]
    fn test_count_changes_no_diff() {
        let content = "line1\nline2\n";
        let (added, removed) = count_changes(content, content);

        assert_eq!(added, 0);
        assert_eq!(removed, 0);
    }

    #[tokio::test]
    async fn test_diff_callback_stats() {
        let callback = DiffCallback::new();

        // Initial stats should be zero
        let (files, added, removed) = callback.get_stats().await;
        assert_eq!(files, 0);
        assert_eq!(added, 0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = DiffCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.stats, &callback2.stats));
        assert!(Arc::ptr_eq(
            &callback1.current_task,
            &callback2.current_task
        ));
    }

    #[test]
    fn test_composite_callback_creation() {
        let composite = CompositeCallback::new();
        assert!(composite.callbacks.is_empty());
    }

    #[tokio::test]
    async fn test_composite_callback_with_diff() {
        let mut composite = CompositeCallback::new();
        composite.add_callback(Box::new(DiffCallback::new()));

        // Should not panic
        composite.on_playbook_start("test").await;
        composite.on_task_start("task", "host").await;
        composite.on_playbook_end("test", true).await;
    }

    #[test]
    fn test_diff_with_empty_content() {
        // Test adding new content
        let (added, removed) = count_changes("", "new content\n");
        assert_eq!(added, 1);
        assert_eq!(removed, 0);

        // Test removing all content
        let (added, removed) = count_changes("old content\n", "");
        assert_eq!(added, 0);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_diff_multiline() {
        let before = "header\nline1\nline2\nline3\nfooter\n";
        let after = "header\nline1\nMODIFIED\nline3\nfooter\n";

        let diff = generate_diff(before, after, 1);

        assert!(diff.contains("-line2"));
        assert!(diff.contains("+MODIFIED"));
        // Context lines should be present
        assert!(diff.contains(" line1"));
        assert!(diff.contains(" line3"));
    }

    fn _create_execution_result(
        host: &str,
        task_name: &str,
        success: bool,
        changed: bool,
        skipped: bool,
        message: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success,
                changed,
                message: message.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }
}
