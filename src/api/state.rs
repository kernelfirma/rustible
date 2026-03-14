//! Application state management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use tokio::sync::{broadcast, Mutex, Notify};
use uuid::Uuid;

use super::auth::{AuthConfig, JwtAuth};
use super::types::{
    JobInfo, JobStats, JobStatus, KernelDeploymentActionRequired, KernelDeploymentHost,
    KernelDeploymentProgress, KernelDeploymentStage, WsMessage,
};
use super::ApiConfig;
use crate::inventory::Inventory;

/// Shared application state.
pub struct AppState {
    /// JWT authentication handler
    pub jwt_auth: JwtAuth,
    /// Job storage
    pub jobs: RwLock<HashMap<Uuid, Job>>,
    /// Loaded inventory (cached)
    pub inventory: RwLock<Option<Arc<Inventory>>>,
    /// API configuration
    pub config: ApiConfig,
    /// Server start time
    pub start_time: Instant,
    /// WebSocket broadcast channels per job
    pub ws_channels: RwLock<HashMap<Uuid, broadcast::Sender<WsMessage>>>,
    /// User credentials (simple in-memory store for demo)
    pub users: RwLock<HashMap<String, UserCredentials>>,
    /// Kernel deployment runtime state keyed by job ID.
    pub kernel_jobs: RwLock<HashMap<Uuid, Arc<KernelJobRuntime>>>,
}

/// User credentials for authentication.
#[derive(Clone)]
pub struct UserCredentials {
    /// Hashed password (in production, use proper password hashing)
    pub password_hash: String,
    /// User roles
    pub roles: Vec<String>,
}

impl AppState {
    /// Create a new application state.
    pub fn new(config: ApiConfig) -> Self {
        let auth_config = AuthConfig {
            secret: config.jwt_secret.clone(),
            expiration_secs: config.token_expiration_secs,
            issuer: "rustible".to_string(),
        };

        let jwt_auth = JwtAuth::new(&auth_config);

        let mut users = HashMap::new();
        for (username, user) in &config.users {
            users.insert(
                username.clone(),
                UserCredentials {
                    password_hash: user.password.clone(),
                    roles: user.roles.clone(),
                },
            );
        }

        Self {
            jwt_auth,
            jobs: RwLock::new(HashMap::new()),
            inventory: RwLock::new(None),
            config,
            start_time: Instant::now(),
            ws_channels: RwLock::new(HashMap::new()),
            users: RwLock::new(users),
            kernel_jobs: RwLock::new(HashMap::new()),
        }
    }

    /// Get server uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Get count of active (running) jobs.
    pub fn active_job_count(&self) -> usize {
        self.jobs
            .read()
            .values()
            .filter(|j| matches!(j.status, JobStatus::Running | JobStatus::ActionRequired))
            .count()
    }

    /// Check user credentials (returns roles if valid).
    pub fn verify_credentials(&self, username: &str, password: &str) -> Option<Vec<String>> {
        let users = self.users.read();
        users
            .get(username)
            .filter(|creds| {
                // In production, use proper password verification
                creds.password_hash == password
            })
            .map(|creds| creds.roles.clone())
    }

    /// Create a new job.
    pub fn create_job(
        &self,
        playbook: String,
        inventory: Option<String>,
        user: Option<String>,
        extra_vars: HashMap<String, serde_json::Value>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let job = Job {
            id,
            status: JobStatus::Pending,
            playbook,
            inventory,
            extra_vars,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            user,
            output: Vec::new(),
            stats: None,
            error: None,
        };

        // Create broadcast channel for this job
        let (tx, _) = broadcast::channel(1000);
        self.ws_channels.write().insert(id, tx);

        self.jobs.write().insert(id, job);
        id
    }

    /// Get a job by ID.
    pub fn get_job(&self, id: Uuid) -> Option<Job> {
        self.jobs.read().get(&id).cloned()
    }

    /// Update job status.
    pub fn update_job_status(&self, id: Uuid, status: JobStatus) {
        if let Some(job) = self.jobs.write().get_mut(&id) {
            job.status = status;
            if status == JobStatus::Running && job.started_at.is_none() {
                job.started_at = Some(Utc::now());
            }
            if matches!(
                status,
                JobStatus::Success | JobStatus::Failed | JobStatus::Cancelled
            ) {
                job.finished_at = Some(Utc::now());
            }

            // Broadcast status change
            if let Some(tx) = self.ws_channels.read().get(&id) {
                let _ = tx.send(WsMessage::StatusChange {
                    job_id: id,
                    status,
                    message: None,
                });
            }
        }
    }

    /// Append output to a job.
    pub fn append_job_output(&self, id: Uuid, line: String, stream: &str) {
        if let Some(job) = self.jobs.write().get_mut(&id) {
            job.output.push(line.clone());

            // Broadcast output
            if let Some(tx) = self.ws_channels.read().get(&id) {
                let _ = tx.send(WsMessage::Output {
                    job_id: id,
                    line,
                    stream: stream.to_string(),
                    timestamp: Utc::now(),
                });
            }
        }
    }

    /// Set job error.
    pub fn set_job_error(&self, id: Uuid, error: String) {
        if let Some(job) = self.jobs.write().get_mut(&id) {
            job.error = Some(error);
        }
    }

    /// Set job stats.
    pub fn set_job_stats(&self, id: Uuid, stats: JobStats) {
        if let Some(job) = self.jobs.write().get_mut(&id) {
            job.stats = Some(stats);
        }
    }

    /// Register runtime metadata for a kernel deployment job.
    pub fn register_kernel_job(&self, id: Uuid, hosts: &[KernelDeploymentHost]) {
        let runtime = Arc::new(KernelJobRuntime::new(
            hosts.iter().map(|host| host.name.clone()).collect(),
        ));
        self.kernel_jobs.write().insert(id, runtime);
    }

    /// Remove kernel runtime metadata after completion.
    pub fn remove_kernel_job(&self, id: Uuid) {
        self.kernel_jobs.write().remove(&id);
    }

    /// Get a clone of the runtime handle for a kernel deployment job.
    pub fn get_kernel_job_runtime(&self, id: Uuid) -> Option<Arc<KernelJobRuntime>> {
        self.kernel_jobs.read().get(&id).cloned()
    }

    /// Snapshot the current kernel deployment progress.
    pub async fn kernel_job_progress(&self, id: Uuid) -> Option<KernelDeploymentProgress> {
        let runtime = self.get_kernel_job_runtime(id)?;
        Some(runtime.snapshot().await)
    }

    /// Get WebSocket sender for a job.
    pub fn get_ws_sender(&self, id: Uuid) -> Option<broadcast::Sender<WsMessage>> {
        self.ws_channels.read().get(&id).cloned()
    }

    /// Subscribe to job updates.
    pub fn subscribe_to_job(&self, id: Uuid) -> Option<broadcast::Receiver<WsMessage>> {
        self.ws_channels.read().get(&id).map(|tx| tx.subscribe())
    }

    /// List jobs with optional filtering.
    pub fn list_jobs(
        &self,
        status_filter: Option<JobStatus>,
        page: usize,
        per_page: usize,
    ) -> (Vec<JobInfo>, usize) {
        let jobs = self.jobs.read();

        let filtered: Vec<_> = jobs
            .values()
            .filter(|j| status_filter.is_none_or(|s| j.status == s))
            .collect();

        let total = filtered.len();
        let start = (page.saturating_sub(1)) * per_page;

        let page_jobs: Vec<JobInfo> = filtered
            .into_iter()
            .skip(start)
            .take(per_page)
            .map(|j| j.to_info())
            .collect();

        (page_jobs, total)
    }

    /// Load or get cached inventory.
    pub fn get_inventory(&self) -> Option<Arc<Inventory>> {
        self.inventory.read().clone()
    }

    /// Load inventory from the configured path.
    pub fn load_inventory(&self) -> Result<Arc<Inventory>, crate::inventory::InventoryError> {
        if let Some(path) = &self.config.inventory_path {
            let inv = Inventory::load(path)?;
            let arc_inv = Arc::new(inv);
            *self.inventory.write() = Some(arc_inv.clone());
            Ok(arc_inv)
        } else {
            Err(crate::inventory::InventoryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No inventory path configured",
            )))
        }
    }

    /// Set inventory directly.
    pub fn set_inventory(&self, inventory: Inventory) {
        *self.inventory.write() = Some(Arc::new(inventory));
    }

    /// Cancel a job.
    pub fn cancel_job(&self, id: Uuid) -> bool {
        if let Some(job) = self.jobs.write().get_mut(&id) {
            if matches!(
                job.status,
                JobStatus::Pending | JobStatus::Running | JobStatus::ActionRequired
            ) {
                job.status = JobStatus::Cancelled;
                job.finished_at = Some(Utc::now());

                if let Some(runtime) = self.kernel_jobs.read().get(&id).cloned() {
                    runtime.resume_notify.notify_waiters();
                }

                // Broadcast cancellation
                if let Some(tx) = self.ws_channels.read().get(&id) {
                    let _ = tx.send(WsMessage::StatusChange {
                        job_id: id,
                        status: JobStatus::Cancelled,
                        message: Some("Job cancelled by user".to_string()),
                    });
                }
                return true;
            }
        }
        false
    }
}

/// Runtime state for a kernel deployment job.
#[derive(Debug)]
pub struct KernelJobRuntime {
    state: Mutex<KernelJobRuntimeState>,
    pub resume_notify: Notify,
}

impl KernelJobRuntime {
    fn new(hosts: Vec<String>) -> Self {
        Self {
            state: Mutex::new(KernelJobRuntimeState {
                stage: KernelDeploymentStage::Queued,
                current_host: None,
                hosts,
                action_required: None,
            }),
            resume_notify: Notify::new(),
        }
    }

    /// Update the visible workflow stage.
    pub async fn set_stage(
        &self,
        stage: KernelDeploymentStage,
        current_host: Option<String>,
    ) -> KernelDeploymentProgress {
        let mut state = self.state.lock().await;
        state.stage = stage;
        state.current_host = current_host;
        if stage != KernelDeploymentStage::ActionRequired {
            state.action_required = None;
        }
        state.snapshot()
    }

    /// Set the current action-required payload.
    pub async fn set_action_required(
        &self,
        action: KernelDeploymentActionRequired,
    ) -> KernelDeploymentProgress {
        let mut state = self.state.lock().await;
        state.stage = KernelDeploymentStage::ActionRequired;
        state.current_host = Some(action.host.clone());
        state.action_required = Some(action);
        state.snapshot()
    }

    /// Snapshot the current runtime state.
    pub async fn snapshot(&self) -> KernelDeploymentProgress {
        self.state.lock().await.snapshot()
    }

    /// Validate and clear the current action-required state.
    pub async fn clear_action_required(
        &self,
        action_id: &str,
    ) -> Result<KernelDeploymentProgress, &'static str> {
        let mut state = self.state.lock().await;
        match state.action_required.as_ref() {
            Some(action) if action.action_id == action_id => {
                state.action_required = None;
                state.stage = KernelDeploymentStage::Preflight;
                Ok(state.snapshot())
            }
            Some(_) => Err("action_id does not match the pending action"),
            None => Err("job is not waiting for an action"),
        }
    }
}

#[derive(Debug)]
struct KernelJobRuntimeState {
    stage: KernelDeploymentStage,
    current_host: Option<String>,
    hosts: Vec<String>,
    action_required: Option<KernelDeploymentActionRequired>,
}

impl KernelJobRuntimeState {
    fn snapshot(&self) -> KernelDeploymentProgress {
        KernelDeploymentProgress {
            stage: self.stage,
            current_host: self.current_host.clone(),
            hosts: self.hosts.clone(),
            action_required: self.action_required.clone(),
        }
    }
}

/// Internal job representation.
#[derive(Debug, Clone)]
pub struct Job {
    /// Unique job ID
    pub id: Uuid,
    /// Job status
    pub status: JobStatus,
    /// Playbook path
    pub playbook: String,
    /// Inventory path
    pub inventory: Option<String>,
    /// Extra variables
    pub extra_vars: HashMap<String, serde_json::Value>,
    /// Creation time
    pub created_at: DateTime<Utc>,
    /// Start time
    pub started_at: Option<DateTime<Utc>>,
    /// Finish time
    pub finished_at: Option<DateTime<Utc>>,
    /// User who started the job
    pub user: Option<String>,
    /// Output lines
    pub output: Vec<String>,
    /// Execution statistics
    pub stats: Option<JobStats>,
    /// Error message
    pub error: Option<String>,
}

impl Job {
    /// Convert to JobInfo for API responses.
    pub fn to_info(&self) -> JobInfo {
        let duration_secs = match (self.started_at, self.finished_at) {
            (Some(start), Some(end)) => Some((end - start).num_milliseconds() as f64 / 1000.0),
            (Some(start), None)
                if matches!(self.status, JobStatus::Running | JobStatus::ActionRequired) =>
            {
                Some((Utc::now() - start).num_milliseconds() as f64 / 1000.0)
            }
            _ => None,
        };

        JobInfo {
            id: self.id,
            status: self.status,
            playbook: self.playbook.clone(),
            inventory: self.inventory.clone(),
            extra_vars: self.extra_vars.clone(),
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            duration_secs,
            user: self.user.clone(),
        }
    }

    /// Get full output as a single string.
    pub fn full_output(&self) -> String {
        self.output.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{
        KernelDeploymentActionKind, KernelDeploymentBmcActionHint, KernelDeploymentBmcProvider,
    };

    #[tokio::test]
    async fn test_kernel_runtime_action_required_round_trip() {
        let runtime = KernelJobRuntime::new(vec!["node-a".to_string()]);
        let action = KernelDeploymentActionRequired {
            action_id: "node-a:1".to_string(),
            kind: KernelDeploymentActionKind::SecureBootEnrollment,
            host: "node-a".to_string(),
            message: "enroll cert".to_string(),
            instructions: vec!["step 1".to_string()],
            bmc: Some(KernelDeploymentBmcActionHint {
                provider: KernelDeploymentBmcProvider::Redfish,
                endpoint: "https://bmc.example.test".to_string(),
            }),
        };

        let progress = runtime.set_action_required(action.clone()).await;
        assert_eq!(progress.stage, KernelDeploymentStage::ActionRequired);
        assert_eq!(progress.action_required, Some(action.clone()));

        let resumed = runtime.clear_action_required("node-a:1").await.unwrap();
        assert_eq!(resumed.stage, KernelDeploymentStage::Preflight);
        assert!(resumed.action_required.is_none());
    }
}
