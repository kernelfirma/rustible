//! Application state management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::auth::{AuthConfig, JwtAuth};
use super::types::{JobInfo, JobStats, JobStatus, WsMessage};
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
            .filter(|j| j.status == JobStatus::Running)
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
            if job.status == JobStatus::Pending || job.status == JobStatus::Running {
                job.status = JobStatus::Cancelled;
                job.finished_at = Some(Utc::now());

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
            (Some(start), None) if self.status == JobStatus::Running => {
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
