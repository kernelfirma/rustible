//! Progress tracking module for Rustible
//!
//! Provides progress bars with ETA for task execution.

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Progress tracker for playbook execution
pub struct PlaybookProgress {
    multi: Arc<MultiProgress>,
    overall_bar: ProgressBar,
    task_bar: Option<ProgressBar>,
    host_bars: Vec<ProgressBar>,
    start_time: Instant,
    total_tasks: u64,
    completed_tasks: u64,
    json_mode: bool,
}

impl PlaybookProgress {
    /// Create a new progress tracker
    pub fn new(total_tasks: u64, _total_hosts: u64, json_mode: bool) -> Self {
        let multi = Arc::new(MultiProgress::new());

        if json_mode {
            // In JSON mode, hide all progress bars
            multi.set_draw_target(ProgressDrawTarget::hidden());
        }

        // Overall progress bar
        let overall_style = ProgressStyle::default_bar()
            .template("{spinner:.green} {prefix:.bold.dim} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} tasks ({eta})")
            .unwrap()
            .progress_chars("=>-");

        let overall_bar = multi.add(ProgressBar::new(total_tasks));
        overall_bar.set_style(overall_style);
        overall_bar.set_prefix("Playbook");

        // Enable steady tick for spinner animation
        if !json_mode {
            overall_bar.enable_steady_tick(Duration::from_millis(100));
        }

        Self {
            multi,
            overall_bar,
            task_bar: None,
            host_bars: Vec::new(),
            start_time: Instant::now(),
            total_tasks,
            completed_tasks: 0,
            json_mode,
        }
    }

    /// Start a new task
    pub fn start_task(&mut self, task_name: &str, host_count: u64) {
        if self.json_mode {
            return;
        }

        // Remove previous task bar if exists
        if let Some(bar) = self.task_bar.take() {
            bar.finish_and_clear();
        }

        // Clear previous host bars
        for bar in self.host_bars.drain(..) {
            bar.finish_and_clear();
        }

        // Create new task bar
        let task_style = ProgressStyle::default_bar()
            .template("  {spinner:.yellow} {prefix:.bold} [{bar:30.yellow/red}] {pos}/{len} hosts")
            .unwrap()
            .progress_chars("#>-");

        let task_bar = self.multi.add(ProgressBar::new(host_count));
        task_bar.set_style(task_style);
        task_bar.set_prefix(truncate_string(task_name, 40));
        task_bar.enable_steady_tick(Duration::from_millis(100));

        self.task_bar = Some(task_bar);
    }

    /// Add a host progress bar for parallel execution
    pub fn add_host_bar(&mut self, host: &str) -> ProgressBar {
        if self.json_mode {
            let bar = ProgressBar::hidden();
            return bar;
        }

        let host_style = ProgressStyle::default_spinner()
            .template("    {spinner:.cyan} {prefix:.dim} {msg}")
            .unwrap();

        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(host_style);
        bar.set_prefix(truncate_string(host, 20));
        bar.enable_steady_tick(Duration::from_millis(100));

        self.host_bars.push(bar.clone());
        bar
    }

    /// Mark a host as complete within current task
    pub fn complete_host(&self, status: HostStatus) {
        if self.json_mode {
            return;
        }

        if let Some(ref task_bar) = self.task_bar {
            task_bar.inc(1);
        }

        // Update message with colored status
        let _status_str = match status {
            HostStatus::Ok => "ok".green().to_string(),
            HostStatus::Changed => "changed".yellow().to_string(),
            HostStatus::Failed => "failed".red().bold().to_string(),
            HostStatus::Skipped => "skipped".cyan().to_string(),
            HostStatus::Unreachable => "unreachable".red().bold().to_string(),
        };
    }

    /// Complete the current task
    pub fn complete_task(&mut self) {
        self.completed_tasks += 1;
        self.overall_bar.inc(1);

        if let Some(bar) = self.task_bar.take() {
            bar.finish_and_clear();
        }

        // Clear host bars
        for bar in self.host_bars.drain(..) {
            bar.finish_and_clear();
        }
    }

    /// Skip a task
    pub fn skip_task(&mut self, reason: &str) {
        if !self.json_mode {
            self.overall_bar
                .set_message(format!("skipped: {}", reason).cyan().to_string());
        }
        self.overall_bar.inc(1);
        self.completed_tasks += 1;
    }

    /// Finish all progress bars
    pub fn finish(&self) {
        // Finish any remaining task bar
        if let Some(ref bar) = self.task_bar {
            bar.finish_and_clear();
        }

        // Finish overall bar with completion message
        let elapsed = self.start_time.elapsed();
        let message = format!(
            "Completed {} tasks in {}",
            self.total_tasks,
            format_duration(elapsed)
        );
        self.overall_bar.finish_with_message(message);
    }

    /// Finish with failure
    pub fn finish_with_error(&self, error: &str) {
        if let Some(ref bar) = self.task_bar {
            bar.abandon_with_message(format!("Failed: {}", error).red().to_string());
        }
        self.overall_bar
            .abandon_with_message(format!("Failed: {}", error).red().to_string());
    }

    /// Get the multi-progress for external use
    pub fn multi_progress(&self) -> Arc<MultiProgress> {
        self.multi.clone()
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Calculate ETA based on current progress
    pub fn eta(&self) -> Option<Duration> {
        if self.completed_tasks == 0 {
            return None;
        }

        let elapsed = self.start_time.elapsed();
        let avg_time_per_task = elapsed.as_secs_f64() / self.completed_tasks as f64;
        let remaining_tasks = self.total_tasks.saturating_sub(self.completed_tasks);
        let eta_secs = avg_time_per_task * remaining_tasks as f64;

        Some(Duration::from_secs_f64(eta_secs))
    }

    /// Get a summary of progress
    pub fn summary(&self) -> ProgressSummary {
        ProgressSummary {
            total_tasks: self.total_tasks,
            completed_tasks: self.completed_tasks,
            elapsed: self.start_time.elapsed(),
            eta: self.eta(),
        }
    }
}

/// Host execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    Ok,
    Changed,
    Failed,
    Skipped,
    Unreachable,
}

/// Progress summary
#[derive(Debug, Clone)]
pub struct ProgressSummary {
    pub total_tasks: u64,
    pub completed_tasks: u64,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
}

impl ProgressSummary {
    /// Get completion percentage
    pub fn percentage(&self) -> f64 {
        if self.total_tasks == 0 {
            return 100.0;
        }
        (self.completed_tasks as f64 / self.total_tasks as f64) * 100.0
    }

    /// Get tasks per second rate
    pub fn tasks_per_second(&self) -> f64 {
        if self.elapsed.as_secs_f64() == 0.0 {
            return 0.0;
        }
        self.completed_tasks as f64 / self.elapsed.as_secs_f64()
    }
}

/// Create a spinner for indeterminate progress
pub fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(Duration::from_millis(100));
    spinner
}

/// Create a download/transfer progress bar
pub fn create_transfer_bar(total: u64, prefix: &str) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("{prefix:.bold} [{bar:40.green/white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("=>-"),
    );
    bar.set_prefix(prefix.to_string());
    bar
}

/// Create a simple counter progress bar
pub fn create_counter_bar(total: u64, prefix: &str) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("{prefix:.bold} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")
            .unwrap()
            .progress_chars("=>-"),
    );
    bar.set_prefix(prefix.to_string());
    bar
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

/// Truncate a string to a maximum length
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_secs(5)), "5.000s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m 5s");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(
            truncate_string("this is a very long string", 10),
            "this is..."
        );
    }

    #[test]
    fn test_progress_summary() {
        let summary = ProgressSummary {
            total_tasks: 100,
            completed_tasks: 50,
            elapsed: Duration::from_secs(10),
            eta: Some(Duration::from_secs(10)),
        };
        assert_eq!(summary.percentage(), 50.0);
        assert_eq!(summary.tasks_per_second(), 5.0);
    }

    #[test]
    fn test_playbook_progress_json_mode() {
        let progress = PlaybookProgress::new(10, 2, true);
        // In JSON mode, operations should be no-ops
        assert!(progress.json_mode);
    }
}
