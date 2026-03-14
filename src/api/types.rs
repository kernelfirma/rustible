//! API request and response types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Authentication Types
// ============================================================================

/// Login request body.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Username
    pub username: String,
    /// Password
    pub password: String,
}

/// Login response with JWT token.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// JWT access token
    pub token: String,
    /// Token type (always "Bearer")
    pub token_type: String,
    /// Expiration time in seconds
    pub expires_in: u64,
}

/// Token refresh request.
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    /// Current valid token
    pub token: String,
}

// ============================================================================
// Playbook Types
// ============================================================================

/// Request to execute a playbook.
#[derive(Debug, Deserialize)]
pub struct PlaybookExecuteRequest {
    /// Path to the playbook file
    pub playbook: String,
    /// Inventory path or pattern (optional, uses default if not provided)
    #[serde(default)]
    pub inventory: Option<String>,
    /// Limit execution to specific hosts/groups
    #[serde(default)]
    pub limit: Option<String>,
    /// Extra variables as key-value pairs
    #[serde(default)]
    pub extra_vars: HashMap<String, serde_json::Value>,
    /// Run in check mode (dry-run)
    #[serde(default)]
    pub check: bool,
    /// Show diffs for changed files
    #[serde(default)]
    pub diff: bool,
    /// Number of parallel forks
    #[serde(default)]
    pub forks: Option<usize>,
    /// Verbosity level (0-4)
    #[serde(default)]
    pub verbosity: u8,
    /// Tags to run
    #[serde(default)]
    pub tags: Vec<String>,
    /// Tags to skip
    #[serde(default)]
    pub skip_tags: Vec<String>,
    /// Start at a specific task
    #[serde(default)]
    pub start_at_task: Option<String>,
}

/// Response when a playbook execution is started.
#[derive(Debug, Serialize)]
pub struct PlaybookExecuteResponse {
    /// Unique job ID
    pub job_id: Uuid,
    /// Job status
    pub status: JobStatus,
    /// Message
    pub message: String,
    /// WebSocket URL for real-time output
    pub websocket_url: Option<String>,
}

/// Internal kernel deployment request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentRequest {
    /// Inline managed-host definitions. This is the preferred portable format.
    pub hosts: Vec<KernelDeploymentHost>,
    /// Signed kernel artifact metadata.
    pub artifact: KernelDeploymentArtifact,
    /// Reboot policy to use after installation.
    #[serde(default)]
    pub reboot_policy: KernelDeploymentRebootPolicy,
}

/// Inline connection and managed-host metadata for a deployment target.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentHost {
    /// Stable host identifier used in status reporting.
    pub name: String,
    /// Hostname or IP address to connect to.
    pub address: String,
    /// SSH port.
    #[serde(default = "default_kernel_host_port")]
    pub port: u16,
    /// SSH username.
    pub username: String,
    /// Optional SSH password.
    #[serde(default)]
    pub password: Option<String>,
    /// Optional inline private key content.
    #[serde(default)]
    pub private_key: Option<String>,
    /// Whether commands should use sudo escalation.
    #[serde(default)]
    pub sudo_enabled: bool,
    /// Secure Boot handling mode for this host.
    #[serde(default)]
    pub secure_boot_mode: KernelSecureBootMode,
    /// Optional bootloader hint supplied by the control plane.
    #[serde(default)]
    pub bootloader_hint: Option<KernelBootloader>,
    /// Optional BMC metadata for action-required recovery.
    #[serde(default)]
    pub bmc: Option<KernelDeploymentBmc>,
}

fn default_kernel_host_port() -> u16 {
    22
}

/// Signed artifact metadata used for a kernel deployment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentArtifact {
    /// Download URL or local file path for the kernel package.
    pub url: String,
    /// Expected SHA-256 digest for the kernel package.
    pub sha256: String,
    /// Debian package name used for rollback/uninstall.
    pub package_name: String,
    /// Expected kernel release after reboot (`uname -r`).
    pub expected_kernel_release: String,
    /// Detached cosign signature for the package blob.
    pub signature_url: String,
    /// Pinned public key used for `cosign verify-blob`.
    pub public_key_url: String,
    /// Expected SHA-256 fingerprint of the public key bytes.
    pub public_key_fingerprint: String,
    /// Optional immutable manifest URL.
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Optional Secure Boot certificate URL.
    #[serde(default)]
    pub secure_boot_cert_url: Option<String>,
    /// Optional SHA-256 fingerprint of the Secure Boot certificate.
    #[serde(default)]
    pub secure_boot_cert_fingerprint: Option<String>,
}

/// Secure Boot handling mode for a host.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelSecureBootMode {
    /// The host should not require Secure Boot orchestration.
    #[default]
    Disabled,
    /// The signing certificate must already be enrolled on the host.
    PreEnrolled,
    /// Enrollment requires an operator and BMC-assisted console flow.
    ConsoleBmc,
}

/// Supported bootloader types for kernel deployments.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelBootloader {
    /// GNU GRUB.
    Grub,
    /// systemd-boot.
    SystemdBoot,
}

/// BMC metadata used for action-required recovery flows.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentBmc {
    /// Power-control provider.
    pub provider: KernelDeploymentBmcProvider,
    /// BMC endpoint or host.
    pub endpoint: String,
    /// Optional BMC username.
    #[serde(default)]
    pub username: Option<String>,
    /// Optional BMC password.
    #[serde(default)]
    pub password: Option<String>,
    /// Whether to verify Redfish TLS certificates.
    #[serde(default)]
    pub verify_tls: bool,
}

/// Supported BMC providers.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelDeploymentBmcProvider {
    /// Redfish over HTTPS.
    Redfish,
    /// IPMI via `ipmitool`.
    Ipmi,
}

/// Current kernel deployment stage.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelDeploymentStage {
    /// Job is queued.
    Queued,
    /// Host preflight and policy checks are running.
    Preflight,
    /// Operator intervention is required before the workflow can continue.
    ActionRequired,
    /// Package installation is in progress.
    Installing,
    /// Host reboot is in progress.
    Rebooting,
    /// Post-boot verification is in progress.
    Verifying,
    /// The new kernel is being committed as the default boot entry.
    Committing,
    /// The workflow is reverting to the prior kernel.
    RollingBack,
    /// All targets completed successfully.
    Succeeded,
    /// The workflow failed.
    Failed,
}

/// Action the control plane must resolve before a deployment can continue.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct KernelDeploymentActionRequired {
    /// Stable identifier used when resuming the job.
    pub action_id: String,
    /// Action type.
    pub kind: KernelDeploymentActionKind,
    /// Host currently blocked.
    pub host: String,
    /// Human-readable summary.
    pub message: String,
    /// Concrete operator instructions.
    #[serde(default)]
    pub instructions: Vec<String>,
    /// Optional BMC information for the operator.
    #[serde(default)]
    pub bmc: Option<KernelDeploymentBmcActionHint>,
}

/// Supported action-required categories.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelDeploymentActionKind {
    /// Secure Boot certificate enrollment through the firmware console.
    SecureBootEnrollment,
}

/// Minimal BMC information exposed in action-required payloads.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct KernelDeploymentBmcActionHint {
    /// Power-control provider.
    pub provider: KernelDeploymentBmcProvider,
    /// BMC endpoint or host.
    pub endpoint: String,
}

/// Current kernel deployment progress.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentProgress {
    /// Current workflow stage.
    pub stage: KernelDeploymentStage,
    /// Host currently being processed, if any.
    #[serde(default)]
    pub current_host: Option<String>,
    /// Inline host inventory known to the job.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Pending operator action, if any.
    #[serde(default)]
    pub action_required: Option<KernelDeploymentActionRequired>,
}

/// Resume request for an action-required deployment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KernelDeploymentResumeRequest {
    /// The action identifier returned by the last status response.
    pub action_id: String,
}

/// Kernel deployment status response for internal callers.
#[derive(Debug, Clone, Serialize)]
pub struct KernelDeploymentStatusResponse {
    /// Unique job ID.
    pub job_id: Uuid,
    /// Top-level job status.
    pub status: JobStatus,
    /// Current kernel deployment progress.
    pub deployment: KernelDeploymentProgress,
    /// Error message if the job has failed.
    pub error: Option<String>,
}

/// Reboot policy for kernel deployments.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelDeploymentRebootPolicy {
    /// Always reboot and verify the new kernel.
    #[default]
    Required,
    /// Skip the reboot and fail verification if the kernel is not active yet.
    Skip,
}

/// Response for an accepted kernel deployment job.
#[derive(Debug, Serialize)]
pub struct KernelDeploymentResponse {
    /// Unique job ID
    pub job_id: Uuid,
    /// Job status
    pub status: JobStatus,
    /// Message
    pub message: String,
    /// WebSocket URL for real-time output
    pub websocket_url: Option<String>,
    /// Initial deployment progress snapshot.
    pub deployment: KernelDeploymentProgress,
}

/// List playbooks response.
#[derive(Debug, Serialize)]
pub struct PlaybookListResponse {
    /// List of available playbooks
    pub playbooks: Vec<PlaybookInfo>,
}

/// Information about a playbook.
#[derive(Debug, Serialize)]
pub struct PlaybookInfo {
    /// Playbook name
    pub name: String,
    /// File path
    pub path: String,
    /// Number of plays
    pub plays: usize,
    /// Last modified timestamp
    pub modified: Option<DateTime<Utc>>,
}

// ============================================================================
// Job Types
// ============================================================================

/// Job status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Job is queued and waiting to run
    Pending,
    /// Job is currently running
    Running,
    /// Job is paused waiting for an operator action.
    ActionRequired,
    /// Job completed successfully
    Success,
    /// Job failed
    Failed,
    /// Job was cancelled
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::ActionRequired => write!(f, "action_required"),
            JobStatus::Success => write!(f, "success"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Job information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    /// Unique job ID
    pub id: Uuid,
    /// Job status
    pub status: JobStatus,
    /// Playbook being executed
    pub playbook: String,
    /// Inventory used
    pub inventory: Option<String>,
    /// Extra variables
    pub extra_vars: HashMap<String, serde_json::Value>,
    /// When the job was created
    pub created_at: DateTime<Utc>,
    /// When the job started running
    pub started_at: Option<DateTime<Utc>>,
    /// When the job finished
    pub finished_at: Option<DateTime<Utc>>,
    /// Duration in seconds
    pub duration_secs: Option<f64>,
    /// User who started the job
    pub user: Option<String>,
}

/// Job details with execution results.
#[derive(Debug, Serialize)]
pub struct JobDetails {
    /// Basic job info
    #[serde(flatten)]
    pub info: JobInfo,
    /// Execution statistics per host
    pub stats: Option<JobStats>,
    /// Output log
    pub output: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Kernel deployment progress for internal deployment jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel_deployment: Option<KernelDeploymentProgress>,
}

/// Execution statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobStats {
    /// Total hosts
    pub hosts: usize,
    /// Tasks that succeeded without changes
    pub ok: usize,
    /// Tasks that made changes
    pub changed: usize,
    /// Tasks that failed
    pub failed: usize,
    /// Tasks that were skipped
    pub skipped: usize,
    /// Unreachable hosts
    pub unreachable: usize,
}

/// Job list response.
#[derive(Debug, Serialize)]
pub struct JobListResponse {
    /// List of jobs
    pub jobs: Vec<JobInfo>,
    /// Total count (for pagination)
    pub total: usize,
    /// Current page
    pub page: usize,
    /// Page size
    pub per_page: usize,
}

/// Job list query parameters.
#[derive(Debug, Default, Deserialize)]
pub struct JobListQuery {
    /// Filter by status
    pub status: Option<JobStatus>,
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: usize,
    /// Items per page
    #[serde(default = "default_per_page")]
    pub per_page: usize,
    /// Sort order (asc or desc)
    #[serde(default)]
    pub order: Option<String>,
}

fn default_page() -> usize {
    1
}

fn default_per_page() -> usize {
    20
}

// ============================================================================
// Inventory Types
// ============================================================================

/// Host information response.
#[derive(Debug, Serialize)]
pub struct HostResponse {
    /// Host name
    pub name: String,
    /// Ansible host (IP/hostname)
    pub ansible_host: Option<String>,
    /// Groups this host belongs to
    pub groups: Vec<String>,
    /// Host variables
    pub vars: HashMap<String, serde_json::Value>,
    /// Connection type
    pub connection: String,
    /// SSH port
    pub port: u16,
    /// SSH user
    pub user: Option<String>,
}

/// Group information response.
#[derive(Debug, Serialize)]
pub struct GroupResponse {
    /// Group name
    pub name: String,
    /// Direct hosts in this group
    pub hosts: Vec<String>,
    /// Child groups
    pub children: Vec<String>,
    /// Parent groups
    pub parents: Vec<String>,
    /// Group variables
    pub vars: HashMap<String, serde_json::Value>,
}

/// Inventory summary response.
#[derive(Debug, Serialize)]
pub struct InventorySummaryResponse {
    /// Total number of hosts
    pub host_count: usize,
    /// Total number of groups
    pub group_count: usize,
    /// List of all host names
    pub hosts: Vec<String>,
    /// List of all group names
    pub groups: Vec<String>,
    /// Inventory source path
    pub source: Option<String>,
}

/// Host list response.
#[derive(Debug, Serialize)]
pub struct HostListResponse {
    /// List of hosts
    pub hosts: Vec<HostResponse>,
    /// Total count
    pub total: usize,
}

/// Group list response.
#[derive(Debug, Serialize)]
pub struct GroupListResponse {
    /// List of groups
    pub groups: Vec<GroupResponse>,
    /// Total count
    pub total: usize,
}

// ============================================================================
// Health Check Types
// ============================================================================

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Service status
    pub status: String,
    /// Version information
    pub version: String,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Number of active jobs
    pub active_jobs: usize,
}

/// API information response.
#[derive(Debug, Serialize)]
pub struct ApiInfoResponse {
    /// API name
    pub name: String,
    /// API version
    pub version: String,
    /// Available endpoints
    pub endpoints: Vec<EndpointInfo>,
}

/// Endpoint information.
#[derive(Debug, Serialize)]
pub struct EndpointInfo {
    /// HTTP method
    pub method: String,
    /// Path
    pub path: String,
    /// Description
    pub description: String,
}

// ============================================================================
// WebSocket Types
// ============================================================================

/// WebSocket message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Job output line
    Output {
        /// Job ID
        job_id: Uuid,
        /// Output line
        line: String,
        /// Stream (stdout or stderr)
        stream: String,
        /// Timestamp
        timestamp: DateTime<Utc>,
    },
    /// Job status change
    StatusChange {
        /// Job ID
        job_id: Uuid,
        /// New status
        status: JobStatus,
        /// Message
        message: Option<String>,
    },
    /// Task started
    TaskStart {
        /// Job ID
        job_id: Uuid,
        /// Task name
        task: String,
        /// Host
        host: String,
    },
    /// Task completed
    TaskComplete {
        /// Job ID
        job_id: Uuid,
        /// Task name
        task: String,
        /// Host
        host: String,
        /// Result status
        result: String,
        /// Whether changed
        changed: bool,
    },
    /// Ping/pong for keepalive
    Ping,
    Pong,
    /// Error message
    Error {
        /// Error message
        message: String,
    },
}

/// WebSocket subscription request.
#[derive(Debug, Deserialize)]
pub struct WsSubscribe {
    /// Job ID to subscribe to
    pub job_id: Uuid,
}
