//! Dense callback plugin - compact output for large inventories
//!
//! This plugin provides compact output optimized for playbooks running on many hosts:
//! - Groups hosts by task result status (ok, changed, failed, skipped, unreachable)
//! - Shows multiple hosts per line
//! - Uses count-based display when many hosts have the same status
//! - Reduces output noise for large-scale deployments
//!
//! # Example Output
//!
//! ```text
//! PLAY [Configure web servers] ******************************************
//!
//! TASK [Install nginx] **************************************************
//!   ok: web[01:05]
//!   changed: web[06:10], db01
//!
//! TASK [Start service] **************************************************
//!   ok: (15 hosts)
//!   failed: web08 => Connection refused
//!
//! PLAY RECAP ************************************************************
//!   15 total hosts: ok=10 changed=4 failed=1
//!
//!   Failed hosts:
//!     web08: ok=2 changed=0 failed=1 unreachable=0
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::DenseCallback;
//! use std::sync::Arc;
//!
//! let callback = Arc::new(DenseCallback::new());
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Configuration for the dense callback plugin.
#[derive(Debug, Clone)]
pub struct DenseConfig {
    /// Maximum hosts to show per line before wrapping
    pub max_hosts_per_line: usize,
    /// Threshold above which to show "(N hosts)" instead of listing
    pub host_count_threshold: usize,
    /// Use colored output
    pub use_colors: bool,
    /// Verbosity level (0 = normal, 1+ = more details)
    pub verbosity: u8,
}

impl Default for DenseConfig {
    fn default() -> Self {
        Self {
            max_hosts_per_line: 6,
            host_count_threshold: 10,
            use_colors: true,
            verbosity: 0,
        }
    }
}

impl DenseConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum hosts per line.
    pub fn with_max_hosts_per_line(mut self, n: usize) -> Self {
        self.max_hosts_per_line = n;
        self
    }

    /// Set host count threshold for count-based display.
    pub fn with_host_count_threshold(mut self, n: usize) -> Self {
        self.host_count_threshold = n;
        self
    }

    /// Enable or disable colors.
    pub fn with_colors(mut self, enabled: bool) -> Self {
        self.use_colors = enabled;
        self
    }

    /// Set verbosity level.
    pub fn with_verbosity(mut self, level: u8) -> Self {
        self.verbosity = level;
        self
    }
}

/// Categories for grouping task results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResultCategory {
    Ok,
    Changed,
    Skipped,
    Failed,
    Unreachable,
}

impl ResultCategory {
    fn from_execution_result(result: &ExecutionResult) -> Self {
        if !result.result.success {
            ResultCategory::Failed
        } else if result.result.skipped {
            ResultCategory::Skipped
        } else if result.result.changed {
            ResultCategory::Changed
        } else {
            ResultCategory::Ok
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ResultCategory::Ok => "ok",
            ResultCategory::Changed => "changed",
            ResultCategory::Skipped => "skipping",
            ResultCategory::Failed => "failed",
            ResultCategory::Unreachable => "unreachable",
        }
    }

    /// Order for display (errors first for visibility)
    fn display_order(&self) -> u8 {
        match self {
            ResultCategory::Failed => 0,
            ResultCategory::Unreachable => 1,
            ResultCategory::Changed => 2,
            ResultCategory::Ok => 3,
            ResultCategory::Skipped => 4,
        }
    }
}

/// Accumulated task result for grouping
#[derive(Debug, Clone)]
struct AccumulatedResult {
    host: String,
    msg: Option<String>,
}

/// Per-host execution statistics
#[derive(Debug, Clone, Default)]
struct HostStats {
    ok: u32,
    changed: u32,
    failed: u32,
    skipped: u32,
    unreachable: u32,
}

impl HostStats {
    fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }

    fn has_changes(&self) -> bool {
        self.changed > 0
    }
}

/// Dense callback plugin for compact output with large inventories.
///
/// This plugin groups hosts by their task result status and displays them
/// compactly, making it ideal for playbooks targeting many hosts.
///
/// # Features
///
/// - **Host Grouping**: Groups hosts by status (ok, changed, failed, etc.)
/// - **Range Compression**: `web01, web02, web03` becomes `web[01:03]`
/// - **Count Display**: Shows `(15 hosts)` when many hosts have same status
/// - **Compact Recap**: Shows summary with detailed info only for failures
///
/// # Design Principles
///
/// 1. **Minimal Noise**: Reduce output for large inventories
/// 2. **Failure Visibility**: Always show failure details prominently
/// 3. **Smart Compression**: Use ranges and counts where appropriate
/// 4. **Fast Scanning**: Put important info (failures) first
#[derive(Debug)]
pub struct DenseCallback {
    config: DenseConfig,
    /// Accumulated results for current task (grouped by status)
    current_task_results: Arc<RwLock<HashMap<ResultCategory, Vec<AccumulatedResult>>>>,
    /// Current task name
    current_task_name: Arc<RwLock<Option<String>>>,
    /// Per-host statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Whether task header has been printed
    task_header_printed: Arc<RwLock<bool>>,
}

impl DenseCallback {
    /// Create a new dense callback with default configuration.
    pub fn new() -> Self {
        Self::with_config(DenseConfig::default())
    }

    /// Create a new dense callback with custom configuration.
    pub fn with_config(config: DenseConfig) -> Self {
        Self {
            config,
            current_task_results: Arc::new(RwLock::new(HashMap::new())),
            current_task_name: Arc::new(RwLock::new(None)),
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            task_header_printed: Arc::new(RwLock::new(false)),
        }
    }

    /// Format a list of hosts compactly.
    ///
    /// Strategies used:
    /// 1. If count > threshold: show "(N hosts)"
    /// 2. Try to detect ranges: host01, host02, host03 -> host[01:03]
    /// 3. Otherwise list hosts with max_per_line limit
    fn format_hosts(&self, hosts: &[String]) -> String {
        if hosts.is_empty() {
            return String::new();
        }

        // If too many hosts, just show count
        if hosts.len() > self.config.host_count_threshold {
            return format!("({} hosts)", hosts.len());
        }

        // Try to compress into ranges
        let compressed = Self::compress_host_ranges(hosts);

        // If we got good compression, use it
        if compressed.len() < hosts.len() {
            return Self::format_host_list(&compressed, self.config.max_hosts_per_line);
        }

        // Otherwise just list them
        Self::format_host_list(hosts, self.config.max_hosts_per_line)
    }

    /// Attempt to compress sequential hosts into ranges.
    /// e.g., ["web01", "web02", "web03"] -> ["web[01:03]"]
    fn compress_host_ranges(hosts: &[String]) -> Vec<String> {
        if hosts.len() < 3 {
            return hosts.to_vec();
        }

        let mut result = Vec::new();
        let mut hosts_sorted = hosts.to_vec();
        hosts_sorted.sort();

        // Group hosts by prefix
        let mut groups: HashMap<String, Vec<(String, u32)>> = HashMap::new();

        for host in &hosts_sorted {
            if let Some((prefix, num)) = Self::extract_host_prefix_and_number(host) {
                groups.entry(prefix).or_default().push((host.clone(), num));
            } else {
                result.push(host.clone());
            }
        }

        // Process each group
        for (prefix, mut host_nums) in groups {
            if host_nums.len() < 3 {
                // Not worth compressing
                for (host, _) in host_nums {
                    result.push(host);
                }
                continue;
            }

            host_nums.sort_by_key(|(_, n)| *n);

            // Find consecutive ranges
            let mut i = 0;
            while i < host_nums.len() {
                let start = host_nums[i].1;
                let mut end = start;
                let mut j = i + 1;

                while j < host_nums.len() && host_nums[j].1 == end + 1 {
                    end = host_nums[j].1;
                    j += 1;
                }

                if j - i >= 3 {
                    // Worth compressing - find the width from original hostname
                    let width = Self::number_width(&host_nums[i].0);
                    result.push(format!(
                        "{}[{:0width$}:{:0width$}]",
                        prefix,
                        start,
                        end,
                        width = width
                    ));
                } else {
                    // Just add individually
                    for host in &host_nums[i..j] {
                        result.push(host.0.clone());
                    }
                }

                i = j;
            }
        }

        result.sort();
        result
    }

    /// Extract prefix and trailing number from hostname.
    /// e.g., "web01" -> Some(("web", 1))
    fn extract_host_prefix_and_number(host: &str) -> Option<(String, u32)> {
        // Find where the trailing digits start
        let digit_start = host
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_digit())
            .last()
            .map(|(i, _)| i);

        if let Some(start) = digit_start {
            if start > 0 {
                let prefix = &host[..start];
                let num_str = &host[start..];
                if let Ok(num) = num_str.parse::<u32>() {
                    return Some((prefix.to_string(), num));
                }
            }
        }

        None
    }

    /// Determine the width of the number portion in a hostname.
    fn number_width(host: &str) -> usize {
        host.chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .count()
    }

    /// Format a list of hosts with line limit.
    fn format_host_list(hosts: &[String], max_per_line: usize) -> String {
        if hosts.is_empty() {
            return String::new();
        }

        if hosts.len() <= max_per_line {
            return hosts.join(", ");
        }

        // Split across lines
        let mut lines = Vec::new();
        for chunk in hosts.chunks(max_per_line) {
            lines.push(chunk.join(", "));
        }
        lines.join(",\n          ")
    }

    /// Print accumulated results for a category.
    fn print_category(&self, category: ResultCategory, results: &[AccumulatedResult]) {
        if results.is_empty() {
            return;
        }

        let hosts: Vec<String> = results.iter().map(|r| r.host.clone()).collect();
        let host_str = self.format_hosts(&hosts);
        let label = category.label();

        if self.config.use_colors {
            let colored_label = match category {
                ResultCategory::Ok => label.green(),
                ResultCategory::Changed => label.yellow(),
                ResultCategory::Skipped => label.cyan(),
                ResultCategory::Failed => label.red(),
                ResultCategory::Unreachable => label.red(),
            };
            print!("  {}: ", colored_label);
        } else {
            print!("  {}: ", label);
        }

        // For failures, show messages too
        if category == ResultCategory::Failed || category == ResultCategory::Unreachable {
            if results.len() == 1 {
                let result = &results[0];
                if self.config.use_colors {
                    print!("{}", result.host.bright_white().bold());
                } else {
                    print!("{}", result.host);
                }
                if let Some(ref msg) = result.msg {
                    print!(" => {}", msg);
                }
                println!();
            } else {
                // Multiple failures - list each with message
                println!();
                for result in results {
                    if self.config.use_colors {
                        print!("    {}", result.host.bright_white().bold());
                    } else {
                        print!("    {}", result.host);
                    }
                    if let Some(ref msg) = result.msg {
                        print!(" => {}", msg);
                    }
                    println!();
                }
            }
        } else {
            // Non-failure: just show hosts
            if self.config.use_colors {
                println!("{}", host_str.bright_white());
            } else {
                println!("{}", host_str);
            }
        }
    }

    /// Flush accumulated task results to output.
    fn flush_task_results(&self) {
        let mut results = self.current_task_results.write();
        if results.is_empty() {
            return;
        }

        // Sort categories by display order
        let mut categories: Vec<_> = results.keys().copied().collect();
        categories.sort_by_key(|c| c.display_order());

        for category in categories {
            if let Some(cat_results) = results.get(&category) {
                self.print_category(category, cat_results);
            }
        }

        results.clear();
    }

    /// Print the play header.
    fn print_play_header(&self, name: &str, host_count: usize) {
        let header = format!("PLAY [{}]", name);
        let stars = "*".repeat(80_usize.saturating_sub(header.len()));

        if self.config.use_colors {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }

        if self.config.verbosity > 0 {
            println!("  Targeting {} hosts", host_count);
        }
    }

    /// Print the task header.
    fn print_task_header(&self, name: &str) {
        let header = format!("TASK [{}]", name);
        let stars = "*".repeat(80_usize.saturating_sub(header.len()));

        if self.config.use_colors {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }
    }

    /// Print the final recap with host statistics.
    fn print_final_recap(&self) {
        let header = "PLAY RECAP";
        let divider = "*".repeat(80 - header.len());

        if self.config.use_colors {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                divider.bright_black()
            );
        } else {
            println!("\n{} {}", header, divider);
        }

        let stats = self.host_stats.read();

        // Calculate summary
        let mut total_ok = 0u32;
        let mut total_changed = 0u32;
        let mut total_failed = 0u32;
        let mut failed_hosts = Vec::new();
        let mut changed_hosts = Vec::new();

        for (host, host_stats) in stats.iter() {
            total_ok += host_stats.ok;
            total_changed += host_stats.changed;
            total_failed += host_stats.failed;

            if host_stats.has_failures() {
                failed_hosts.push((host.clone(), host_stats.clone()));
            } else if host_stats.has_changes() {
                changed_hosts.push(host.clone());
            }
        }

        // Print summary line
        if self.config.use_colors {
            println!(
                "\n  {} total hosts: {}={} {}={} {}={}",
                stats.len(),
                "ok".green(),
                total_ok,
                "changed".yellow(),
                total_changed,
                "failed".red(),
                total_failed
            );
        } else {
            println!(
                "\n  {} total hosts: ok={} changed={} failed={}",
                stats.len(),
                total_ok,
                total_changed,
                total_failed
            );
        }

        // Print failed hosts with details
        if !failed_hosts.is_empty() {
            if self.config.use_colors {
                println!("\n  {}:", "Failed hosts".red().bold());
            } else {
                println!("\n  Failed hosts:");
            }

            for (host, host_stats) in &failed_hosts {
                if self.config.use_colors {
                    println!(
                        "    {}: ok={} changed={} failed={} unreachable={}",
                        host.red().bold(),
                        host_stats.ok.to_string().green(),
                        host_stats.changed.to_string().yellow(),
                        host_stats.failed.to_string().red(),
                        host_stats.unreachable.to_string().red()
                    );
                } else {
                    println!(
                        "    {}: ok={} changed={} failed={} unreachable={}",
                        host,
                        host_stats.ok,
                        host_stats.changed,
                        host_stats.failed,
                        host_stats.unreachable
                    );
                }
            }
        }

        // Show changed hosts if verbose
        if self.config.verbosity > 0 && !changed_hosts.is_empty() {
            let hosts_str = self.format_hosts(&changed_hosts);
            if self.config.use_colors {
                println!("\n  {}: {}", "Changed hosts".yellow(), hosts_str);
            } else {
                println!("\n  Changed hosts: {}", hosts_str);
            }
        }
    }
}

impl Default for DenseCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DenseCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            current_task_results: Arc::clone(&self.current_task_results),
            current_task_name: Arc::clone(&self.current_task_name),
            host_stats: Arc::clone(&self.host_stats),
            start_time: Arc::clone(&self.start_time),
            task_header_printed: Arc::clone(&self.task_header_printed),
        }
    }
}

#[async_trait]
impl ExecutionCallback for DenseCallback {
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());
        self.host_stats.write().clear();

        if self.config.verbosity > 0 {
            if self.config.use_colors {
                println!("{}: {}", "Playbook".bright_black(), name.bright_white());
            } else {
                println!("Playbook: {}", name);
            }
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        // Flush any remaining results
        self.flush_task_results();

        // Print final recap
        self.print_final_recap();

        let start_time = *self.start_time.read();
        if let Some(start) = start_time {
            let duration = start.elapsed();
            let status = if success {
                if self.config.use_colors {
                    "completed".green()
                } else {
                    "completed".into()
                }
            } else if self.config.use_colors {
                "failed".red().bold()
            } else {
                "failed".into()
            };

            println!(
                "\n{} {} in {:.2}s",
                if self.config.use_colors {
                    name.bright_white().bold().to_string()
                } else {
                    name.to_string()
                },
                status,
                duration.as_secs_f64()
            );
        }
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Flush any pending results
        self.flush_task_results();

        self.print_play_header(name, hosts.len());

        // Initialize stats for all hosts
        let mut stats = self.host_stats.write();
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Flush any pending results
        self.flush_task_results();
    }

    async fn on_task_start(&self, name: &str, _host: &str) {
        // Check if we need to print a new task header
        let current_name = self.current_task_name.read().clone();
        if current_name.as_deref() != Some(name) {
            // Flush previous task results
            self.flush_task_results();

            // Print new task header
            self.print_task_header(name);
            *self.current_task_name.write() = Some(name.to_string());
            *self.task_header_printed.write() = true;
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update host stats
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(result.host.clone()).or_default();

            if !result.result.success {
                host_stats.failed += 1;
            } else if result.result.skipped {
                host_stats.skipped += 1;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Accumulate result by category
        let category = ResultCategory::from_execution_result(result);
        let accumulated = AccumulatedResult {
            host: result.host.clone(),
            msg: if !result.result.success {
                Some(result.result.message.clone())
            } else {
                None
            },
        };

        self.current_task_results
            .write()
            .entry(category)
            .or_default()
            .push(accumulated);
    }

    async fn on_handler_triggered(&self, name: &str) {
        // Flush pending results
        self.flush_task_results();

        if self.config.use_colors {
            println!("\n{}: {}", "HANDLER".bright_white().bold(), name.yellow());
        } else {
            println!("\nHANDLER: {}", name);
        }
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        if self.config.verbosity > 1 {
            if self.config.use_colors {
                println!("  {}: {}", "facts".bright_black(), host);
            } else {
                println!("  facts: {}", host);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;

    fn create_execution_result(
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

    #[test]
    fn test_dense_callback_name() {
        let callback = DenseCallback::new();
        // Verify it was created successfully
        assert!(callback.config.use_colors);
    }

    #[test]
    fn test_extract_host_prefix_and_number() {
        assert_eq!(
            DenseCallback::extract_host_prefix_and_number("web01"),
            Some(("web".to_string(), 1))
        );
        assert_eq!(
            DenseCallback::extract_host_prefix_and_number("server123"),
            Some(("server".to_string(), 123))
        );
        assert_eq!(
            DenseCallback::extract_host_prefix_and_number("db-node-05"),
            Some(("db-node-".to_string(), 5))
        );
        assert_eq!(
            DenseCallback::extract_host_prefix_and_number("localhost"),
            None
        );
        assert_eq!(DenseCallback::extract_host_prefix_and_number("01"), None);
    }

    #[test]
    fn test_compress_host_ranges() {
        // Consecutive hosts should be compressed
        let hosts = vec![
            "web01".to_string(),
            "web02".to_string(),
            "web03".to_string(),
            "web04".to_string(),
            "web05".to_string(),
        ];
        let compressed = DenseCallback::compress_host_ranges(&hosts);
        assert_eq!(compressed.len(), 1);
        assert!(compressed[0].contains("[01:05]"));

        // Mixed hosts
        let hosts = vec![
            "web01".to_string(),
            "web02".to_string(),
            "web03".to_string(),
            "db01".to_string(),
            "db02".to_string(),
        ];
        let compressed = DenseCallback::compress_host_ranges(&hosts);
        // db hosts not enough for compression, web hosts are
        assert!(compressed.iter().any(|h| h.contains("web[01:03]")));

        // Non-consecutive shouldn't compress
        let hosts = vec![
            "web01".to_string(),
            "web03".to_string(),
            "web05".to_string(),
        ];
        let compressed = DenseCallback::compress_host_ranges(&hosts);
        assert_eq!(compressed.len(), 3); // No compression
    }

    #[test]
    fn test_format_hosts_count_threshold() {
        let config = DenseConfig::new().with_host_count_threshold(5);
        let callback = DenseCallback::with_config(config);

        // Under threshold - list all (use non-compressible names)
        let hosts = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let formatted = callback.format_hosts(&hosts);
        assert!(
            formatted.contains("alpha"),
            "Expected 'alpha' in output but got: {}",
            formatted
        );
        assert!(!formatted.contains("hosts)"));

        // Over threshold - show count
        // Use non-sequential host names to avoid range compression
        let hosts: Vec<String> = (1..=10).map(|i| format!("node-{}", i * 3)).collect();
        let formatted = callback.format_hosts(&hosts);
        assert!(
            formatted.contains("10 hosts"),
            "Expected '(10 hosts)' but got: {}",
            formatted
        );
    }

    #[test]
    fn test_result_category_from_execution_result() {
        let ok_result = create_execution_result("test", "task", true, false, false, "ok");
        assert_eq!(
            ResultCategory::from_execution_result(&ok_result),
            ResultCategory::Ok
        );

        let changed_result = create_execution_result("test", "task", true, true, false, "changed");
        assert_eq!(
            ResultCategory::from_execution_result(&changed_result),
            ResultCategory::Changed
        );

        let failed_result = create_execution_result("test", "task", false, false, false, "error");
        assert_eq!(
            ResultCategory::from_execution_result(&failed_result),
            ResultCategory::Failed
        );

        let skipped_result = create_execution_result("test", "task", true, false, true, "skipped");
        assert_eq!(
            ResultCategory::from_execution_result(&skipped_result),
            ResultCategory::Skipped
        );
    }

    #[test]
    fn test_category_display_order() {
        // Failed should come first
        assert!(ResultCategory::Failed.display_order() < ResultCategory::Ok.display_order());
        assert!(
            ResultCategory::Unreachable.display_order() < ResultCategory::Changed.display_order()
        );
    }

    #[test]
    fn test_number_width() {
        assert_eq!(DenseCallback::number_width("web01"), 2);
        assert_eq!(DenseCallback::number_width("server123"), 3);
        assert_eq!(DenseCallback::number_width("localhost"), 0);
    }

    #[test]
    fn test_format_host_list() {
        let hosts = vec!["h1".to_string(), "h2".to_string(), "h3".to_string()];
        let formatted = DenseCallback::format_host_list(&hosts, 6);
        assert_eq!(formatted, "h1, h2, h3");

        // Should wrap
        let formatted = DenseCallback::format_host_list(&hosts, 2);
        assert!(formatted.contains("\n"));
    }

    #[test]
    fn test_config_builder() {
        let config = DenseConfig::new()
            .with_max_hosts_per_line(10)
            .with_host_count_threshold(20)
            .with_colors(false)
            .with_verbosity(2);

        assert_eq!(config.max_hosts_per_line, 10);
        assert_eq!(config.host_count_threshold, 20);
        assert!(!config.use_colors);
        assert_eq!(config.verbosity, 2);
    }

    #[test]
    fn test_default_trait() {
        let callback = DenseCallback::default();
        assert!(callback.config.use_colors);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = DenseCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(
            &callback1.current_task_results,
            &callback2.current_task_results
        ));
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
    }

    #[tokio::test]
    async fn test_playbook_lifecycle() {
        let callback = DenseCallback::with_config(DenseConfig::new().with_colors(false));

        callback.on_playbook_start("test-playbook").await;

        // Verify start time was set
        assert!(callback.start_time.read().is_some());

        callback.on_playbook_end("test-playbook", true).await;
    }

    #[tokio::test]
    async fn test_task_accumulation() {
        let callback = DenseCallback::with_config(DenseConfig::new().with_colors(false));

        callback.on_playbook_start("test").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        callback.on_task_start("Install nginx", "host1").await;

        let result1 = create_execution_result("host1", "Install nginx", true, false, false, "ok");
        callback.on_task_complete(&result1).await;

        let result2 =
            create_execution_result("host2", "Install nginx", true, true, false, "changed");
        callback.on_task_complete(&result2).await;

        // Verify results were accumulated
        let results = callback.current_task_results.read();
        assert!(results.contains_key(&ResultCategory::Ok));
        assert!(results.contains_key(&ResultCategory::Changed));
    }

    #[tokio::test]
    async fn test_host_stats_tracking() {
        let callback = DenseCallback::with_config(DenseConfig::new().with_colors(false));

        callback.on_playbook_start("test").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback.on_task_start("task1", "host1").await;
        let result1 = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&result1).await;

        callback.on_task_start("task2", "host1").await;
        let result2 = create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&result2).await;

        callback.on_task_start("task3", "host1").await;
        let result3 = create_execution_result("host1", "task3", false, false, false, "failed");
        callback.on_task_complete(&result3).await;

        let stats = callback.host_stats.read();
        let host_stats = stats.get("host1").unwrap();
        assert_eq!(host_stats.ok, 1);
        assert_eq!(host_stats.changed, 1);
        assert_eq!(host_stats.failed, 1);
    }
}
