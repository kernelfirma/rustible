//! Output formatting module for Rustible
//!
//! Provides colored output, progress indicators, and various output formats.

use colored::Colorize;
use console::measure_text_width;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
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
    pub fn colored_string(&self) -> &'static str {
        static OK: Lazy<String> = Lazy::new(|| "✔ ok".green().to_string());
        static CHANGED: Lazy<String> = Lazy::new(|| "✎ changed".yellow().to_string());
        static SKIPPED: Lazy<String> = Lazy::new(|| "↷ skipping".cyan().to_string());
        static FAILED: Lazy<String> = Lazy::new(|| "✖ failed".red().bold().to_string());
        static UNREACHABLE: Lazy<String> = Lazy::new(|| "✘ unreachable".red().bold().to_string());
        static RESCUED: Lazy<String> = Lazy::new(|| "✚ rescued".magenta().to_string());
        static IGNORED: Lazy<String> = Lazy::new(|| "⊘ ignored".blue().to_string());

        match self {
            TaskStatus::Ok => &OK,
            TaskStatus::Changed => &CHANGED,
            TaskStatus::Skipped => &SKIPPED,
            TaskStatus::Failed => &FAILED,
            TaskStatus::Unreachable => &UNREACHABLE,
            TaskStatus::Rescued => &RESCUED,
            TaskStatus::Ignored => &IGNORED,
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
#[derive(Clone)]
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

    /// Check if JSON mode is enabled
    pub fn is_json(&self) -> bool {
        self.json_mode
    }

    /// Print a banner/header
    pub fn banner(&self, title: &str) {
        if self.json_mode {
            return;
        }

        let width = measure_text_width(title);
        let horizontal = "─".repeat(width + 4);

        if self.use_color {
            println!("\n{}", format!("┌{}┐", horizontal).bright_blue());
            println!(
                "{}",
                format!("│  {}  │", title).bright_blue().bold()
            );
            println!("{}\n", format!("└{}┘", horizontal).bright_blue());
        } else {
            println!("\n┌{}┐", horizontal);
            println!("│  {}  │", title);
            println!("└{}┘\n", horizontal);
        }
    }

    /// Print a section header
    pub fn section(&self, title: &str) {
        if self.json_mode {
            return;
        }

        let width = measure_text_width(title);
        if self.use_color {
            println!("\n{}", title.cyan().bold());
            println!("{}", "─".repeat(width).cyan());
        } else {
            println!("\n{}", title);
            println!("{}", "─".repeat(width));
        }
    }

    /// Print a play header
    pub fn play_header(&self, play_name: &str) {
        if self.json_mode {
            return;
        }

        let header = format!("PLAY [{}]", play_name);
        let stars = "─".repeat(80_usize.saturating_sub(measure_text_width(&header)));

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
        let stars = "─".repeat(80_usize.saturating_sub(measure_text_width(&header)));

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
            status.as_str()
        };

        // Calculate padding for alignment (longest status is "unreachable" = 11 chars)
        let padding_len = 11usize.saturating_sub(status.as_str().len());

        let host_str = if self.use_color {
            match status {
                TaskStatus::Ok => host.green().to_string(),
                TaskStatus::Changed => host.yellow().to_string(),
                TaskStatus::Skipped => host.cyan().to_string(),
                TaskStatus::Failed | TaskStatus::Unreachable => host.red().bold().to_string(),
                TaskStatus::Rescued => host.magenta().to_string(),
                TaskStatus::Ignored => host.blue().to_string(),
            }
        } else {
            host.to_string()
        };

        match status {
            TaskStatus::Ok
            | TaskStatus::Changed
            | TaskStatus::Skipped
            | TaskStatus::Rescued
            | TaskStatus::Ignored => {
                print!(
                    "{}{:width$}: [{}]",
                    status_str,
                    "",
                    host_str,
                    width = padding_len
                );
            }
            TaskStatus::Failed | TaskStatus::Unreachable => {
                print!(
                    "{}{:width$}: [{}]",
                    status_str,
                    "",
                    host_str,
                    width = padding_len
                );
            }
        }

        if let Some(msg) = message {
            print!(" => {}", msg);
        }

        if let Some(d) = duration {
            let duration_str = format_duration(d);
            if self.use_color {
                if d.as_secs() >= 5 {
                    print!("  {}", duration_str.red());
                } else if d.as_secs() >= 1 {
                    print!("  {}", duration_str.yellow());
                } else {
                    print!("  {}", duration_str.dimmed());
                }
            } else {
                print!("  {}", duration_str);
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
            let duration = self.start_time.elapsed();
            let result = serde_json::json!({
                "hosts": &stats.hosts,
                "duration_ms": duration.as_millis(),
            });
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            return;
        }

        let header = "PLAY RECAP";
        let stars = "─".repeat(80_usize.saturating_sub(measure_text_width(header)));

        if self.use_color {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }

        // Calculate max host length for alignment (min 30)
        let max_host_len = stats
            .hosts
            .keys()
            .map(|h| measure_text_width(h))
            .max()
            .unwrap_or(0)
            .max(30);

        // Sort hosts alphabetically
        let mut sorted_hosts: Vec<_> = stats.hosts.keys().collect();
        sorted_hosts.sort();

        for host in sorted_hosts {
            let host_stats = &stats.hosts[host];

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

                // Manual padding to ensure proper visual alignment with ANSI codes
                let padding_len = max_host_len.saturating_sub(measure_text_width(host));
                print!("{}{:width$}: ", host_colored, "", width = padding_len);
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
                let line = format!(
                    "{:<width$} : ok={:<4} changed={:<4} unreachable={:<4} failed={:<4} skipped={:<4} rescued={:<4} ignored={:<4}",
                    host,
                    host_stats.ok,
                    host_stats.changed,
                    host_stats.unreachable,
                    host_stats.failed,
                    host_stats.skipped,
                    host_stats.rescued,
                    host_stats.ignored,
                    width = max_host_len
                );
                println!("{}", line);
            }
        }

        // Print execution summary
        let duration = self.start_time.elapsed();
        let duration_str = format_duration(duration);

        println!();
        if self.use_color {
            let width = 50;
            let line = "─".repeat(width).bright_black();
            println!("{}", line);

            // Align labels using visual width
            let duration_label = "⏱️  Duration";
            let status_label = "🏁 Status";
            let label_width: usize = 15;

            let align_label = |text: &str| -> String {
                let w = measure_text_width(text);
                let padding = label_width.saturating_sub(w);
                format!("{}{}", text, " ".repeat(padding))
            };

            println!(
                "  {} : {}",
                align_label(duration_label).bright_white(),
                duration_str.cyan()
            );

            if stats.has_failures() {
                let failures = stats.total_failed();
                let status_msg = format!("✖ FAILED ({} errors)", failures);
                println!(
                    "  {} : {}",
                    align_label(status_label).bright_white(),
                    status_msg.red().bold()
                );
            } else {
                let changes = stats.total_changed();
                let (status_msg, status_color) = if changes > 0 {
                    (
                        format!("✔ SUCCESS ({} changed)", changes),
                        colored::Color::Yellow,
                    )
                } else {
                    ("✔ SUCCESS (no changes)".to_string(), colored::Color::Green)
                };
                println!(
                    "  {} : {}",
                    align_label(status_label).bright_white(),
                    status_msg.color(status_color).bold()
                );
            }

            println!();

            // Generate banners with correct visual width
            let banner_width = 60;

            if stats.has_failures() {
                let failures = stats.total_failed();
                let message = format!("✖ FAILED ({} errors)", failures);
                let banner = format_banner(&message, banner_width);
                println!("{}", banner.red().bold());
            } else {
                let changes = stats.total_changed();
                let message = if changes > 0 {
                    format!("✔ SUCCESS ({} changed)", changes)
                } else {
                    "✔ SUCCESS (no changes)".to_string()
                };
                let banner = format_banner(&message, banner_width);
                println!("{}", banner.green().bold());
            }

            println!("{}", line);
        } else {
            let width = 50;
            let line = "-".repeat(width);
            println!("{}", line);
            println!("  {:<12} : {}", "Duration", duration_str);

            if stats.has_failures() {
                let failures = stats.total_failed();
                println!("  {:<12} : FAILED ({} errors)", "Status", failures);
            } else {
                let changes = stats.total_changed();
                let status_msg = if changes > 0 {
                    format!("SUCCESS ({} changed)", changes)
                } else {
                    "SUCCESS (no changes)".to_string()
                };
                println!("  {:<12} : {}", "Status", status_msg);
            }
            println!("{}", line);
        }
    }

    /// Print a success message
    pub fn success(&self, message: &str) {
        if self.json_mode {
            let success = serde_json::json!({
                "type": "success",
                "message": message
            });
            println!(
                "{}",
                serde_json::to_string(&success).expect("Failed to serialize output")
            );
            return;
        }

        if self.use_color {
            println!("{} {}", "✔ SUCCESS:".green().bold(), message);
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
            eprintln!("{} {}", "✖ ERROR:".red().bold(), message);
        } else {
            eprintln!("ERROR: {}", message);
        }
    }

    /// Print a diagnostic message without extra prefixes
    pub fn diagnostic(&self, message: &str) {
        if self.json_mode {
            let err = serde_json::json!({
                "type": "diagnostic",
                "message": message
            });
            eprintln!("{}", serde_json::to_string(&err).unwrap());
            return;
        }

        eprintln!("{}", message);
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
            eprintln!("{} {}", "⚠ WARNING:".yellow().bold(), message);
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
            eprintln!("{} {}", "💡 HINT:".cyan().bold(), message);
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

    /// Print a created status
    pub fn created(&self, message: &str) {
        if self.json_mode {
            let success = serde_json::json!({
                "type": "created",
                "message": message
            });
            println!("{}", serde_json::to_string(&success).unwrap());
            return;
        }

        if self.use_color {
            println!("{} {}", "✔ Created".green().bold(), message);
        } else {
            println!("Created {}", message);
        }
    }

    /// Print a skipped status
    pub fn skipped(&self, message: &str) {
        if self.json_mode {
            let success = serde_json::json!({
                "type": "skipped",
                "message": message
            });
            println!("{}", serde_json::to_string(&success).unwrap());
            return;
        }

        if self.use_color {
            println!("{} {}", "↷ Skipped".cyan(), message);
        } else {
            println!("Skipped {}", message);
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
                .progress_chars("━╸ "),
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
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.green} {msg} {elapsed}")
                .unwrap(),
        );
        sp.set_message(message.to_string());
        sp.enable_steady_tick(Duration::from_millis(80));

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

    /// Print a plan header (Terraform-style)
    pub fn plan_header(&self, title: &str) {
        if self.json_mode {
            return;
        }

        println!();
        if self.use_color {
            println!("{}", "─".repeat(78).bright_black());
            println!("{}", title.bright_white().bold());
            println!("{}", "─".repeat(78).bright_black());
        } else {
            println!("{}", "─".repeat(78));
            println!("{}", title);
            println!("{}", "─".repeat(78));
        }
        println!();
    }

    /// Print a resource change in Terraform style (+, ~, -, -/+)
    ///
    /// `action` should be one of: "create", "update", "delete", "replace"
    pub fn plan_resource_change(
        &self,
        action: &str,
        resource_type: &str,
        resource_name: &str,
        details: Option<&str>,
    ) {
        if self.json_mode {
            let change = serde_json::json!({
                "type": "resource_change",
                "action": action,
                "resource_type": resource_type,
                "resource_name": resource_name,
                "details": details
            });
            println!("{}", serde_json::to_string(&change).unwrap());
            return;
        }

        let (symbol, color) = match action {
            "create" => ("+", colored::Color::Green),
            "update" | "change" => ("~", colored::Color::Yellow),
            "delete" | "destroy" => ("-", colored::Color::Red),
            "replace" => ("-/+", colored::Color::Cyan),
            _ => ("?", colored::Color::White),
        };

        let resource_id = format!("{}.{}", resource_type, resource_name);

        if self.use_color {
            println!(
                "  {} {}",
                symbol.color(color).bold(),
                resource_id.color(color)
            );
        } else {
            println!("  {} {}", symbol, resource_id);
        }

        if let Some(detail) = details {
            if self.use_color {
                println!("      {}", detail.bright_black());
            } else {
                println!("      {}", detail);
            }
        }
    }

    /// Print a field-level change within a resource
    pub fn plan_field_change(
        &self,
        field: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        forces_replacement: bool,
    ) {
        if self.json_mode {
            return;
        }

        let old_str = old_value.unwrap_or("(not set)");
        let new_str = new_value.unwrap_or("(not set)");

        let force_marker = if forces_replacement {
            " # forces replacement"
        } else {
            ""
        };

        if self.use_color {
            let arrow = "→".bright_black();
            println!(
                "      {} = {} {} {}{}",
                field.white(),
                old_str.red(),
                arrow,
                new_str.green(),
                force_marker.yellow()
            );
        } else {
            println!(
                "      {} = {} -> {}{}",
                field, old_str, new_str, force_marker
            );
        }
    }

    /// Print a plan summary (Terraform-style)
    pub fn plan_summary(&self, to_add: usize, to_change: usize, to_destroy: usize) {
        if self.json_mode {
            let summary = serde_json::json!({
                "type": "plan_summary",
                "to_add": to_add,
                "to_change": to_change,
                "to_destroy": to_destroy
            });
            println!("{}", serde_json::to_string(&summary).unwrap());
            return;
        }

        println!();
        if self.use_color {
            println!("{}", "─".repeat(78).bright_black());

            let total = to_add + to_change + to_destroy;
            if total == 0 {
                println!(
                    "{}",
                    "No changes. Your infrastructure matches the configuration.".green()
                );
            } else {
                println!(
                    "Plan: {} to add, {} to change, {} to destroy.",
                    to_add.to_string().green().bold(),
                    to_change.to_string().yellow().bold(),
                    to_destroy.to_string().red().bold()
                );
            }
        } else {
            println!("{}", "-".repeat(78));

            let total = to_add + to_change + to_destroy;
            if total == 0 {
                println!("No changes. Your infrastructure matches the configuration.");
            } else {
                println!(
                    "Plan: {} to add, {} to change, {} to destroy.",
                    to_add, to_change, to_destroy
                );
            }
        }
    }

    /// Print a plan note/hint
    pub fn plan_note(&self, message: &str) {
        if self.json_mode {
            return;
        }

        if self.use_color {
            println!("\n{}", message.bright_black().italic());
        } else {
            println!("\n{}", message);
        }
    }

    /// Flush stdout
    pub fn flush(&self) {
        let _ = io::stdout().flush();
    }
}

/// Helper to format a centered banner with consistent visual width
fn format_banner(message: &str, width: usize) -> String {
    let msg_width = measure_text_width(message);
    // 2 spaces around message
    let available = width.saturating_sub(msg_width + 2);
    let padding = available / 2;
    let left_pad = "━".repeat(padding);
    // Ensure total width is exactly `width`
    let used = padding + msg_width + 2;
    let right_pad_len = width.saturating_sub(used);
    let right_pad = "━".repeat(right_pad_len);
    format!("{} {} {}", left_pad, message, right_pad)
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
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if total_secs >= 3600 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        if mins == 0 && secs == 0 {
            format!("{}h", hours)
        } else if secs == 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h {}m {}s", hours, mins, secs)
        }
    } else if total_secs >= 60 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        if secs == 0 {
            format!("{}m", mins)
        } else {
            format!("{}m {}s", mins, secs)
        }
    } else if total_secs > 0 {
        use std::fmt::Write;
        let mut s = String::with_capacity(16);
        write!(s, "{}.{:03}", total_secs, millis).unwrap();
        // Remove trailing zeros
        while s.ends_with('0') {
            s.pop();
        }
        // Remove trailing dot
        if s.ends_with('.') {
            s.pop();
        }
        s.push('s');
        s
    } else if millis > 0 {
        format!("{}ms", millis)
    } else {
        let micros = duration.subsec_micros();
        if micros > 0 {
            format!("{}µs", micros)
        } else {
            "0ms".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_width_calculation() {
        // Basic ASCII
        assert_eq!(measure_text_width("test"), 4);
        assert_eq!("test".len(), 4);

        // Emoji (rocket is 4 bytes, 2 columns wide)
        let rocket = "🚀";
        assert_eq!(rocket.len(), 4);
        assert_eq!(measure_text_width(rocket), 2);

        // Mixed
        let mixed = "Start 🚀";
        assert_eq!(mixed.len(), 5 + 1 + 4); // 10 bytes
        assert_eq!(measure_text_width(mixed), 5 + 1 + 2); // 8 columns

        // Verify that our fix logic would work correctly
        // Banner line length should be visual width + 4
        let title = "Start 🚀";
        let width = measure_text_width(title);
        let line = "─".repeat(width + 4);
        assert_eq!(measure_text_width(&line), 12); // 8 + 4 = 12

        // The text line "  Start 🚀  "
        // 2 spaces + 8 visual width + 2 spaces = 12 visual width
        // So the line matches the text visual width
    }

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

        // Verify new icons
        assert!(TaskStatus::Changed.colored_string().contains("✎"));
        assert!(TaskStatus::Skipped.colored_string().contains("↷"));
        assert!(TaskStatus::Unreachable.colored_string().contains("✘"));
        assert!(TaskStatus::Ignored.colored_string().contains("⊘"));
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
        assert_eq!(format_duration(Duration::from_micros(500)), "500µs");
        assert_eq!(format_duration(Duration::from_micros(5)), "5µs");
        assert_eq!(format_duration(Duration::ZERO), "0ms");
        assert_eq!(format_duration(Duration::from_secs(5)), "5s");
        assert_eq!(format_duration(Duration::from_millis(5500)), "5.5s");
        assert_eq!(format_duration(Duration::from_millis(5050)), "5.05s");
        assert_eq!(format_duration(Duration::from_millis(5005)), "5.005s");
        assert_eq!(format_duration(Duration::from_secs(60)), "1m");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
        assert_eq!(format_duration(Duration::from_secs(3660)), "1h 1m");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m 5s");
    }

    #[test]
    fn test_format_banner() {
        // Test with a short message
        let msg = "TEST";
        let width = 60;
        let banner = format_banner(msg, width);
        // Visual width should be 60
        assert_eq!(measure_text_width(&banner), 60);
        assert!(banner.contains("TEST"));

        // Test with multi-byte emoji
        let msg_emoji = "✔ SUCCESS";
        let banner_emoji = format_banner(msg_emoji, width);

        // Visual width should still be 60 despite multi-byte characters
        assert_eq!(measure_text_width(&banner_emoji), 60);
        assert!(banner_emoji.contains("SUCCESS"));

        // Test with very long message (should minimize padding)
        let msg_long = "A very long message that takes up most of the space";
        let banner_long = format_banner(msg_long, width);
        assert_eq!(measure_text_width(&banner_long), 60);
    }

    #[test]
    fn test_plan_output_methods_no_panic() {
        // Test that plan output methods don't panic in non-JSON mode
        let formatter = OutputFormatter::new(false, false, 1);

        // These should not panic and should produce no errors
        formatter.plan_header("Test Plan");
        formatter.plan_resource_change("create", "file", "/tmp/test", None);
        formatter.plan_resource_change("update", "package", "nginx", Some("version changed"));
        formatter.plan_resource_change("delete", "service", "old-service", None);
        formatter.plan_resource_change("replace", "user", "admin", None);
        formatter.plan_field_change("mode", Some("0644"), Some("0755"), false);
        formatter.plan_field_change("owner", None, Some("root"), true);
        formatter.plan_summary(2, 1, 1);
        formatter.plan_note("This is a test note");
    }

    #[test]
    fn test_plan_output_json_mode() {
        // We can't easily capture output in JSON mode without more infrastructure,
        // but we can at least verify the methods don't panic
        let formatter = OutputFormatter::new(false, true, 1);

        // Test resource change JSON output
        formatter.plan_resource_change("create", "file", "/tmp/test", Some("test details"));
        formatter.plan_summary(1, 2, 3);

        // Methods that don't output in JSON mode should also not panic
        formatter.plan_header("Test");
        formatter.plan_field_change("test", Some("old"), Some("new"), false);
        formatter.plan_note("Note");
    }
}
