//! Output formatting module for Rustible
//!
//! Provides colored output, progress indicators, and various output formats.

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Task execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TaskStatus {
    /// Task completed successfully with no changes
    Ok,
    /// Task completed with changes made
    Changed,
    /// Task was skipped
    Skipped,
    /// Task failed
    Failed,
    /// Task is unreachable (connection failed)
    Unreachable,
    /// Task is being rescued
    Rescued,
    /// Task is being ignored
    Ignored,
}

impl TaskStatus {
    /// Get the colored string representation
    pub fn colored_string(&self) -> String {
        match self {
            TaskStatus::Ok => "ok".green().to_string(),
            TaskStatus::Changed => "changed".yellow().to_string(),
            TaskStatus::Skipped => "skipping".cyan().to_string(),
            TaskStatus::Failed => "failed".red().bold().to_string(),
            TaskStatus::Unreachable => "unreachable".red().bold().to_string(),
            TaskStatus::Rescued => "rescued".magenta().to_string(),
            TaskStatus::Ignored => "ignored".blue().to_string(),
        }
    }

    /// Get the plain string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Ok => "ok",
            TaskStatus::Changed => "changed",
            TaskStatus::Skipped => "skipping",
            TaskStatus::Failed => "failed",
            TaskStatus::Unreachable => "unreachable",
            TaskStatus::Rescued => "rescued",
            TaskStatus::Ignored => "ignored",
        }
    }
}

/// Output formatter for different output modes
pub struct OutputFormatter {
    /// Use colored output
    use_color: bool,
    /// JSON output mode
    json_mode: bool,
    /// Verbosity level
    verbosity: u8,
    /// Start time for duration calculations
    start_time: Instant,
    /// Multi-progress bar container
    #[allow(dead_code)]
    multi_progress: Option<Arc<MultiProgress>>,
}

impl OutputFormatter {
    /// Create a new output formatter
    pub fn new(use_color: bool, json_mode: bool, verbosity: u8) -> Self {
        // Respect NO_COLOR environment variable
        let use_color = use_color && std::env::var("NO_COLOR").is_err();

        Self {
            use_color,
            json_mode,
            verbosity,
            start_time: Instant::now(),
            multi_progress: None,
        }
    }

    /// Initialize progress bar support
    #[allow(dead_code)]
    pub fn init_progress(&mut self) {
        if !self.json_mode {
            self.multi_progress = Some(Arc::new(MultiProgress::new()));
        }
    }

    /// Get the multi-progress bar container
    #[allow(dead_code)]
    pub fn multi_progress(&self) -> Option<Arc<MultiProgress>> {
        self.multi_progress.clone()
    }

    /// Print a banner/header
    pub fn banner(&self, title: &str) {
        if self.json_mode {
            return;
        }

        let line = "=".repeat(title.len() + 4);
        if self.use_color {
            println!("\n{}", line.bright_blue());
            println!("{}", format!("  {}  ", title).bright_blue().bold());
            println!("{}\n", line.bright_blue());
        } else {
            println!("\n{}", line);
            println!("  {}  ", title);
            println!("{}\n", line);
        }
    }

    /// Print a section header
    pub fn section(&self, title: &str) {
        if self.json_mode {
            return;
        }

        if self.use_color {
            println!("\n{}", title.cyan().bold());
            println!("{}", "-".repeat(title.len()).cyan());
        } else {
            println!("\n{}", title);
            println!("{}", "-".repeat(title.len()));
        }
    }

    /// Print a play header
    pub fn play_header(&self, play_name: &str) {
        if self.json_mode {
            return;
        }

        let header = format!("PLAY [{}]", play_name);
        let stars = "*".repeat(80_usize.saturating_sub(header.len()));

        if self.use_color {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }
    }

    /// Print a task header
    pub fn task_header(&self, task_name: &str) {
        if self.json_mode {
            return;
        }

        let header = format!("TASK [{}]", task_name);
        let stars = "*".repeat(80_usize.saturating_sub(header.len()));

        if self.use_color {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }
    }

    /// Print task result
    pub fn task_result(
        &self,
        host: &str,
        status: TaskStatus,
        message: Option<&str>,
        duration: Option<Duration>,
    ) {
        if self.json_mode {
            let result = serde_json::json!({
                "host": host,
                "status": status.as_str(),
                "message": message,
                "duration_ms": duration.map(|d| d.as_millis())
            });
            println!("{}", serde_json::to_string(&result).unwrap());
            return;
        }

        let status_str = if self.use_color {
            status.colored_string()
        } else {
            status.as_str().to_string()
        };

        let host_str = if self.use_color {
            host.bright_white().bold().to_string()
        } else {
            host.to_string()
        };

        match status {
            TaskStatus::Ok
            | TaskStatus::Changed
            | TaskStatus::Skipped
            | TaskStatus::Rescued
            | TaskStatus::Ignored => {
                print!("{}: [{}]", status_str, host_str);
            }
            TaskStatus::Failed | TaskStatus::Unreachable => {
                print!("{}: [{}]", status_str, host_str);
            }
        }

        if let Some(msg) = message {
            print!(" => {}", msg);
        }

        if let Some(d) = duration {
            if self.use_color {
                print!("  {}", format_duration(d).dimmed());
            } else {
                print!("  {}", format_duration(d));
            }
        }

        println!();
    }

    /// Print task result with detailed output
    #[allow(dead_code)]
    pub fn task_result_verbose(
        &self,
        host: &str,
        status: TaskStatus,
        details: &HashMap<String, String>,
    ) {
        if self.verbosity < 1 {
            return;
        }

        if self.json_mode {
            let result = serde_json::json!({
                "host": host,
                "status": status.as_str(),
                "details": details
            });
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            return;
        }

        self.task_result(host, status, None, None);

        for (key, value) in details {
            if self.use_color {
                println!("    {}: {}", key.bright_black(), value);
            } else {
                println!("    {}: {}", key, value);
            }
        }
    }

    /// Print a recap summary
    pub fn recap(&self, stats: &RecapStats) {
        if self.json_mode {
            println!("{}", serde_json::to_string_pretty(stats).unwrap());
            return;
        }

        let header = "PLAY RECAP";
        let stars = "*".repeat(80 - header.len());

        if self.use_color {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }

        for (host, host_stats) in &stats.hosts {
            let line = format!(
                "{:<30} : ok={:<4} changed={:<4} unreachable={:<4} failed={:<4} skipped={:<4} rescued={:<4} ignored={:<4}",
                host,
                host_stats.ok,
                host_stats.changed,
                host_stats.unreachable,
                host_stats.failed,
                host_stats.skipped,
                host_stats.rescued,
                host_stats.ignored
            );

            if self.use_color {
                let host_colored = if host_stats.failed > 0 || host_stats.unreachable > 0 {
                    host.red().bold()
                } else if host_stats.changed > 0 {
                    host.yellow()
                } else {
                    host.green()
                };

                // Helper to format stats: dim if zero, colored if non-zero
                let fmt_stat = |label: &str, value: u32, color: colored::Color| -> String {
                    if value > 0 {
                        format!("{}={:<4}", label.color(color), value)
                    } else {
                        format!("{}={:<4}", label, value).dimmed().to_string()
                    }
                };

                print!("{:<30} : ", host_colored);
                print!("{} ", fmt_stat("ok", host_stats.ok, colored::Color::Green));
                print!(
                    "{} ",
                    fmt_stat("changed", host_stats.changed, colored::Color::Yellow)
                );
                print!(
                    "{} ",
                    fmt_stat("unreachable", host_stats.unreachable, colored::Color::Red)
                );
                print!(
                    "{} ",
                    fmt_stat("failed", host_stats.failed, colored::Color::Red)
                );
                print!(
                    "{} ",
                    fmt_stat("skipped", host_stats.skipped, colored::Color::Cyan)
                );
                print!(
                    "{} ",
                    fmt_stat("rescued", host_stats.rescued, colored::Color::Magenta)
                );
                print!(
                    "{} ",
                    fmt_stat("ignored", host_stats.ignored, colored::Color::Blue)
                );
                println!();
            } else {
                println!("{}", line);
            }
        }

        // Print duration
        let duration = self.start_time.elapsed();
        let duration_str = format_duration(duration);

        if self.use_color {
            println!(
                "\n{} {}",
                "Playbook run took".bright_black(),
                duration_str.bright_white()
            );
        } else {
            println!("\nPlaybook run took {}", duration_str);
        }

        // Print final status summary
        if self.use_color {
            if stats.has_failures() {
                let failures = stats.total_failed();
                println!(
                    "{}",
                    format!("Playbook run failed ({} failed).", failures)
                        .red()
                        .bold()
                );
            } else {
                let changes = stats.total_changed();
                if changes > 0 {
                    println!(
                        "{}",
                        format!("Playbook completed successfully ({} changed).", changes)
                            .green()
                            .bold()
                    );
                } else {
                    println!(
                        "{}",
                        "Playbook completed successfully (no changes)."
                            .green()
                            .bold()
                    );
                }
            }
        } else if stats.has_failures() {
            let failures = stats.total_failed();
            println!("Playbook run failed ({} failed).", failures);
        } else {
            let changes = stats.total_changed();
            if changes > 0 {
                println!("Playbook completed successfully ({} changed).", changes);
            } else {
                println!("Playbook completed successfully (no changes).");
            }
        }
    }

    /// Print a success message
    pub fn success(&self, message: &str) {
        if self.json_mode {
            let success = serde_json::json!({
                "type": "success",
                "message": message
            });
            println!("{}", serde_json::to_string(&success).expect("Failed to serialize output"));
            return;
        }

        if self.use_color {
            println!("{} {}", "SUCCESS:".green().bold(), message);
        } else {
            println!("SUCCESS: {}", message);
        }
    }

    /// Print an error message
    pub fn error(&self, message: &str) {
        if self.json_mode {
            let err = serde_json::json!({
                "type": "error",
                "message": message
            });
            eprintln!("{}", serde_json::to_string(&err).unwrap());
            return;
        }

        if self.use_color {
            eprintln!("{} {}", "ERROR:".red().bold(), message);
        } else {
            eprintln!("ERROR: {}", message);
        }
    }

    /// Print a warning message
    pub fn warning(&self, message: &str) {
        if self.json_mode {
            let warn = serde_json::json!({
                "type": "warning",
                "message": message
            });
            eprintln!("{}", serde_json::to_string(&warn).unwrap());
            return;
        }

        if self.use_color {
            eprintln!("{} {}", "WARNING:".yellow().bold(), message);
        } else {
            eprintln!("WARNING: {}", message);
        }
    }

    /// Print a hint message
    pub fn hint(&self, message: &str) {
        if self.json_mode {
            let hint = serde_json::json!({
                "type": "hint",
                "message": message
            });
            eprintln!("{}", serde_json::to_string(&hint).unwrap());
            return;
        }

        if self.use_color {
            eprintln!("{} {}", "HINT:".cyan().bold(), message);
        } else {
            eprintln!("HINT: {}", message);
        }
    }

    /// Print an info message (respects verbosity)
    pub fn info(&self, message: &str) {
        if self.verbosity < 1 {
            return;
        }

        if self.json_mode {
            let info = serde_json::json!({
                "type": "info",
                "message": message
            });
            println!("{}", serde_json::to_string(&info).unwrap());
            return;
        }

        if self.use_color {
            println!("{} {}", "INFO:".blue(), message);
        } else {
            println!("INFO: {}", message);
        }
    }

    /// Print plan output (always shows, bypasses verbosity)
    pub fn plan(&self, message: &str) {
        if self.json_mode {
            let plan = serde_json::json!({
                "type": "plan",
                "message": message
            });
            println!("{}", serde_json::to_string(&plan).unwrap());
            return;
        }

        // Plan output is plain text (no prefix) for terraform-like appearance
        println!("{}", message);
    }

    /// Print a debug message (requires higher verbosity)
    pub fn debug(&self, message: &str) {
        if self.verbosity < 2 {
            return;
        }

        if self.json_mode {
            let debug = serde_json::json!({
                "type": "debug",
                "message": message
            });
            println!("{}", serde_json::to_string(&debug).unwrap());
            return;
        }

        if self.use_color {
            println!("{} {}", "DEBUG:".magenta(), message);
        } else {
            println!("DEBUG: {}", message);
        }
    }

    /// Print a diff output
    #[allow(dead_code)]
    pub fn diff(&self, old: &str, new: &str) {
        if self.json_mode {
            let diff = serde_json::json!({
                "type": "diff",
                "before": old,
                "after": new
            });
            println!("{}", serde_json::to_string(&diff).unwrap());
            return;
        }

        println!();
        for line in old.lines() {
            if self.use_color {
                println!("{}", format!("- {}", line).red());
            } else {
                println!("- {}", line);
            }
        }
        for line in new.lines() {
            if self.use_color {
                println!("{}", format!("+ {}", line).green());
            } else {
                println!("+ {}", line);
            }
        }
        println!();
    }

    /// Create a progress bar for a task
    #[allow(dead_code)]
    pub fn create_progress_bar(&self, len: u64, message: &str) -> Option<ProgressBar> {
        if self.json_mode {
            return None;
        }

        let mp = self.multi_progress.as_ref()?;
        let pb = mp.add(ProgressBar::new(len));

        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
                )
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message(message.to_string());

        Some(pb)
    }

    /// Create a spinner for indeterminate progress
    #[allow(dead_code)]
    pub fn create_spinner(&self, message: &str) -> Option<ProgressBar> {
        if self.json_mode {
            return None;
        }

        let mp = self.multi_progress.as_ref()?;
        let sp = mp.add(ProgressBar::new_spinner());

        sp.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg} {elapsed}")
                .unwrap(),
        );
        sp.set_message(message.to_string());
        sp.enable_steady_tick(Duration::from_millis(100));

        Some(sp)
    }

    /// Print a list of items
    pub fn list(&self, title: &str, items: &[String]) {
        if self.json_mode {
            let list = serde_json::json!({
                "type": "list",
                "title": title,
                "items": items
            });
            println!("{}", serde_json::to_string_pretty(&list).unwrap());
            return;
        }

        if self.use_color {
            println!("\n{}:", title.bright_white().bold());
        } else {
            println!("\n{}:", title);
        }

        for item in items {
            if self.use_color {
                println!("  {} {}", "-".bright_black(), item);
            } else {
                println!("  - {}", item);
            }
        }
    }

    /// Print a table
    #[allow(dead_code)]
    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        if self.json_mode {
            let table = serde_json::json!({
                "type": "table",
                "headers": headers,
                "rows": rows
            });
            println!("{}", serde_json::to_string_pretty(&table).unwrap());
            return;
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Print header
        let mut header_line = String::new();
        for (i, h) in headers.iter().enumerate() {
            if i > 0 {
                header_line.push_str(" | ");
            }
            header_line.push_str(&format!("{:width$}", h, width = widths[i]));
        }

        if self.use_color {
            println!("{}", header_line.bright_white().bold());
        } else {
            println!("{}", header_line);
        }

        // Print separator
        let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        if self.use_color {
            println!("{}", sep.join("-+-").bright_black());
        } else {
            println!("{}", sep.join("-+-"));
        }

        // Print rows
        for row in rows {
            let mut row_line = String::new();
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    row_line.push_str(" | ");
                }
                if i < widths.len() {
                    row_line.push_str(&format!("{:width$}", cell, width = widths[i]));
                }
            }
            println!("{}", row_line);
        }
    }

    /// Flush stdout
    pub fn flush(&self) {
        let _ = io::stdout().flush();
    }
}

/// Statistics for a single host
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct HostStats {
    pub ok: u32,
    pub changed: u32,
    pub unreachable: u32,
    pub failed: u32,
    pub skipped: u32,
    pub rescued: u32,
    pub ignored: u32,
}

impl HostStats {
    /// Create new empty stats
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a task status
    pub fn record(&mut self, status: TaskStatus) {
        match status {
            TaskStatus::Ok => self.ok += 1,
            TaskStatus::Changed => self.changed += 1,
            TaskStatus::Skipped => self.skipped += 1,
            TaskStatus::Failed => self.failed += 1,
            TaskStatus::Unreachable => self.unreachable += 1,
            TaskStatus::Rescued => self.rescued += 1,
            TaskStatus::Ignored => self.ignored += 1,
        }
    }

    /// Check if there were any failures
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

/// Recap statistics for all hosts
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RecapStats {
    pub hosts: HashMap<String, HostStats>,
}

impl RecapStats {
    /// Create new empty recap stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a task result for a host
    pub fn record(&mut self, host: &str, status: TaskStatus) {
        self.hosts
            .entry(host.to_string())
            .or_default()
            .record(status);
    }

    /// Check if any host had failures
    pub fn has_failures(&self) -> bool {
        self.hosts.values().any(|h| h.has_failures())
    }

    /// Get total task count
    #[allow(dead_code)]
    pub fn total_tasks(&self) -> u32 {
        self.hosts
            .values()
            .map(|h| {
                h.ok + h.changed + h.failed + h.unreachable + h.skipped + h.rescued + h.ignored
            })
            .sum()
    }

    /// Get total changed count
    pub fn total_changed(&self) -> u32 {
        self.hosts.values().map(|h| h.changed).sum()
    }

    /// Get total failed count (failed + unreachable)
    pub fn total_failed(&self) -> u32 {
        self.hosts.values().map(|h| h.failed + h.unreachable).sum()
    }
}

/// Format a duration as a human-readable string
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs = secs % 60;
        format!("{}h {}m {}s", hours, mins, secs)
    } else if secs >= 60 {
        let mins = secs / 60;
        let secs = secs % 60;
        format!("{}m {}s", mins, secs)
    } else if secs > 0 {
        format!("{}.{:03}s", secs, millis)
    } else {
        format!("{}ms", millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Ok.as_str(), "ok");
        assert_eq!(TaskStatus::Changed.as_str(), "changed");
        assert_eq!(TaskStatus::Failed.as_str(), "failed");

        // Test colored string content (without color codes for simplicity if we could strip them, but here we just check availability)
        // Note: Colored strings contain ANSI codes, so we can't easily assert equality with plain text.
        // But we can check it doesn't contain the old brackets
        assert!(!TaskStatus::Ok.colored_string().contains("[+]"));
        assert!(!TaskStatus::Changed.colored_string().contains("[~]"));
        assert!(!TaskStatus::Failed.colored_string().contains("[!]"));

        // It should contain the text
        assert!(TaskStatus::Ok.colored_string().contains("ok"));
        assert!(TaskStatus::Changed.colored_string().contains("changed"));
    }

    #[test]
    fn test_host_stats() {
        let mut stats = HostStats::new();
        stats.record(TaskStatus::Ok);
        stats.record(TaskStatus::Changed);
        stats.record(TaskStatus::Failed);

        assert_eq!(stats.ok, 1);
        assert_eq!(stats.changed, 1);
        assert_eq!(stats.failed, 1);
        assert!(stats.has_failures());
    }

    #[test]
    fn test_recap_stats() {
        let mut recap = RecapStats::new();
        recap.record("host1", TaskStatus::Ok);
        recap.record("host1", TaskStatus::Changed);
        recap.record("host2", TaskStatus::Failed);

        assert!(recap.has_failures());
        assert_eq!(recap.total_tasks(), 3);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_secs(5)), "5.000s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m 5s");
    }
}
