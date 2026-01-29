//! AWX/Tower API Compatibility Module
//!
//! This module provides API compatibility with Ansible AWX/Tower, enabling
//! integration with existing AWX/Tower tooling and workflows.
//!
//! ## Supported Endpoints (Phase 1)
//!
//! - `GET /api/v2/ping/` - Health check
//! - `GET /api/v2/jobs/<id>/` - Get job details
//! - `POST /api/v2/job_templates/<id>/launch/` - Launch a job template
//! - `GET /api/v2/inventories/<id>/hosts/` - List inventory hosts
//!
//! ## Authentication
//!
//! Supports both token and basic authentication for AWX compatibility.
//!
//! ## Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::api::awx::{AwxCompatHandler, JobLaunchRequest};
//!
//! let handler = AwxCompatHandler::new(state);
//!
//! // Launch a job template
//! let request = JobLaunchRequest {
//!     extra_vars: Some(serde_json::json!({"env": "production"})),
//!     limit: Some("webservers".to_string()),
//!     ..Default::default()
//! };
//!
//! let response = handler.launch_job_template(1, request).await?;
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use super::error::{ApiError, ApiResult};
use super::state::AppState;
use super::types::JobStatus;

// ============================================================================
// API Endpoint Constants (AWX/Tower v2 API)
// ============================================================================

/// AWX API version prefix
pub const AWX_API_VERSION: &str = "/api/v2";

/// Ping endpoint for health checks
pub const ENDPOINT_PING: &str = "/api/v2/ping/";

/// Jobs endpoint base
pub const ENDPOINT_JOBS: &str = "/api/v2/jobs/";

/// Job templates endpoint base
pub const ENDPOINT_JOB_TEMPLATES: &str = "/api/v2/job_templates/";

/// Inventories endpoint base
pub const ENDPOINT_INVENTORIES: &str = "/api/v2/inventories/";

/// Job template launch suffix
pub const ENDPOINT_LAUNCH_SUFFIX: &str = "/launch/";

/// Inventory hosts suffix
pub const ENDPOINT_HOSTS_SUFFIX: &str = "/hosts/";

// ============================================================================
// Request Types
// ============================================================================

/// Request body for launching a job template.
///
/// Mirrors the AWX/Tower job template launch payload for compatibility.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct JobLaunchRequest {
    /// Extra variables to pass to the playbook (as JSON)
    #[serde(default)]
    pub extra_vars: Option<serde_json::Value>,

    /// Limit execution to specific hosts/groups
    #[serde(default)]
    pub limit: Option<String>,

    /// Inventory ID to use (overrides template default)
    #[serde(default)]
    pub inventory: Option<i64>,

    /// Credential ID to use (overrides template default)
    #[serde(default)]
    pub credential: Option<i64>,

    /// Job tags to run
    #[serde(default)]
    pub job_tags: Option<String>,

    /// Tags to skip
    #[serde(default)]
    pub skip_tags: Option<String>,

    /// Job type: "run" or "check"
    #[serde(default)]
    pub job_type: Option<String>,

    /// Verbosity level (0-5)
    #[serde(default)]
    pub verbosity: Option<u8>,

    /// Diff mode
    #[serde(default)]
    pub diff_mode: Option<bool>,

    /// Fork count
    #[serde(default)]
    pub forks: Option<u32>,

    /// Execution environment ID
    #[serde(default)]
    pub execution_environment: Option<i64>,

    /// Labels to apply to the job
    #[serde(default)]
    pub labels: Option<Vec<i64>>,

    /// Timeout in seconds
    #[serde(default)]
    pub timeout: Option<u64>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Response from launching a job template.
///
/// Mirrors the AWX/Tower job launch response format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobLaunchResponse {
    /// Job ID (AWX uses integers, we use UUID internally)
    pub id: i64,

    /// Job URL
    pub url: String,

    /// Related URLs
    pub related: JobRelatedUrls,

    /// Job type
    #[serde(rename = "type")]
    pub job_type: String,

    /// Job name
    pub name: String,

    /// Job status
    pub status: AwxJobStatus,

    /// When the job was created
    pub created: DateTime<Utc>,

    /// When the job was last modified
    pub modified: DateTime<Utc>,

    /// When the job started
    pub started: Option<DateTime<Utc>>,

    /// When the job finished
    pub finished: Option<DateTime<Utc>>,

    /// Whether the job was cancelled
    pub canceled_on: Option<DateTime<Utc>>,

    /// Playbook being executed
    pub playbook: String,

    /// Extra variables (as string)
    pub extra_vars: String,

    /// Job template ID
    pub job_template: i64,

    /// Inventory ID
    pub inventory: Option<i64>,

    /// Whether launch was successful
    pub launch_success: bool,

    /// Ignored fields for AWX compatibility
    #[serde(default)]
    pub ignored_fields: HashMap<String, serde_json::Value>,
}

/// Related URLs in AWX responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobRelatedUrls {
    /// URL to get job details
    #[serde(default)]
    pub job: Option<String>,

    /// URL to get job stdout
    #[serde(default)]
    pub stdout: Option<String>,

    /// URL to cancel the job
    #[serde(default)]
    pub cancel: Option<String>,

    /// URL to relaunch the job
    #[serde(default)]
    pub relaunch: Option<String>,

    /// URL to get job events
    #[serde(default)]
    pub job_events: Option<String>,

    /// URL to get job host summaries
    #[serde(default)]
    pub job_host_summaries: Option<String>,
}

/// AWX-compatible job status.
///
/// Maps internal job statuses to AWX status strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AwxJobStatus {
    /// Job is new/pending
    New,
    /// Job is pending in queue
    Pending,
    /// Job is waiting for dependencies
    Waiting,
    /// Job is running
    Running,
    /// Job completed successfully
    Successful,
    /// Job failed
    Failed,
    /// Job had errors
    Error,
    /// Job was canceled
    Canceled,
}

impl From<JobStatus> for AwxJobStatus {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => AwxJobStatus::Pending,
            JobStatus::Running => AwxJobStatus::Running,
            JobStatus::Success => AwxJobStatus::Successful,
            JobStatus::Failed => AwxJobStatus::Failed,
            JobStatus::Cancelled => AwxJobStatus::Canceled,
        }
    }
}

/// AWX ping response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwxPingResponse {
    /// Instance group capacities
    pub instance_groups: Vec<InstanceGroupInfo>,

    /// Cluster instances
    pub instances: Vec<InstanceInfo>,

    /// HA status
    pub ha: bool,

    /// Version string
    pub version: String,

    /// Active node
    pub active_node: String,
}

/// Instance group information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceGroupInfo {
    /// Instance group name
    pub name: String,

    /// Capacity
    pub capacity: u32,

    /// Number of instances
    pub instances: u32,
}

/// Instance information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Instance node name
    pub node: String,

    /// Node type
    pub node_type: String,

    /// Instance UUID
    pub uuid: String,

    /// Heartbeat timestamp
    pub heartbeat: DateTime<Utc>,

    /// Capacity
    pub capacity: u32,

    /// Version
    pub version: String,
}

/// AWX job details response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwxJobDetails {
    /// Job ID
    pub id: i64,

    /// Job type
    #[serde(rename = "type")]
    pub job_type: String,

    /// Job URL
    pub url: String,

    /// Related URLs
    pub related: JobRelatedUrls,

    /// Summary fields
    pub summary_fields: JobSummaryFields,

    /// Job name
    pub name: String,

    /// Description
    pub description: String,

    /// Status
    pub status: AwxJobStatus,

    /// Whether the job failed
    pub failed: bool,

    /// Started timestamp
    pub started: Option<DateTime<Utc>>,

    /// Finished timestamp
    pub finished: Option<DateTime<Utc>>,

    /// Elapsed time in seconds
    pub elapsed: f64,

    /// Playbook name
    pub playbook: String,

    /// Extra variables
    pub extra_vars: String,

    /// Limit
    pub limit: Option<String>,

    /// Verbosity level
    pub verbosity: u8,

    /// Result stdout
    pub result_stdout: Option<String>,

    /// Job template ID
    pub job_template: Option<i64>,
}

/// Summary fields in AWX responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobSummaryFields {
    /// Job template summary
    #[serde(default)]
    pub job_template: Option<JobTemplateSummary>,

    /// Inventory summary
    #[serde(default)]
    pub inventory: Option<InventorySummary>,

    /// Created by user
    #[serde(default)]
    pub created_by: Option<UserSummary>,
}

/// Job template summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTemplateSummary {
    /// Template ID
    pub id: i64,
    /// Template name
    pub name: String,
    /// Description
    pub description: String,
}

/// Inventory summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventorySummary {
    /// Inventory ID
    pub id: i64,
    /// Inventory name
    pub name: String,
    /// Description
    pub description: String,
    /// Total hosts
    pub total_hosts: u32,
}

/// User summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    /// User ID
    pub id: i64,
    /// Username
    pub username: String,
}

/// Host in inventory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwxHost {
    /// Host ID
    pub id: i64,

    /// Host type
    #[serde(rename = "type")]
    pub host_type: String,

    /// Host URL
    pub url: String,

    /// Host name
    pub name: String,

    /// Description
    pub description: String,

    /// Inventory ID
    pub inventory: i64,

    /// Whether host is enabled
    pub enabled: bool,

    /// Host variables (JSON string)
    pub variables: String,

    /// Last job status
    pub last_job: Option<i64>,

    /// Last job host summary ID
    pub last_job_host_summary: Option<i64>,
}

/// Inventory hosts list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryHostsResponse {
    /// Total count
    pub count: u32,

    /// Next page URL
    pub next: Option<String>,

    /// Previous page URL
    pub previous: Option<String>,

    /// List of hosts
    pub results: Vec<AwxHost>,
}

// ============================================================================
// Job Template Storage
// ============================================================================

/// Job template configuration.
///
/// Stores the configuration for a job template that can be launched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTemplate {
    /// Template ID (AWX-style integer ID)
    pub id: i64,
    /// Template name
    pub name: String,
    /// Description
    pub description: String,
    /// Playbook path
    pub playbook: String,
    /// Default inventory ID
    pub inventory: Option<i64>,
    /// Default extra variables
    pub extra_vars: Option<serde_json::Value>,
    /// Default limit
    pub limit: Option<String>,
    /// Default job tags
    pub job_tags: Option<String>,
    /// Default skip tags
    pub skip_tags: Option<String>,
    /// Default verbosity (0-5)
    pub verbosity: u8,
    /// Default forks
    pub forks: u32,
    /// Default timeout
    pub timeout: Option<u64>,
    /// Whether to use become
    pub become_enabled: bool,
    /// Allow launching with custom extra_vars
    pub ask_variables_on_launch: bool,
    /// Allow launching with custom inventory
    pub ask_inventory_on_launch: bool,
    /// Allow launching with custom limit
    pub ask_limit_on_launch: bool,
    /// Created timestamp
    pub created: DateTime<Utc>,
    /// Last modified timestamp
    pub modified: DateTime<Utc>,
}

impl JobTemplate {
    /// Create a new job template
    pub fn new(id: i64, name: String, playbook: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            description: String::new(),
            playbook,
            inventory: None,
            extra_vars: None,
            limit: None,
            job_tags: None,
            skip_tags: None,
            verbosity: 0,
            forks: 5,
            timeout: None,
            become_enabled: false,
            ask_variables_on_launch: true,
            ask_inventory_on_launch: true,
            ask_limit_on_launch: true,
            created: now,
            modified: now,
        }
    }
}

/// AWX inventory configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwxInventory {
    /// Inventory ID (AWX-style integer ID)
    pub id: i64,
    /// Inventory name
    pub name: String,
    /// Description
    pub description: String,
    /// Inventory source path
    pub source_path: Option<String>,
    /// Total hosts (cached)
    pub total_hosts: u32,
    /// Total groups (cached)
    pub total_groups: u32,
    /// Created timestamp
    pub created: DateTime<Utc>,
    /// Last modified timestamp
    pub modified: DateTime<Utc>,
}

impl AwxInventory {
    /// Create a new AWX inventory
    pub fn new(id: i64, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            description: String::new(),
            source_path: None,
            total_hosts: 0,
            total_groups: 0,
            created: now,
            modified: now,
        }
    }
}

// ============================================================================
// ID Mapping
// ============================================================================

/// Maps AWX integer IDs to internal UUIDs and vice versa.
pub struct AwxIdMapper {
    /// Counter for generating new AWX IDs
    next_id: AtomicI64,
    /// UUID to AWX ID mapping
    uuid_to_awx: RwLock<HashMap<Uuid, i64>>,
    /// AWX ID to UUID mapping
    awx_to_uuid: RwLock<HashMap<i64, Uuid>>,
}

impl AwxIdMapper {
    /// Create a new ID mapper
    pub fn new() -> Self {
        Self {
            next_id: AtomicI64::new(1),
            uuid_to_awx: RwLock::new(HashMap::new()),
            awx_to_uuid: RwLock::new(HashMap::new()),
        }
    }

    /// Register a UUID and get its AWX ID
    pub fn register_uuid(&self, uuid: Uuid) -> i64 {
        // Check if already registered
        if let Some(&id) = self.uuid_to_awx.read().get(&uuid) {
            return id;
        }

        // Generate new ID
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.uuid_to_awx.write().insert(uuid, id);
        self.awx_to_uuid.write().insert(id, uuid);
        id
    }

    /// Get AWX ID for a UUID
    pub fn get_awx_id(&self, uuid: &Uuid) -> Option<i64> {
        self.uuid_to_awx.read().get(uuid).copied()
    }

    /// Get UUID for an AWX ID
    pub fn get_uuid(&self, awx_id: i64) -> Option<Uuid> {
        self.awx_to_uuid.read().get(&awx_id).copied()
    }
}

impl Default for AwxIdMapper {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// AwxCompatHandler
// ============================================================================

/// Handler for AWX/Tower API compatible endpoints.
///
/// This handler provides AWX-compatible API responses while using
/// Rustible's internal execution engine.
pub struct AwxCompatHandler {
    state: Arc<AppState>,
    /// Job templates storage
    templates: RwLock<HashMap<i64, JobTemplate>>,
    /// Inventory storage
    inventories: RwLock<HashMap<i64, AwxInventory>>,
    /// ID mapper for jobs
    job_id_mapper: AwxIdMapper,
    /// Counter for template IDs
    next_template_id: AtomicI64,
    /// Counter for inventory IDs
    next_inventory_id: AtomicI64,
}

impl AwxCompatHandler {
    /// Create a new AWX compatibility handler.
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            templates: RwLock::new(HashMap::new()),
            inventories: RwLock::new(HashMap::new()),
            job_id_mapper: AwxIdMapper::new(),
            next_template_id: AtomicI64::new(1),
            next_inventory_id: AtomicI64::new(1),
        }
    }

    /// Handle ping request (GET /api/v2/ping/).
    ///
    /// Returns AWX-compatible health/status information.
    pub async fn ping(&self) -> ApiResult<AwxPingResponse> {
        Ok(AwxPingResponse {
            instance_groups: vec![InstanceGroupInfo {
                name: "default".to_string(),
                capacity: 100,
                instances: 1,
            }],
            instances: vec![InstanceInfo {
                node: "rustible".to_string(),
                node_type: "hybrid".to_string(),
                uuid: Uuid::new_v4().to_string(),
                heartbeat: Utc::now(),
                capacity: 100,
                version: crate::version().to_string(),
            }],
            ha: false,
            version: crate::version().to_string(),
            active_node: "rustible".to_string(),
        })
    }

    /// Register a job template.
    ///
    /// Creates a new job template that can be launched via the AWX API.
    pub fn register_template(&self, name: String, playbook: String) -> i64 {
        let id = self.next_template_id.fetch_add(1, Ordering::SeqCst);
        let template = JobTemplate::new(id, name, playbook);
        self.templates.write().insert(id, template);
        id
    }

    /// Get a job template by ID.
    pub fn get_template(&self, id: i64) -> Option<JobTemplate> {
        self.templates.read().get(&id).cloned()
    }

    /// Update a job template.
    pub fn update_template(&self, id: i64, updater: impl FnOnce(&mut JobTemplate)) -> bool {
        if let Some(template) = self.templates.write().get_mut(&id) {
            updater(template);
            template.modified = Utc::now();
            true
        } else {
            false
        }
    }

    /// List all job templates.
    pub fn list_templates(&self) -> Vec<JobTemplate> {
        self.templates.read().values().cloned().collect()
    }

    /// Register an inventory.
    ///
    /// Creates a new inventory that can be referenced by templates.
    pub fn register_inventory(&self, name: String, source_path: Option<String>) -> i64 {
        let id = self.next_inventory_id.fetch_add(1, Ordering::SeqCst);
        let mut inventory = AwxInventory::new(id, name);
        inventory.source_path = source_path;
        self.inventories.write().insert(id, inventory);
        id
    }

    /// Get an inventory by ID.
    pub fn get_inventory(&self, id: i64) -> Option<AwxInventory> {
        self.inventories.read().get(&id).cloned()
    }

    /// Sync inventory hosts from internal inventory.
    ///
    /// Updates the AWX inventory with hosts from the application state.
    pub fn sync_inventory(&self, inventory_id: i64) -> ApiResult<()> {
        let awx_inv = self.inventories.read().get(&inventory_id).cloned();
        let awx_inv = awx_inv
            .ok_or_else(|| ApiError::NotFound(format!("Inventory {} not found", inventory_id)))?;

        // Load inventory from source path
        if let Some(ref source_path) = awx_inv.source_path {
            match crate::inventory::Inventory::load(source_path) {
                Ok(inv) => {
                    // Update host and group counts
                    let host_count = inv.host_count() as u32;
                    let group_count = inv.group_count() as u32;

                    if let Some(inv_ref) = self.inventories.write().get_mut(&inventory_id) {
                        inv_ref.total_hosts = host_count;
                        inv_ref.total_groups = group_count;
                        inv_ref.modified = Utc::now();
                    }

                    // Store in app state for use by jobs
                    self.state.set_inventory(inv);
                    Ok(())
                }
                Err(e) => Err(ApiError::Internal(format!(
                    "Failed to load inventory: {}",
                    e
                ))),
            }
        } else {
            Err(ApiError::BadRequest(
                "Inventory has no source path configured".to_string(),
            ))
        }
    }

    /// Handle job template launch (POST /api/v2/job_templates/<id>/launch/).
    ///
    /// Launches a job template and returns AWX-compatible response.
    pub async fn launch_job_template(
        &self,
        template_id: i64,
        request: JobLaunchRequest,
    ) -> ApiResult<JobLaunchResponse> {
        // Get the template
        let template = self.templates.read().get(&template_id).cloned();
        let template = template
            .ok_or_else(|| ApiError::NotFound(format!("Job template {} not found", template_id)))?;

        // Merge extra vars (template defaults + request overrides)
        let mut extra_vars: HashMap<String, serde_json::Value> = HashMap::new();
        if let Some(template_vars) = &template.extra_vars {
            if let Some(obj) = template_vars.as_object() {
                for (k, v) in obj {
                    extra_vars.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(request_vars) = &request.extra_vars {
            if let Some(obj) = request_vars.as_object() {
                for (k, v) in obj {
                    extra_vars.insert(k.clone(), v.clone());
                }
            }
        }

        // Determine inventory path
        let inventory_path = if let Some(inv_id) = request.inventory.or(template.inventory) {
            self.inventories
                .read()
                .get(&inv_id)
                .and_then(|inv| inv.source_path.clone())
        } else {
            None
        };

        // Create the job
        let job_uuid = self.state.create_job(
            template.playbook.clone(),
            inventory_path,
            None, // user
            extra_vars,
        );

        // Register the UUID with AWX ID mapper
        let awx_job_id = self.job_id_mapper.register_uuid(job_uuid);

        // Update job status to running (in real implementation, this would be async)
        self.state.update_job_status(job_uuid, JobStatus::Running);

        let now = Utc::now();

        // Build response
        let response = JobLaunchResponse {
            id: awx_job_id,
            url: format!("{}{}/", ENDPOINT_JOBS, awx_job_id),
            related: JobRelatedUrls {
                job: Some(format!("{}{}/", ENDPOINT_JOBS, awx_job_id)),
                stdout: Some(format!("{}{}stdout/", ENDPOINT_JOBS, awx_job_id)),
                cancel: Some(format!("{}{}cancel/", ENDPOINT_JOBS, awx_job_id)),
                relaunch: Some(format!("{}{}relaunch/", ENDPOINT_JOBS, awx_job_id)),
                job_events: Some(format!("{}{}job_events/", ENDPOINT_JOBS, awx_job_id)),
                job_host_summaries: Some(format!(
                    "{}{}job_host_summaries/",
                    ENDPOINT_JOBS, awx_job_id
                )),
            },
            job_type: "job".to_string(),
            name: template.name.clone(),
            status: AwxJobStatus::Running,
            created: now,
            modified: now,
            started: Some(now),
            finished: None,
            canceled_on: None,
            playbook: template.playbook,
            extra_vars: serde_json::to_string(&request.extra_vars).unwrap_or_default(),
            job_template: template_id,
            inventory: request.inventory.or(template.inventory),
            launch_success: true,
            ignored_fields: HashMap::new(),
        };

        Ok(response)
    }

    /// Handle get job details (GET /api/v2/jobs/<id>/).
    ///
    /// Returns AWX-compatible job details.
    pub async fn get_job(&self, awx_job_id: i64) -> ApiResult<AwxJobDetails> {
        // Map AWX ID to UUID
        let job_uuid = self
            .job_id_mapper
            .get_uuid(awx_job_id)
            .ok_or_else(|| ApiError::NotFound(format!("Job {} not found", awx_job_id)))?;

        // Get the internal job
        let job = self
            .state
            .get_job(job_uuid)
            .ok_or_else(|| ApiError::NotFound(format!("Job {} not found", awx_job_id)))?;

        // Calculate elapsed time
        let elapsed = match (job.started_at, job.finished_at) {
            (Some(start), Some(end)) => (end - start).num_milliseconds() as f64 / 1000.0,
            (Some(start), None) => (Utc::now() - start).num_milliseconds() as f64 / 1000.0,
            _ => 0.0,
        };

        // Build AWX-compatible response
        let details = AwxJobDetails {
            id: awx_job_id,
            job_type: "job".to_string(),
            url: format!("{}{}/", ENDPOINT_JOBS, awx_job_id),
            related: JobRelatedUrls {
                job: Some(format!("{}{}/", ENDPOINT_JOBS, awx_job_id)),
                stdout: Some(format!("{}{}stdout/", ENDPOINT_JOBS, awx_job_id)),
                cancel: Some(format!("{}{}cancel/", ENDPOINT_JOBS, awx_job_id)),
                relaunch: Some(format!("{}{}relaunch/", ENDPOINT_JOBS, awx_job_id)),
                job_events: None,
                job_host_summaries: None,
            },
            summary_fields: JobSummaryFields {
                job_template: None, // Would need to track which template launched this job
                inventory: None,
                created_by: job.user.as_ref().map(|u| UserSummary {
                    id: 1,
                    username: u.clone(),
                }),
            },
            name: format!("Job {}", awx_job_id),
            description: String::new(),
            status: AwxJobStatus::from(job.status),
            failed: job.status == JobStatus::Failed,
            started: job.started_at,
            finished: job.finished_at,
            elapsed,
            playbook: job.playbook.clone(),
            extra_vars: serde_json::to_string(&job.extra_vars).unwrap_or_default(),
            limit: None,
            verbosity: 0,
            result_stdout: Some(job.full_output()),
            job_template: None,
        };

        Ok(details)
    }

    /// Handle inventory hosts listing (GET /api/v2/inventories/<id>/hosts/).
    ///
    /// Returns AWX-compatible host listing.
    pub async fn list_inventory_hosts(
        &self,
        inventory_id: i64,
    ) -> ApiResult<InventoryHostsResponse> {
        // Get AWX inventory
        let awx_inv = self.inventories.read().get(&inventory_id).cloned();
        let awx_inv = awx_inv
            .ok_or_else(|| ApiError::NotFound(format!("Inventory {} not found", inventory_id)))?;

        // Get internal inventory from state
        let inventory = self
            .state
            .get_inventory()
            .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

        // Convert hosts to AWX format
        let mut host_id = 1i64;
        let hosts: Vec<AwxHost> = inventory
            .get_all_hosts()
            .iter()
            .map(|host| {
                let id = host_id;
                host_id += 1;

                // Get host variables as JSON string
                let vars = serde_json::to_string(&host.vars).unwrap_or_else(|_| "{}".to_string());

                AwxHost {
                    id,
                    host_type: "host".to_string(),
                    url: format!("/api/v2/hosts/{}/", id),
                    name: host.name.clone(),
                    description: String::new(),
                    inventory: inventory_id,
                    enabled: true,
                    variables: vars,
                    last_job: None,
                    last_job_host_summary: None,
                }
            })
            .collect();

        let count = hosts.len() as u32;

        Ok(InventoryHostsResponse {
            count,
            next: None,
            previous: None,
            results: hosts,
        })
    }

    /// Get a reference to the application state.
    pub fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }

    /// Get the AWX ID for an internal job UUID.
    pub fn get_job_awx_id(&self, uuid: &Uuid) -> Option<i64> {
        self.job_id_mapper.get_awx_id(uuid)
    }

    /// Get the internal UUID for an AWX job ID.
    pub fn get_job_uuid(&self, awx_id: i64) -> Option<Uuid> {
        self.job_id_mapper.get_uuid(awx_id)
    }
}

impl std::fmt::Debug for AwxCompatHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwxCompatHandler").finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_awx_job_status_conversion() {
        assert_eq!(
            AwxJobStatus::from(JobStatus::Pending),
            AwxJobStatus::Pending
        );
        assert_eq!(
            AwxJobStatus::from(JobStatus::Running),
            AwxJobStatus::Running
        );
        assert_eq!(
            AwxJobStatus::from(JobStatus::Success),
            AwxJobStatus::Successful
        );
        assert_eq!(AwxJobStatus::from(JobStatus::Failed), AwxJobStatus::Failed);
        assert_eq!(
            AwxJobStatus::from(JobStatus::Cancelled),
            AwxJobStatus::Canceled
        );
    }

    #[test]
    fn test_job_launch_request_default() {
        let request = JobLaunchRequest::default();
        assert!(request.extra_vars.is_none());
        assert!(request.limit.is_none());
        assert!(request.inventory.is_none());
    }

    #[test]
    fn test_job_launch_request_deserialization() {
        let json = r#"{
            "extra_vars": {"env": "production"},
            "limit": "webservers",
            "verbosity": 2
        }"#;

        let request: JobLaunchRequest = serde_json::from_str(json).unwrap();
        assert!(request.extra_vars.is_some());
        assert_eq!(request.limit, Some("webservers".to_string()));
        assert_eq!(request.verbosity, Some(2));
    }

    #[test]
    fn test_endpoint_constants() {
        assert_eq!(ENDPOINT_PING, "/api/v2/ping/");
        assert_eq!(ENDPOINT_JOBS, "/api/v2/jobs/");
        assert_eq!(ENDPOINT_JOB_TEMPLATES, "/api/v2/job_templates/");
        assert_eq!(ENDPOINT_INVENTORIES, "/api/v2/inventories/");
    }

    #[test]
    fn test_awx_ping_response_serialization() {
        let response = AwxPingResponse {
            instance_groups: vec![InstanceGroupInfo {
                name: "default".to_string(),
                capacity: 100,
                instances: 1,
            }],
            instances: vec![],
            ha: false,
            version: "1.0.0".to_string(),
            active_node: "node1".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"ha\":false"));
        assert!(json.contains("\"version\":\"1.0.0\""));
    }

    #[test]
    fn test_job_related_urls_default() {
        let urls = JobRelatedUrls::default();
        assert!(urls.job.is_none());
        assert!(urls.stdout.is_none());
        assert!(urls.cancel.is_none());
    }

    #[test]
    fn test_job_template_creation() {
        let template = JobTemplate::new(1, "test-template".to_string(), "site.yml".to_string());

        assert_eq!(template.id, 1);
        assert_eq!(template.name, "test-template");
        assert_eq!(template.playbook, "site.yml");
        assert!(template.inventory.is_none());
        assert_eq!(template.verbosity, 0);
        assert_eq!(template.forks, 5);
        assert!(template.ask_variables_on_launch);
    }

    #[test]
    fn test_awx_inventory_creation() {
        let inventory = AwxInventory::new(1, "production".to_string());

        assert_eq!(inventory.id, 1);
        assert_eq!(inventory.name, "production");
        assert!(inventory.source_path.is_none());
        assert_eq!(inventory.total_hosts, 0);
        assert_eq!(inventory.total_groups, 0);
    }

    #[test]
    fn test_awx_id_mapper() {
        let mapper = AwxIdMapper::new();
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();

        // Register UUIDs and get AWX IDs
        let id1 = mapper.register_uuid(uuid1);
        let id2 = mapper.register_uuid(uuid2);

        // IDs should be unique
        assert_ne!(id1, id2);

        // Registering same UUID should return same ID
        assert_eq!(mapper.register_uuid(uuid1), id1);

        // Lookups should work both ways
        assert_eq!(mapper.get_awx_id(&uuid1), Some(id1));
        assert_eq!(mapper.get_uuid(id1), Some(uuid1));
        assert_eq!(mapper.get_awx_id(&uuid2), Some(id2));
        assert_eq!(mapper.get_uuid(id2), Some(uuid2));

        // Unknown IDs should return None
        assert_eq!(mapper.get_uuid(999), None);
        assert_eq!(mapper.get_awx_id(&Uuid::new_v4()), None);
    }

    #[test]
    fn test_awx_host_serialization() {
        let host = AwxHost {
            id: 1,
            host_type: "host".to_string(),
            url: "/api/v2/hosts/1/".to_string(),
            name: "webserver1".to_string(),
            description: "Web server 1".to_string(),
            inventory: 1,
            enabled: true,
            variables: r#"{"ansible_host": "192.168.1.10"}"#.to_string(),
            last_job: None,
            last_job_host_summary: None,
        };

        let json = serde_json::to_string(&host).unwrap();
        assert!(json.contains("\"name\":\"webserver1\""));
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"inventory\":1"));
    }

    #[test]
    fn test_inventory_hosts_response() {
        let response = InventoryHostsResponse {
            count: 2,
            next: None,
            previous: None,
            results: vec![
                AwxHost {
                    id: 1,
                    host_type: "host".to_string(),
                    url: "/api/v2/hosts/1/".to_string(),
                    name: "host1".to_string(),
                    description: String::new(),
                    inventory: 1,
                    enabled: true,
                    variables: "{}".to_string(),
                    last_job: None,
                    last_job_host_summary: None,
                },
                AwxHost {
                    id: 2,
                    host_type: "host".to_string(),
                    url: "/api/v2/hosts/2/".to_string(),
                    name: "host2".to_string(),
                    description: String::new(),
                    inventory: 1,
                    enabled: true,
                    variables: "{}".to_string(),
                    last_job: None,
                    last_job_host_summary: None,
                },
            ],
        };

        assert_eq!(response.count, 2);
        assert_eq!(response.results.len(), 2);
        assert!(response.next.is_none());
    }

    #[test]
    fn test_job_summary_fields_default() {
        let summary = JobSummaryFields::default();
        assert!(summary.job_template.is_none());
        assert!(summary.inventory.is_none());
        assert!(summary.created_by.is_none());
    }

    #[test]
    fn test_job_template_summary_serialization() {
        let summary = JobTemplateSummary {
            id: 1,
            name: "Deploy App".to_string(),
            description: "Deploy the application".to_string(),
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"name\":\"Deploy App\""));
    }
}
