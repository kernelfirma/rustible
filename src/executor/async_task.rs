//! Async Task Manager for Rustible
//!
//! This module provides Ansible-compatible async task execution patterns:
//! - Background task submission with `async` parameter
//! - Status polling with `poll` parameter
//! - Timeout handling
//! - Result retrieval via `async_status` module
//!
//! # Example Usage
//!
//! ```yaml
//! # Submit a long-running task asynchronously
//! - name: Run long operation
//!   command: /usr/bin/long_operation
//!   async: 3600        # Maximum runtime in seconds
//!   poll: 0            # Don't poll (fire and forget), or poll interval in seconds
//!   register: async_result
//!
//! # Later, check the status
//! - name: Check on async task
//!   async_status:
//!     jid: "{{ async_result.ansible_job_id }}"
//!   register: job_result
//!   until: job_result.finished
//!   retries: 30
//!   delay: 10
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::executor::task::TaskResult;
use crate::executor::ExecutorResult;

/// Configuration for async task execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AsyncConfig {
    /// Maximum time in seconds the task can run (async parameter)
    /// If 0, task runs synchronously
    pub async_timeout: u64,

    /// Poll interval in seconds (poll parameter)
    /// - 0: Fire and forget (return immediately, don't wait)
    /// - >0: Poll for completion at this interval
    /// - None/default: Poll every 15 seconds (Ansible default)
    pub poll_interval: Option<u64>,
}

impl AsyncConfig {
    /// Create a new async configuration
    pub fn new(async_timeout: u64, poll_interval: Option<u64>) -> Self {
        Self {
            async_timeout,
            poll_interval,
        }
    }

    /// Check if task should run asynchronously
    pub fn is_async(&self) -> bool {
        self.async_timeout > 0
    }

    /// Get the poll interval in seconds
    /// Returns 0 for fire-and-forget, or the specified/default interval
    pub fn get_poll_interval(&self) -> u64 {
        self.poll_interval.unwrap_or(15) // Ansible default is 15 seconds
    }

    /// Check if this is a fire-and-forget task (poll: 0)
    pub fn is_fire_and_forget(&self) -> bool {
        self.poll_interval == Some(0)
    }
}

/// Status of an async job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AsyncJobStatus {
    /// Job is queued but not yet started
    #[default]
    Pending,
    /// Job is currently running
    Running,
    /// Job completed successfully
    Finished,
    /// Job failed
    Failed,
    /// Job was cancelled
    Cancelled,
    /// Job timed out
    TimedOut,
}

/// Information about an async job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncJobInfo {
    /// Unique job identifier
    pub jid: String,

    /// Host the job is running on
    pub host: String,

    /// Task name
    pub task_name: String,

    /// Module being executed
    pub module: String,

    /// Current status
    pub status: AsyncJobStatus,

    /// Whether the job has finished (completed, failed, timed out, or cancelled)
    pub finished: bool,

    /// Task result (available when finished)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,

    /// Start time (Unix timestamp)
    pub started: u64,

    /// End time (Unix timestamp, if finished)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<u64>,

    /// Maximum allowed runtime in seconds
    pub async_timeout: u64,

    /// Progress message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,

    /// Return code (for command/shell modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc: Option<i32>,

    /// Standard output (for command/shell modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,

    /// Standard error (for command/shell modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,

    /// Whether the task made changes
    pub changed: bool,
}

impl AsyncJobInfo {
    fn unix_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Create a new pending job
    pub fn new(
        jid: String,
        host: String,
        task_name: String,
        module: String,
        async_timeout: u64,
    ) -> Self {
        Self {
            jid,
            host,
            task_name,
            module,
            status: AsyncJobStatus::Pending,
            finished: false,
            result: None,
            started: Self::unix_timestamp(),
            ended: None,
            async_timeout,
            msg: Some("Job pending".to_string()),
            rc: None,
            stdout: None,
            stderr: None,
            changed: false,
        }
    }

    /// Mark job as running
    pub fn mark_running(&mut self) {
        self.status = AsyncJobStatus::Running;
        self.msg = Some("Job running".to_string());
    }

    /// Mark job as finished with result
    pub fn mark_finished(&mut self, result: TaskResult) {
        self.status = AsyncJobStatus::Finished;
        self.finished = true;
        self.ended = Some(Self::unix_timestamp());
        self.changed = result.changed;
        self.msg = result.msg.clone().or(Some("Job finished".to_string()));

        // Extract command output if available
        if let Some(ref result_data) = result.result {
            if let Some(rc) = result_data.get("rc").and_then(|v| v.as_i64()) {
                self.rc = Some(rc as i32);
            }
            if let Some(stdout) = result_data.get("stdout").and_then(|v| v.as_str()) {
                self.stdout = Some(stdout.to_string());
            }
            if let Some(stderr) = result_data.get("stderr").and_then(|v| v.as_str()) {
                self.stderr = Some(stderr.to_string());
            }
        }

        self.result = Some(result);
    }

    /// Mark job as failed
    pub fn mark_failed(&mut self, error_msg: String) {
        self.status = AsyncJobStatus::Failed;
        self.finished = true;
        self.ended = Some(Self::unix_timestamp());
        self.msg = Some(error_msg);
        self.result = Some(TaskResult::failed(self.msg.clone().unwrap_or_default()));
    }

    /// Mark job as timed out
    pub fn mark_timed_out(&mut self) {
        self.status = AsyncJobStatus::TimedOut;
        self.finished = true;
        self.ended = Some(Self::unix_timestamp());
        self.msg = Some(format!(
            "Job timed out after {} seconds",
            self.async_timeout
        ));
        self.result = Some(TaskResult::failed(self.msg.clone().unwrap_or_default()));
    }

    /// Mark job as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = AsyncJobStatus::Cancelled;
        self.finished = true;
        self.ended = Some(Self::unix_timestamp());
        self.msg = Some("Job cancelled".to_string());
        self.result = Some(TaskResult::failed("Job cancelled"));
    }

    /// Convert to JSON value for registration
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or(JsonValue::Null)
    }

    /// Get elapsed time in seconds
    pub fn elapsed_seconds(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.started)
    }

    /// Check if job has exceeded timeout
    pub fn is_timed_out(&self) -> bool {
        if self.finished {
            return false;
        }
        self.elapsed_seconds() >= self.async_timeout
    }
}

/// Internal structure for tracking running tasks
struct RunningJob {
    info: AsyncJobInfo,
    handle: Option<JoinHandle<ExecutorResult<TaskResult>>>,
    abort_handle: Option<tokio::task::AbortHandle>,
    start_instant: Instant,
}

/// Async Task Manager
///
/// Manages background task execution, polling, and result retrieval.
/// Thread-safe and can handle multiple concurrent async tasks.
pub struct AsyncTaskManager {
    /// Registry of all jobs (completed and running)
    jobs: Arc<RwLock<HashMap<String, AsyncJobInfo>>>,

    /// Running jobs with their handles
    running: Arc<Mutex<HashMap<String, RunningJob>>>,

    /// Default timeout for async tasks (seconds)
    default_timeout: u64,

    /// Maximum number of concurrent async tasks per host
    max_concurrent_per_host: usize,

    /// Job retention time (seconds) - how long to keep completed job info
    job_retention_time: u64,
}

impl Default for AsyncTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncTaskManager {
    /// Create a new AsyncTaskManager with default settings
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            default_timeout: 3600, // 1 hour default
            max_concurrent_per_host: 10,
            job_retention_time: 86400, // 24 hours
        }
    }

    /// Create a new AsyncTaskManager with custom settings
    pub fn with_config(
        default_timeout: u64,
        max_concurrent_per_host: usize,
        job_retention_time: u64,
    ) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            default_timeout,
            max_concurrent_per_host,
            job_retention_time,
        }
    }

    /// Generate a unique job ID
    pub fn generate_job_id() -> String {
        // Generate Ansible-compatible job ID format
        let uuid = Uuid::new_v4();
        format!(
            "{}.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            &uuid.to_string().replace('-', "")[..16]
        )
    }

    /// Submit a task for async execution
    ///
    /// Returns the job ID immediately. The task runs in the background.
    pub async fn submit_task<F, Fut>(
        &self,
        host: &str,
        task_name: &str,
        module: &str,
        async_timeout: u64,
        task_fn: F,
    ) -> ExecutorResult<String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ExecutorResult<TaskResult>> + Send + 'static,
    {
        let timeout = if async_timeout > 0 {
            async_timeout
        } else {
            self.default_timeout
        };

        // Check concurrent limit
        {
            let running = self.running.lock().await;
            let host_jobs: Vec<_> = running
                .values()
                .filter(|j| j.info.host == host && !j.info.finished)
                .collect();

            if host_jobs.len() >= self.max_concurrent_per_host {
                return Err(crate::executor::ExecutorError::Other(format!(
                    "Maximum concurrent async tasks ({}) reached for host {}",
                    self.max_concurrent_per_host, host
                )));
            }
        }

        // Generate job ID
        let jid = Self::generate_job_id();
        debug!("Submitting async task with jid={}", jid);

        // Create job info
        let mut job_info = AsyncJobInfo::new(
            jid.clone(),
            host.to_string(),
            task_name.to_string(),
            module.to_string(),
            timeout,
        );
        job_info.mark_running();

        // Clone for the spawned task
        let jobs = Arc::clone(&self.jobs);
        let running = Arc::clone(&self.running);
        let jid_clone = jid.clone();

        // Spawn the task with timeout
        let handle = tokio::spawn(async move {
            let timeout_duration = Duration::from_secs(timeout);

            match tokio::time::timeout(timeout_duration, task_fn()).await {
                Ok(result) => {
                    // Task completed (successfully or with error)
                    match &result {
                        Ok(task_result) => {
                            // Update job info with result
                            let mut jobs_guard = jobs.write().await;
                            if let Some(job) = jobs_guard.get_mut(&jid_clone) {
                                job.mark_finished(task_result.clone());
                            }

                            // Remove from running
                            let mut running_guard = running.lock().await;
                            running_guard.remove(&jid_clone);
                        }
                        Err(e) => {
                            // Update job info with error
                            let mut jobs_guard = jobs.write().await;
                            if let Some(job) = jobs_guard.get_mut(&jid_clone) {
                                job.mark_failed(format!("{}", e));
                            }

                            // Remove from running
                            let mut running_guard = running.lock().await;
                            running_guard.remove(&jid_clone);
                        }
                    }
                    result
                }
                Err(_) => {
                    // Timeout occurred
                    error!(
                        "Async task {} timed out after {} seconds",
                        jid_clone, timeout
                    );

                    let mut jobs_guard = jobs.write().await;
                    if let Some(job) = jobs_guard.get_mut(&jid_clone) {
                        job.mark_timed_out();
                    }

                    // Remove from running
                    let mut running_guard = running.lock().await;
                    running_guard.remove(&jid_clone);

                    Err(crate::executor::ExecutorError::Timeout(format!(
                        "Task timed out after {} seconds",
                        timeout
                    )))
                }
            }
        });

        // Store the job
        {
            let mut jobs_guard = self.jobs.write().await;
            jobs_guard.insert(jid.clone(), job_info.clone());
        }

        // Store the running job with handle
        {
            let abort_handle = handle.abort_handle();
            let mut running_guard = self.running.lock().await;
            running_guard.insert(
                jid.clone(),
                RunningJob {
                    info: job_info,
                    handle: Some(handle),
                    abort_handle: Some(abort_handle),
                    start_instant: Instant::now(),
                },
            );
        }

        info!(
            "Async task submitted: jid={}, host={}, module={}",
            jid, host, module
        );
        Ok(jid)
    }

    /// Get the status of a job
    pub async fn get_job_status(&self, jid: &str) -> Option<AsyncJobInfo> {
        let jobs = self.jobs.read().await;
        jobs.get(jid).cloned()
    }

    /// Get the status of a job as JSON (for async_status module)
    pub async fn get_job_status_json(&self, jid: &str) -> Option<JsonValue> {
        self.get_job_status(jid).await.map(|info| info.to_json())
    }

    /// Check if a job is finished
    pub async fn is_finished(&self, jid: &str) -> bool {
        let jobs = self.jobs.read().await;
        jobs.get(jid).map(|j| j.finished).unwrap_or(true)
    }

    /// Wait for a job to complete with polling
    pub async fn wait_for_job(
        &self,
        jid: &str,
        poll_interval: u64,
        max_wait: Option<u64>,
    ) -> Option<AsyncJobInfo> {
        let start = Instant::now();
        let poll_duration = Duration::from_secs(poll_interval);
        let max_duration = max_wait.map(Duration::from_secs);

        loop {
            // Check if job is finished
            if let Some(info) = self.get_job_status(jid).await {
                if info.finished {
                    return Some(info);
                }
            } else {
                // Job not found
                return None;
            }

            // Check max wait time
            if let Some(max) = max_duration {
                if start.elapsed() >= max {
                    warn!("Max wait time exceeded for job {}", jid);
                    return self.get_job_status(jid).await;
                }
            }

            // Wait before next poll
            tokio::time::sleep(poll_duration).await;
        }
    }

    /// Cancel a running job
    pub async fn cancel_job(&self, jid: &str) -> bool {
        // Try to abort the running task
        {
            let mut running = self.running.lock().await;
            if let Some(job) = running.remove(jid) {
                if let Some(abort_handle) = job.abort_handle {
                    abort_handle.abort();
                }
            }
        }

        // Update job status
        {
            let mut jobs = self.jobs.write().await;
            if let Some(job) = jobs.get_mut(jid) {
                if !job.finished {
                    job.mark_cancelled();
                    info!("Cancelled job: {}", jid);
                    return true;
                }
            }
        }

        false
    }

    /// List all jobs for a host
    pub async fn list_jobs(&self, host: Option<&str>) -> Vec<AsyncJobInfo> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|j| host.map(|h| j.host == h).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// List running jobs
    pub async fn list_running_jobs(&self, host: Option<&str>) -> Vec<AsyncJobInfo> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .filter(|j| !j.finished && host.map(|h| j.host == h).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// Clean up old completed jobs
    pub async fn cleanup_old_jobs(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut jobs = self.jobs.write().await;
        let to_remove: Vec<String> = jobs
            .iter()
            .filter(|(_, job)| {
                job.finished
                    && job
                        .ended
                        .map(|e| now - e > self.job_retention_time)
                        .unwrap_or(false)
            })
            .map(|(jid, _)| jid.clone())
            .collect();

        for jid in to_remove {
            debug!("Cleaning up old job: {}", jid);
            jobs.remove(&jid);
        }
    }

    /// Get the result for async task registration
    ///
    /// Returns the initial result for fire-and-forget tasks,
    /// including the job ID for later status checking.
    pub fn create_async_result(jid: &str, _host: &str, started: bool) -> TaskResult {
        let result_data = serde_json::json!({
            "ansible_job_id": jid,
            "started": if started { 1 } else { 0 },
            "finished": 0,
            "results_file": format!("/tmp/ansible-async/{}", jid),
        });

        TaskResult {
            status: crate::executor::task::TaskStatus::Ok,
            changed: true,
            msg: Some(format!("Async job started: {}", jid)),
            result: Some(result_data),
            diff: None,
        }
    }

    /// Create the result for async_status module
    pub async fn create_status_result(&self, jid: &str) -> TaskResult {
        if let Some(info) = self.get_job_status(jid).await {
            let mut result_data = info.to_json();

            // Ensure Ansible-compatible fields
            if let Some(obj) = result_data.as_object_mut() {
                obj.insert(
                    "ansible_job_id".to_string(),
                    JsonValue::String(jid.to_string()),
                );
                obj.insert(
                    "finished".to_string(),
                    JsonValue::Number(if info.finished { 1 } else { 0 }.into()),
                );
            }

            let status = if info.finished {
                if info.status == AsyncJobStatus::Finished {
                    crate::executor::task::TaskStatus::Ok
                } else {
                    crate::executor::task::TaskStatus::Failed
                }
            } else {
                crate::executor::task::TaskStatus::Ok
            };

            TaskResult {
                status,
                changed: info.changed,
                msg: info.msg,
                result: Some(result_data),
                diff: None,
            }
        } else {
            TaskResult::failed(format!("Job not found: {}", jid))
        }
    }
}

/// Global async task manager instance
static ASYNC_MANAGER: once_cell::sync::Lazy<AsyncTaskManager> =
    once_cell::sync::Lazy::new(AsyncTaskManager::new);

/// Get the global async task manager
pub fn get_async_manager() -> &'static AsyncTaskManager {
    &ASYNC_MANAGER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_config() {
        let config = AsyncConfig::new(3600, Some(0));
        assert!(config.is_async());
        assert!(config.is_fire_and_forget());
        assert_eq!(config.get_poll_interval(), 0);

        let config2 = AsyncConfig::new(0, None);
        assert!(!config2.is_async());
        assert!(!config2.is_fire_and_forget());
        assert_eq!(config2.get_poll_interval(), 15); // Default

        let config3 = AsyncConfig::new(300, Some(10));
        assert!(config3.is_async());
        assert!(!config3.is_fire_and_forget());
        assert_eq!(config3.get_poll_interval(), 10);
    }

    #[test]
    fn test_job_id_generation() {
        let jid1 = AsyncTaskManager::generate_job_id();
        let jid2 = AsyncTaskManager::generate_job_id();

        assert!(!jid1.is_empty());
        assert!(!jid2.is_empty());
        assert_ne!(jid1, jid2);
        assert!(jid1.contains('.'));
    }

    #[test]
    fn test_async_job_info() {
        let mut job = AsyncJobInfo::new(
            "test.123".to_string(),
            "localhost".to_string(),
            "Test task".to_string(),
            "command".to_string(),
            3600,
        );

        assert_eq!(job.status, AsyncJobStatus::Pending);
        assert!(!job.finished);

        job.mark_running();
        assert_eq!(job.status, AsyncJobStatus::Running);
        assert!(!job.finished);

        job.mark_finished(TaskResult::changed().with_msg("Done"));
        assert_eq!(job.status, AsyncJobStatus::Finished);
        assert!(job.finished);
        assert!(job.changed);
    }

    #[test]
    fn test_async_job_timeout() {
        let mut job = AsyncJobInfo::new(
            "test.456".to_string(),
            "localhost".to_string(),
            "Test task".to_string(),
            "command".to_string(),
            0, // 0 timeout for testing
        );

        // With 0 timeout, should be timed out immediately
        assert!(job.is_timed_out());

        job.mark_timed_out();
        assert_eq!(job.status, AsyncJobStatus::TimedOut);
        assert!(job.finished);
    }

    #[tokio::test]
    async fn test_async_manager_submit() {
        let manager = AsyncTaskManager::new();

        let jid = manager
            .submit_task("localhost", "Test task", "debug", 10, || async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(TaskResult::ok().with_msg("Success"))
            })
            .await
            .unwrap();

        assert!(!jid.is_empty());

        // Job should be running
        let status = manager.get_job_status(&jid).await.unwrap();
        assert_eq!(status.status, AsyncJobStatus::Running);

        // Wait for completion
        let result = manager.wait_for_job(&jid, 1, Some(5)).await;
        assert!(result.is_some());
        assert!(result.unwrap().finished);
    }

    #[tokio::test]
    async fn test_async_manager_cancel() {
        let manager = AsyncTaskManager::new();

        let jid = manager
            .submit_task("localhost", "Long task", "command", 60, || async {
                // Simulate a long-running task
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await
            .unwrap();

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Cancel the job
        let cancelled = manager.cancel_job(&jid).await;
        assert!(cancelled);

        // Check status
        let status = manager.get_job_status(&jid).await.unwrap();
        assert_eq!(status.status, AsyncJobStatus::Cancelled);
        assert!(status.finished);
    }

    #[tokio::test]
    async fn test_fire_and_forget_result() {
        let result = AsyncTaskManager::create_async_result("123.abc", "localhost", true);

        assert_eq!(result.status, crate::executor::task::TaskStatus::Ok);
        assert!(result.changed);

        let data = result.result.unwrap();
        assert_eq!(data["ansible_job_id"], "123.abc");
        assert_eq!(data["started"], 1);
        assert_eq!(data["finished"], 0);
    }
}
