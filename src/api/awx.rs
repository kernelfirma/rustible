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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::error::ApiResult;
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
// AwxCompatHandler
// ============================================================================

/// Handler for AWX/Tower API compatible endpoints.
///
/// This handler provides AWX-compatible API responses while using
/// Rustible's internal execution engine.
pub struct AwxCompatHandler {
    state: Arc<AppState>,
}

impl AwxCompatHandler {
    /// Create a new AWX compatibility handler.
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Handle ping request (GET /api/v2/ping/).
    ///
    /// Returns AWX-compatible health/status information.
    pub async fn ping(&self) -> ApiResult<AwxPingResponse> {
        // TODO: Implement actual cluster status
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

    /// Handle job template launch (POST /api/v2/job_templates/<id>/launch/).
    ///
    /// Launches a job template and returns AWX-compatible response.
    pub async fn launch_job_template(
        &self,
        _template_id: i64,
        _request: JobLaunchRequest,
    ) -> ApiResult<JobLaunchResponse> {
        // TODO: Map template_id to playbook
        // TODO: Create job and return AWX-compatible response
        Err(super::error::ApiError::NotFound(
            "Job template launch not yet implemented".to_string(),
        ))
    }

    /// Handle get job details (GET /api/v2/jobs/<id>/).
    ///
    /// Returns AWX-compatible job details.
    pub async fn get_job(&self, _job_id: i64) -> ApiResult<AwxJobDetails> {
        // TODO: Map AWX job ID to internal UUID
        // TODO: Return AWX-compatible job details
        Err(super::error::ApiError::NotFound(
            "Job details not yet implemented".to_string(),
        ))
    }

    /// Handle inventory hosts listing (GET /api/v2/inventories/<id>/hosts/).
    ///
    /// Returns AWX-compatible host listing.
    pub async fn list_inventory_hosts(
        &self,
        _inventory_id: i64,
    ) -> ApiResult<InventoryHostsResponse> {
        // TODO: Map AWX inventory ID to internal inventory
        // TODO: Return AWX-compatible host list
        Err(super::error::ApiError::NotFound(
            "Inventory hosts listing not yet implemented".to_string(),
        ))
    }

    /// Get a reference to the application state.
    pub fn state(&self) -> Arc<AppState> {
        self.state.clone()
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
}
