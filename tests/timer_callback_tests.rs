//! Comprehensive tests for timer callback plugins: TimerCallback and ProfileTasksCallback.
//!
//! This test suite covers:
//! 1. Time tracking accuracy for playbooks, plays, and tasks
//! 2. Task duration recording and aggregation
//! 3. Summary generation with proper formatting
//! 4. Sorting by duration (ascending and descending)
//! 5. Concurrent task timing
//! 6. Edge cases (zero duration, very long durations)
//! 7. Thread safety for parallel execution
//!
//! Note: These tests implement TimerCallback and ProfileTasksCallback as reference
//! implementations demonstrating the expected behavior for timer callback plugins.

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// TimerCallback Implementation
// ============================================================================

/// A callback that tracks timing for playbook execution at all levels.
///
/// Tracks:
/// - Total playbook execution time
/// - Individual play execution times
/// - Individual task execution times per host
#[derive(Debug)]
pub struct TimerCallback {
    /// Configuration
    config: TimerConfig,
    /// Start time for the current playbook
    playbook_start: RwLock<Option<Instant>>,
    /// Start times for plays (keyed by play name)
    play_starts: RwLock<HashMap<String, Instant>>,
    /// Play durations
    play_durations: RwLock<HashMap<String, Duration>>,
    /// Start times for tasks (keyed by "task_name:host")
    task_starts: RwLock<HashMap<String, Instant>>,
    /// All recorded task timings
    task_timings: RwLock<Vec<TaskTiming>>,
    /// Total number of tasks executed
    total_tasks: AtomicU64,
    /// Total duration in microseconds
    total_duration_us: AtomicU64,
}

/// Configuration for TimerCallback
#[derive(Debug, Clone)]
pub struct TimerConfig {
    /// Show timing after each task completes
    pub show_per_task: bool,
    /// Show summary of slowest tasks at end
    pub show_summary: bool,
    /// Number of slowest tasks to show in summary
    pub top_slowest: usize,
    /// Only show tasks that took longer than this threshold (seconds)
    pub threshold_secs: f64,
    /// Show play-level timing
    pub show_play_timing: bool,
    /// Show playbook-level timing
    pub show_playbook_timing: bool,
    /// Use colors in output
    pub use_colors: bool,
    /// Show timing in human-readable format
    pub human_readable: bool,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            show_per_task: true,
            show_summary: true,
            top_slowest: 10,
            threshold_secs: 0.0,
            show_play_timing: true,
            show_playbook_timing: true,
            use_colors: true,
            human_readable: true,
        }
    }
}

/// Entry for tracking a single task's timing
#[derive(Debug, Clone)]
pub struct TaskTiming {
    /// Task name
    pub task_name: String,
    /// Host the task ran on
    pub host: String,
    /// Duration of the task execution
    pub duration: Duration,
    /// Whether the task succeeded
    pub success: bool,
    /// Whether the task made changes
    pub changed: bool,
}

impl Default for TimerCallback {
    fn default() -> Self {
        Self::new(TimerConfig::default())
    }
}

impl TimerCallback {
    /// Create a new timer callback with the given configuration
    pub fn new(config: TimerConfig) -> Self {
        Self {
            config,
            playbook_start: RwLock::new(None),
            play_starts: RwLock::new(HashMap::new()),
            play_durations: RwLock::new(HashMap::new()),
            task_starts: RwLock::new(HashMap::new()),
            task_timings: RwLock::new(Vec::new()),
            total_tasks: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
        }
    }

    /// Create a timer callback with summary only
    pub fn summary_only() -> Self {
        Self::new(TimerConfig {
            show_per_task: false,
            show_summary: true,
            ..Default::default()
        })
    }

    /// Create a verbose timer
    pub fn verbose() -> Self {
        Self::new(TimerConfig {
            show_per_task: true,
            show_summary: true,
            top_slowest: 20,
            ..Default::default()
        })
    }

    /// Get all collected task timings
    pub fn get_timings(&self) -> Vec<TaskTiming> {
        self.task_timings.read().clone()
    }

    /// Get the top N slowest tasks
    pub fn get_slowest_tasks(&self, n: usize) -> Vec<TaskTiming> {
        let mut timings = self.task_timings.read().clone();
        timings.sort_by(|a, b| b.duration.cmp(&a.duration));
        timings.into_iter().take(n).collect()
    }

    /// Get total execution time across all tasks
    pub fn get_total_duration(&self) -> Duration {
        Duration::from_micros(self.total_duration_us.load(Ordering::Relaxed))
    }

    /// Get total number of tasks executed
    pub fn get_total_tasks(&self) -> u64 {
        self.total_tasks.load(Ordering::Relaxed)
    }

    /// Get average task duration
    pub fn get_average_duration(&self) -> Duration {
        let total = self.get_total_duration();
        let count = self.get_total_tasks();
        if count == 0 {
            Duration::ZERO
        } else {
            total / count as u32
        }
    }
}

impl Clone for TimerCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            playbook_start: RwLock::new(None),
            play_starts: RwLock::new(HashMap::new()),
            play_durations: RwLock::new(HashMap::new()),
            task_starts: RwLock::new(HashMap::new()),
            task_timings: RwLock::new(Vec::new()),
            total_tasks: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl ExecutionCallback for TimerCallback {
    async fn on_playbook_start(&self, _name: &str) {
        *self.playbook_start.write() = Some(Instant::now());
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        // Playbook completed
    }

    async fn on_play_start(&self, name: &str, _hosts: &[String]) {
        self.play_starts
            .write()
            .insert(name.to_string(), Instant::now());
    }

    async fn on_play_end(&self, name: &str, _success: bool) {
        if let Some(start) = self.play_starts.read().get(name) {
            self.play_durations
                .write()
                .insert(name.to_string(), start.elapsed());
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let key = format!("{}:{}", name, host);
        self.task_starts.write().insert(key, Instant::now());
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let duration = result.duration;

        let timing = TaskTiming {
            task_name: result.task_name.clone(),
            host: result.host.clone(),
            duration,
            success: result.result.success,
            changed: result.result.changed,
        };

        self.total_tasks.fetch_add(1, Ordering::Relaxed);
        self.total_duration_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.task_timings.write().push(timing);
    }

    async fn on_handler_triggered(&self, _name: &str) {}
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {}
}

/// Builder for TimerCallback
#[derive(Debug, Default)]
pub struct TimerCallbackBuilder {
    config: TimerConfig,
}

impl TimerCallbackBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show_per_task(mut self, enabled: bool) -> Self {
        self.config.show_per_task = enabled;
        self
    }

    pub fn show_summary(mut self, enabled: bool) -> Self {
        self.config.show_summary = enabled;
        self
    }

    pub fn top_slowest(mut self, count: usize) -> Self {
        self.config.top_slowest = count;
        self
    }

    pub fn threshold_secs(mut self, seconds: f64) -> Self {
        self.config.threshold_secs = seconds;
        self
    }

    pub fn show_play_timing(mut self, enabled: bool) -> Self {
        self.config.show_play_timing = enabled;
        self
    }

    pub fn show_playbook_timing(mut self, enabled: bool) -> Self {
        self.config.show_playbook_timing = enabled;
        self
    }

    pub fn use_colors(mut self, enabled: bool) -> Self {
        self.config.use_colors = enabled;
        self
    }

    pub fn human_readable(mut self, enabled: bool) -> Self {
        self.config.human_readable = enabled;
        self
    }

    pub fn build(self) -> TimerCallback {
        TimerCallback::new(self.config)
    }
}

// ============================================================================
// ProfileTasksCallback Implementation
// ============================================================================

/// Configuration for the profile tasks callback
#[derive(Debug, Clone)]
pub struct ProfileTasksConfig {
    /// Threshold in seconds above which a task is considered slow
    pub slow_threshold_secs: f64,
    /// Threshold in seconds above which a task is considered a bottleneck
    pub bottleneck_threshold_secs: f64,
    /// Maximum number of tasks to show in the summary
    pub top_tasks_count: usize,
    /// Whether to show per-host breakdown
    pub show_per_host: bool,
    /// Whether to include skipped tasks in profiling
    pub include_skipped: bool,
    /// Sort order for the summary
    pub sort_order: ProfileSortOrder,
}

impl Default for ProfileTasksConfig {
    fn default() -> Self {
        Self {
            slow_threshold_secs: 10.0,
            bottleneck_threshold_secs: 30.0,
            top_tasks_count: 20,
            show_per_host: true,
            include_skipped: false,
            sort_order: ProfileSortOrder::Duration,
        }
    }
}

/// Sort order for the profiling summary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSortOrder {
    Duration,
    Name,
    Chronological,
}

/// Timing information for a single task execution
#[derive(Debug, Clone)]
pub struct ProfileTaskTiming {
    pub task_name: String,
    pub host: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub duration: Option<Duration>,
    pub success: bool,
    pub skipped: bool,
    pub changed: bool,
    pub order: u64,
}

impl ProfileTaskTiming {
    pub fn new(task_name: String, host: String, order: u64) -> Self {
        Self {
            task_name,
            host,
            start_time: Instant::now(),
            end_time: None,
            duration: None,
            success: true,
            skipped: false,
            changed: false,
            order,
        }
    }

    pub fn complete(&mut self, success: bool, skipped: bool, changed: bool) {
        let end = Instant::now();
        self.end_time = Some(end);
        self.duration = Some(end.duration_since(self.start_time));
        self.success = success;
        self.skipped = skipped;
        self.changed = changed;
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration.map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }
}

/// Aggregated timing for a task across all hosts
#[derive(Debug, Clone, Default)]
pub struct AggregatedTaskTiming {
    pub task_name: String,
    pub total_duration: Duration,
    pub min_duration: Option<Duration>,
    pub max_duration: Option<Duration>,
    pub host_count: usize,
    pub host_timings: Vec<(String, Duration)>,
}

impl AggregatedTaskTiming {
    pub fn average_duration(&self) -> Duration {
        if self.host_count > 0 {
            self.total_duration / self.host_count as u32
        } else {
            Duration::ZERO
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct TaskKey {
    task_name: String,
    host: String,
}

/// Profile Tasks Callback Plugin
#[derive(Debug)]
pub struct ProfileTasksCallback {
    config: ProfileTasksConfig,
    running_tasks: RwLock<HashMap<TaskKey, ProfileTaskTiming>>,
    completed_tasks: RwLock<Vec<ProfileTaskTiming>>,
    playbook_start: RwLock<Option<Instant>>,
    play_starts: RwLock<HashMap<String, Instant>>,
    task_counter: AtomicU64,
}

impl Default for ProfileTasksCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileTasksCallback {
    pub fn new() -> Self {
        Self::with_config(ProfileTasksConfig::default())
    }

    pub fn with_config(config: ProfileTasksConfig) -> Self {
        Self {
            config,
            running_tasks: RwLock::new(HashMap::new()),
            completed_tasks: RwLock::new(Vec::new()),
            playbook_start: RwLock::new(None),
            play_starts: RwLock::new(HashMap::new()),
            task_counter: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.running_tasks.write().clear();
        self.completed_tasks.write().clear();
        *self.playbook_start.write() = None;
        self.play_starts.write().clear();
        self.task_counter.store(0, Ordering::SeqCst);
    }

    pub fn get_completed_tasks(&self) -> Vec<ProfileTaskTiming> {
        self.completed_tasks.read().clone()
    }

    pub fn get_aggregated_timings(&self) -> Vec<AggregatedTaskTiming> {
        let completed = self.completed_tasks.read();
        let mut aggregated: HashMap<String, AggregatedTaskTiming> = HashMap::new();

        for timing in completed.iter() {
            if timing.skipped && !self.config.include_skipped {
                continue;
            }

            let duration = timing.duration.unwrap_or(Duration::ZERO);

            let entry = aggregated
                .entry(timing.task_name.clone())
                .or_insert_with(|| AggregatedTaskTiming {
                    task_name: timing.task_name.clone(),
                    ..Default::default()
                });

            entry.total_duration += duration;
            entry.host_count += 1;
            entry.host_timings.push((timing.host.clone(), duration));

            entry.min_duration = Some(
                entry
                    .min_duration
                    .map(|min| min.min(duration))
                    .unwrap_or(duration),
            );

            entry.max_duration = Some(
                entry
                    .max_duration
                    .map(|max| max.max(duration))
                    .unwrap_or(duration),
            );
        }

        aggregated.into_values().collect()
    }

    fn get_sorted_timings(&self) -> Vec<ProfileTaskTiming> {
        let mut timings: Vec<_> = self.completed_tasks.read().clone();

        if !self.config.include_skipped {
            timings.retain(|t| !t.skipped);
        }

        match self.config.sort_order {
            ProfileSortOrder::Duration => {
                timings.sort_by(|a, b| {
                    b.duration
                        .unwrap_or(Duration::ZERO)
                        .cmp(&a.duration.unwrap_or(Duration::ZERO))
                });
            }
            ProfileSortOrder::Name => {
                timings.sort_by(|a, b| a.task_name.cmp(&b.task_name));
            }
            ProfileSortOrder::Chronological => {
                timings.sort_by_key(|t| t.order);
            }
        }

        timings
    }

    pub fn identify_bottlenecks(&self) -> Vec<ProfileTaskTiming> {
        self.completed_tasks
            .read()
            .iter()
            .filter(|t| !t.skipped && t.duration_secs() >= self.config.bottleneck_threshold_secs)
            .cloned()
            .collect()
    }

    pub fn total_execution_time(&self) -> Duration {
        self.playbook_start
            .read()
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    pub fn average_task_duration(&self) -> Duration {
        let completed = self.completed_tasks.read();
        let valid_tasks: Vec<_> = completed
            .iter()
            .filter(|t| !t.skipped && t.duration.is_some())
            .collect();

        if valid_tasks.is_empty() {
            return Duration::ZERO;
        }

        let total: Duration = valid_tasks
            .iter()
            .map(|t| t.duration.unwrap_or(Duration::ZERO))
            .sum();

        total / valid_tasks.len() as u32
    }

    pub fn slow_task_count(&self) -> usize {
        self.completed_tasks
            .read()
            .iter()
            .filter(|t| !t.skipped && t.duration_secs() >= self.config.slow_threshold_secs)
            .count()
    }

    pub fn to_json_report(&self) -> serde_json::Value {
        let sorted_timings = self.get_sorted_timings();
        let bottlenecks = self.identify_bottlenecks();

        serde_json::json!({
            "summary": {
                "total_execution_time_secs": self.total_execution_time().as_secs_f64(),
                "average_task_time_secs": self.average_task_duration().as_secs_f64(),
                "total_tasks": sorted_timings.len(),
                "slow_tasks": self.slow_task_count(),
                "bottlenecks": bottlenecks.len(),
            },
            "tasks": sorted_timings.iter().map(|t| {
                serde_json::json!({
                    "name": t.task_name,
                    "host": t.host,
                    "duration_secs": t.duration_secs(),
                    "success": t.success,
                    "skipped": t.skipped,
                    "changed": t.changed,
                    "order": t.order,
                })
            }).collect::<Vec<_>>(),
            "bottlenecks": bottlenecks.iter().map(|t| {
                serde_json::json!({
                    "name": t.task_name,
                    "host": t.host,
                    "duration_secs": t.duration_secs(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

#[async_trait]
impl ExecutionCallback for ProfileTasksCallback {
    async fn on_playbook_start(&self, _name: &str) {
        *self.playbook_start.write() = Some(Instant::now());
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {}

    async fn on_play_start(&self, name: &str, _hosts: &[String]) {
        self.play_starts
            .write()
            .insert(name.to_string(), Instant::now());
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {}

    async fn on_task_start(&self, name: &str, host: &str) {
        let order = self.task_counter.fetch_add(1, Ordering::SeqCst);
        let key = TaskKey {
            task_name: name.to_string(),
            host: host.to_string(),
        };
        let timing = ProfileTaskTiming::new(name.to_string(), host.to_string(), order);
        self.running_tasks.write().insert(key, timing);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let key = TaskKey {
            task_name: result.task_name.clone(),
            host: result.host.clone(),
        };

        if let Some(mut timing) = self.running_tasks.write().remove(&key) {
            timing.complete(
                result.result.success,
                result.result.skipped,
                result.result.changed,
            );

            if result.duration > Duration::ZERO {
                timing.duration = Some(result.duration);
            }

            self.completed_tasks.write().push(timing);
        } else {
            let order = self.task_counter.fetch_add(1, Ordering::SeqCst);
            let timing = ProfileTaskTiming {
                task_name: result.task_name.clone(),
                host: result.host.clone(),
                start_time: Instant::now(),
                end_time: Some(Instant::now()),
                duration: Some(result.duration),
                success: result.result.success,
                skipped: result.result.skipped,
                changed: result.result.changed,
                order,
            };
            self.completed_tasks.write().push(timing);
        }
    }

    async fn on_handler_triggered(&self, _name: &str) {}
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {}
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_result(task_name: &str, host: &str, duration_ms: u64, success: bool) -> ExecutionResult {
    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: if success {
            ModuleResult::ok("OK")
        } else {
            ModuleResult::failed("Failed")
        },
        duration: Duration::from_millis(duration_ms),
        notify: vec![],
    }
}

fn create_changed_result(task_name: &str, host: &str, duration_ms: u64) -> ExecutionResult {
    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(duration_ms),
        notify: vec![],
    }
}

fn create_skipped_result(task_name: &str, host: &str) -> ExecutionResult {
    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: ModuleResult::skipped("Skipped"),
        duration: Duration::ZERO,
        notify: vec![],
    }
}

// ============================================================================
// TimerCallback Tests
// ============================================================================

#[cfg(test)]
mod timer_callback_tests {
    use super::*;

    // ========================================================================
    // Test 1: Time Tracking Accuracy
    // ========================================================================

    #[tokio::test]
    async fn test_timer_callback_creation_with_defaults() {
        let timer = TimerCallback::default();
        assert_eq!(timer.get_total_tasks(), 0);
        assert_eq!(timer.get_total_duration(), Duration::ZERO);
    }

    #[tokio::test]
    async fn test_timer_callback_builder_configuration() {
        let timer = TimerCallbackBuilder::new()
            .show_per_task(false)
            .show_summary(true)
            .top_slowest(5)
            .threshold_secs(1.0)
            .use_colors(false)
            .human_readable(true)
            .show_play_timing(true)
            .show_playbook_timing(true)
            .build();

        assert_eq!(timer.get_total_tasks(), 0);
    }

    #[tokio::test]
    async fn test_timer_tracks_task_duration_from_result() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("test_task", "host1", 150, true);
        timer.on_task_start("test_task", "host1").await;
        timer.on_task_complete(&result).await;

        let timings = timer.get_timings();
        assert_eq!(timings.len(), 1);
        assert_eq!(timings[0].task_name, "test_task");
        assert_eq!(timings[0].host, "host1");
        assert_eq!(timings[0].duration, Duration::from_millis(150));
        assert!(timings[0].success);
    }

    #[tokio::test]
    async fn test_timer_tracks_multiple_tasks() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let tasks = vec![
            ("task1", "host1", 100u64),
            ("task2", "host1", 200),
            ("task1", "host2", 150),
        ];

        for (task, host, duration) in &tasks {
            timer.on_task_start(task, host).await;
            let result = create_result(task, host, *duration, true);
            timer.on_task_complete(&result).await;
        }

        assert_eq!(timer.get_total_tasks(), 3);
        let timings = timer.get_timings();
        assert_eq!(timings.len(), 3);
    }

    #[tokio::test]
    async fn test_timer_handles_zero_duration() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("instant_task", "host1", 0, true);
        timer.on_task_start("instant_task", "host1").await;
        timer.on_task_complete(&result).await;

        let timings = timer.get_timings();
        assert_eq!(timings[0].duration, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_timer_handles_long_duration() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("long_task", "host1", 3_600_000, true);
        timer.on_task_start("long_task", "host1").await;
        timer.on_task_complete(&result).await;

        let timings = timer.get_timings();
        assert_eq!(timings[0].duration, Duration::from_secs(3600));
    }

    // ========================================================================
    // Test 2: Task Duration Recording
    // ========================================================================

    #[tokio::test]
    async fn test_timer_total_task_count() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        for i in 0..10 {
            let result = create_result(&format!("task{}", i), "host1", 10, true);
            timer.on_task_start(&format!("task{}", i), "host1").await;
            timer.on_task_complete(&result).await;
        }

        assert_eq!(timer.get_total_tasks(), 10);
    }

    #[tokio::test]
    async fn test_timer_total_duration_calculation() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let durations = vec![100u64, 200, 300];
        for (i, duration) in durations.iter().enumerate() {
            let result = create_result(&format!("task{}", i), "host1", *duration, true);
            timer.on_task_start(&format!("task{}", i), "host1").await;
            timer.on_task_complete(&result).await;
        }

        assert_eq!(timer.get_total_duration(), Duration::from_millis(600));
    }

    #[tokio::test]
    async fn test_timer_average_duration_calculation() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        for i in 1..=3 {
            let result = create_result(&format!("task{}", i), "host1", i * 1000, true);
            timer.on_task_start(&format!("task{}", i), "host1").await;
            timer.on_task_complete(&result).await;
        }

        let avg = timer.get_average_duration();
        assert_eq!(avg, Duration::from_secs(2));
    }

    #[tokio::test]
    async fn test_timer_tracks_changed_status() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_changed_result("change_task", "host1", 100);
        timer.on_task_start("change_task", "host1").await;
        timer.on_task_complete(&result).await;

        let timings = timer.get_timings();
        assert!(timings[0].changed);
        assert!(timings[0].success);
    }

    #[tokio::test]
    async fn test_timer_tracks_failed_status() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("failed_task", "host1", 100, false);
        timer.on_task_start("failed_task", "host1").await;
        timer.on_task_complete(&result).await;

        let timings = timer.get_timings();
        assert!(!timings[0].success);
    }

    // ========================================================================
    // Test 3: Summary Generation (Sorting by Duration)
    // ========================================================================

    #[tokio::test]
    async fn test_timer_get_slowest_tasks() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let tasks = vec![("fast", 50u64), ("slow", 500), ("medium", 200)];

        for (task, duration) in &tasks {
            let result = create_result(task, "host1", *duration, true);
            timer.on_task_start(task, "host1").await;
            timer.on_task_complete(&result).await;
        }

        let slowest = timer.get_slowest_tasks(2);
        assert_eq!(slowest.len(), 2);
        assert_eq!(slowest[0].task_name, "slow");
        assert_eq!(slowest[1].task_name, "medium");
    }

    #[tokio::test]
    async fn test_timer_get_all_slowest_when_fewer_tasks() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("only_task", "host1", 100, true);
        timer.on_task_start("only_task", "host1").await;
        timer.on_task_complete(&result).await;

        let slowest = timer.get_slowest_tasks(5);
        assert_eq!(slowest.len(), 1);
    }

    #[tokio::test]
    async fn test_timer_empty_slowest_when_no_tasks() {
        let timer = TimerCallback::default();
        let slowest = timer.get_slowest_tasks(5);
        assert!(slowest.is_empty());
    }

    #[tokio::test]
    async fn test_timer_clone_starts_fresh() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let result = create_result("task1", "host1", 100, true);
        timer.on_task_start("task1", "host1").await;
        timer.on_task_complete(&result).await;

        let cloned = timer.clone();

        assert_eq!(timer.get_total_tasks(), 1);
        assert_eq!(cloned.get_total_tasks(), 0);
        assert!(cloned.get_timings().is_empty());
    }

    #[test]
    fn test_timer_summary_only_preset() {
        let timer = TimerCallback::summary_only();
        assert_eq!(timer.get_total_tasks(), 0);
    }

    #[test]
    fn test_timer_verbose_preset() {
        let timer = TimerCallback::verbose();
        assert_eq!(timer.get_total_tasks(), 0);
    }

    #[tokio::test]
    async fn test_timer_concurrent_task_recording() {
        let timer = Arc::new(TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        }));

        let mut handles = vec![];
        for i in 0..50 {
            let t = timer.clone();
            let handle = tokio::spawn(async move {
                let task_name = format!("task{}", i);
                let host = format!("host{}", i % 5);
                t.on_task_start(&task_name, &host).await;
                let result = create_result(&task_name, &host, 10, true);
                t.on_task_complete(&result).await;
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(timer.get_total_tasks(), 50);
    }
}

// ============================================================================
// ProfileTasksCallback Tests
// ============================================================================

#[cfg(test)]
mod profile_tasks_callback_tests {
    use super::*;

    #[tokio::test]
    async fn test_profile_callback_creation() {
        let callback = ProfileTasksCallback::new();
        assert!(callback.get_completed_tasks().is_empty());
    }

    #[tokio::test]
    async fn test_profile_records_single_task() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;
        callback.on_task_start("install_nginx", "web1").await;

        let result = create_result("install_nginx", "web1", 500, true);
        callback.on_task_complete(&result).await;

        let completed = callback.get_completed_tasks();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].task_name, "install_nginx");
        assert_eq!(completed[0].host, "web1");
        assert!(completed[0].success);
    }

    #[tokio::test]
    async fn test_profile_records_same_task_multiple_hosts() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;

        for host in &["web1", "web2", "web3"] {
            callback.on_task_start("install_package", host).await;
            let result = create_result("install_package", host, 100, true);
            callback.on_task_complete(&result).await;
        }

        let aggregated = callback.get_aggregated_timings();
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].host_count, 3);
    }

    #[tokio::test]
    async fn test_profile_aggregated_min_max_duration() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;

        let durations = vec![100u64, 500, 200];
        for (i, duration) in durations.iter().enumerate() {
            let host = format!("host{}", i);
            callback.on_task_start("variable_task", &host).await;
            let result = create_result("variable_task", &host, *duration, true);
            callback.on_task_complete(&result).await;
        }

        let aggregated = callback.get_aggregated_timings();
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].min_duration, Some(Duration::from_millis(100)));
        assert_eq!(aggregated[0].max_duration, Some(Duration::from_millis(500)));
    }

    #[tokio::test]
    async fn test_profile_aggregated_average_duration() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;

        for i in 0..3 {
            let host = format!("host{}", i);
            callback.on_task_start("uniform_task", &host).await;
            let result = create_result("uniform_task", &host, 100, true);
            callback.on_task_complete(&result).await;
        }

        let aggregated = callback.get_aggregated_timings();
        assert_eq!(aggregated[0].average_duration(), Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_profile_average_task_duration() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;

        for (i, duration) in [100u64, 200, 300].iter().enumerate() {
            let task = format!("task{}", i);
            callback.on_task_start(&task, "host1").await;
            let result = create_result(&task, "host1", *duration, true);
            callback.on_task_complete(&result).await;
        }

        let avg = callback.average_task_duration();
        assert_eq!(avg, Duration::from_millis(200));
    }

    #[tokio::test]
    async fn test_profile_slow_task_count() {
        let config = ProfileTasksConfig {
            slow_threshold_secs: 0.5,
            ..Default::default()
        };
        let callback = ProfileTasksCallback::with_config(config);

        callback.on_playbook_start("test").await;

        callback.on_task_start("fast_task", "host1").await;
        let fast_result = create_result("fast_task", "host1", 100, true);
        callback.on_task_complete(&fast_result).await;

        callback.on_task_start("slow_task", "host1").await;
        let slow_result = create_result("slow_task", "host1", 1000, true);
        callback.on_task_complete(&slow_result).await;

        assert_eq!(callback.slow_task_count(), 1);
    }

    #[tokio::test]
    async fn test_profile_bottleneck_identification() {
        let config = ProfileTasksConfig {
            bottleneck_threshold_secs: 1.0,
            ..Default::default()
        };
        let callback = ProfileTasksCallback::with_config(config);

        callback.on_playbook_start("test").await;

        callback.on_task_start("normal_task", "host1").await;
        let normal_result = create_result("normal_task", "host1", 500, true);
        callback.on_task_complete(&normal_result).await;

        callback.on_task_start("bottleneck_task", "host1").await;
        let bottleneck_result = create_result("bottleneck_task", "host1", 2000, true);
        callback.on_task_complete(&bottleneck_result).await;

        let bottlenecks = callback.identify_bottlenecks();
        assert_eq!(bottlenecks.len(), 1);
        assert_eq!(bottlenecks[0].task_name, "bottleneck_task");
    }

    #[tokio::test]
    async fn test_profile_reset_clears_all_data() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;
        callback.on_task_start("task1", "host1").await;
        let result = create_result("task1", "host1", 100, true);
        callback.on_task_complete(&result).await;

        assert!(!callback.get_completed_tasks().is_empty());

        callback.reset();

        assert!(callback.get_completed_tasks().is_empty());
    }

    #[tokio::test]
    async fn test_profile_json_report_structure() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;
        callback.on_task_start("task1", "host1").await;
        let result = create_changed_result("task1", "host1", 500);
        callback.on_task_complete(&result).await;

        let report = callback.to_json_report();

        assert!(report.get("summary").is_some());
        assert!(report.get("tasks").is_some());
        assert!(report.get("bottlenecks").is_some());

        let tasks = report["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["name"], "task1");
        assert_eq!(tasks[0]["changed"], true);
    }

    #[tokio::test]
    async fn test_profile_sort_by_duration() {
        let config = ProfileTasksConfig {
            sort_order: ProfileSortOrder::Duration,
            ..Default::default()
        };
        let callback = ProfileTasksCallback::with_config(config);

        callback.on_playbook_start("test").await;

        let tasks = vec![("medium", 200u64), ("fast", 50), ("slow", 500)];

        for (task, duration) in &tasks {
            callback.on_task_start(task, "host1").await;
            let result = create_result(task, "host1", *duration, true);
            callback.on_task_complete(&result).await;
        }

        let report = callback.to_json_report();
        let tasks_arr = report["tasks"].as_array().unwrap();

        assert_eq!(tasks_arr[0]["name"], "slow");
        assert_eq!(tasks_arr[1]["name"], "medium");
        assert_eq!(tasks_arr[2]["name"], "fast");
    }

    #[tokio::test]
    async fn test_profile_concurrent_task_recording() {
        let callback = Arc::new(ProfileTasksCallback::new());

        callback.on_playbook_start("concurrent_test").await;

        let mut handles = vec![];
        for i in 0..50 {
            let cb = callback.clone();
            let handle = tokio::spawn(async move {
                let task_name = format!("task{}", i);
                let host = format!("host{}", i % 5);
                cb.on_task_start(&task_name, &host).await;
                let result = create_result(&task_name, &host, 50, true);
                cb.on_task_complete(&result).await;
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let completed = callback.get_completed_tasks();
        assert_eq!(completed.len(), 50);
    }

    #[tokio::test]
    async fn test_profile_skipped_tasks_excluded_by_default() {
        let callback = ProfileTasksCallback::new();

        callback.on_playbook_start("test").await;

        callback.on_task_start("skipped_task", "host1").await;
        let skipped = create_skipped_result("skipped_task", "host1");
        callback.on_task_complete(&skipped).await;

        callback.on_task_start("normal_task", "host1").await;
        let normal = create_result("normal_task", "host1", 100, true);
        callback.on_task_complete(&normal).await;

        let aggregated = callback.get_aggregated_timings();
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].task_name, "normal_task");
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_playbook_lifecycle_with_timer() {
        let timer = TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        });

        let hosts = vec!["web1".to_string(), "web2".to_string()];

        timer.on_playbook_start("deploy_app").await;
        timer.on_play_start("Install dependencies", &hosts).await;

        for host in &["web1", "web2"] {
            timer.on_task_start("install_nginx", host).await;
            let result = create_changed_result("install_nginx", host, 500);
            timer.on_task_complete(&result).await;
        }

        timer.on_handler_triggered("restart_nginx").await;
        timer.on_play_end("Install dependencies", true).await;
        timer.on_playbook_end("deploy_app", true).await;

        assert_eq!(timer.get_total_tasks(), 2);
        let timings = timer.get_timings();
        assert!(timings.iter().all(|t| t.changed));
    }

    #[tokio::test]
    async fn test_full_playbook_lifecycle_with_profiler() {
        let profiler = ProfileTasksCallback::with_config(ProfileTasksConfig {
            slow_threshold_secs: 0.4,
            bottleneck_threshold_secs: 0.8,
            ..Default::default()
        });

        let hosts = vec!["web1".to_string(), "web2".to_string()];

        profiler.on_playbook_start("deploy").await;
        profiler.on_play_start("setup", &hosts).await;

        let tasks = vec![
            ("install_deps", "web1", 500u64, true),
            ("install_deps", "web2", 550, true),
            ("configure", "web1", 300, true),
            ("configure", "web2", 320, true),
            ("verify", "web1", 100, true),
            ("verify", "web2", 80, false),
        ];

        for (task, host, duration, success) in &tasks {
            profiler.on_task_start(task, host).await;
            let result = create_result(task, host, *duration, *success);
            profiler.on_task_complete(&result).await;
        }

        profiler.on_play_end("setup", false).await;
        profiler.on_playbook_end("deploy", false).await;

        let completed = profiler.get_completed_tasks();
        assert_eq!(completed.len(), 6);

        let aggregated = profiler.get_aggregated_timings();
        assert_eq!(aggregated.len(), 3);

        assert!(profiler.slow_task_count() >= 2);
    }

    #[tokio::test]
    async fn test_both_callbacks_together() {
        let timer = Arc::new(TimerCallback::new(TimerConfig {
            show_per_task: false,
            show_summary: false,
            ..Default::default()
        }));
        let profiler = Arc::new(ProfileTasksCallback::new());

        timer.on_playbook_start("test").await;
        profiler.on_playbook_start("test").await;

        let hosts = vec!["host1".to_string()];
        timer.on_play_start("play1", &hosts).await;
        profiler.on_play_start("play1", &hosts).await;

        for i in 0..5 {
            let task = format!("task{}", i);
            timer.on_task_start(&task, "host1").await;
            profiler.on_task_start(&task, "host1").await;

            let result = create_result(&task, "host1", (i + 1) * 100, true);
            timer.on_task_complete(&result).await;
            profiler.on_task_complete(&result).await;
        }

        timer.on_play_end("play1", true).await;
        profiler.on_play_end("play1", true).await;

        timer.on_playbook_end("test", true).await;
        profiler.on_playbook_end("test", true).await;

        assert_eq!(timer.get_total_tasks(), 5);
        assert_eq!(profiler.get_completed_tasks().len(), 5);

        let timer_slowest = timer.get_slowest_tasks(1);
        assert_eq!(timer_slowest[0].task_name, "task4");
    }
}
