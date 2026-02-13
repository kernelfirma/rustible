//! Unified HPC scheduler abstraction layer
//!
//! Provides a common trait interface (`HpcScheduler`) and shared types so that
//! playbooks can use `hpc_job`, `hpc_queue`, and `hpc_server` modules with
//! either Slurm or PBS Pro without scheduler-specific parameters.
//!
//! # Auto-detection
//!
//! When `scheduler` is set to `"auto"` (the default), `resolve_scheduler`
//! probes the remote host for `scontrol` (Slurm) or `qstat` (PBS) and
//! selects the appropriate backend.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
};

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

/// Common job states across schedulers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Slurm PENDING, PBS Q
    Queued,
    /// Slurm RUNNING, PBS R
    Running,
    /// Slurm HELD, PBS H
    Held,
    /// Slurm SUSPENDED, PBS S
    Suspended,
    /// Slurm COMPLETED, PBS F (exit 0)
    Completed,
    /// Slurm FAILED, PBS F (non-zero exit)
    Failed,
    /// Slurm CANCELLED, PBS deleted
    Cancelled,
    /// Any state that does not map to the above
    Unknown(String),
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobState::Queued => write!(f, "queued"),
            JobState::Running => write!(f, "running"),
            JobState::Held => write!(f, "held"),
            JobState::Suspended => write!(f, "suspended"),
            JobState::Completed => write!(f, "completed"),
            JobState::Failed => write!(f, "failed"),
            JobState::Cancelled => write!(f, "cancelled"),
            JobState::Unknown(s) => write!(f, "unknown({})", s),
        }
    }
}

/// Scheduler-agnostic job information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: String,
    pub name: Option<String>,
    pub state: JobState,
    pub queue: Option<String>,
    pub owner: Option<String>,
    pub nodes: Option<u32>,
    pub cpus: Option<u32>,
    pub walltime_limit: Option<String>,
    pub walltime_used: Option<String>,
    pub raw: serde_json::Value,
}

/// Scheduler-agnostic queue information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueInfo {
    pub name: String,
    /// Normalized state: `"active"` or `"inactive"`.
    pub state: String,
    pub total_jobs: Option<u32>,
    pub raw: serde_json::Value,
}

/// Scheduler-agnostic server/cluster information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// `"slurm"` or `"pbs"`
    pub scheduler: String,
    pub attributes: HashMap<String, String>,
    pub raw: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait implemented by each scheduler backend (Slurm, PBS).
pub trait HpcScheduler: Send + Sync {
    /// Returns `"slurm"` or `"pbs"`.
    fn scheduler_name(&self) -> &'static str;

    fn submit_job(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput>;

    fn cancel_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput>;

    fn job_status(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<JobInfo>;

    fn hold_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput>;

    fn release_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput>;

    fn list_queues(&self, context: &ModuleContext) -> ModuleResult<Vec<QueueInfo>>;

    fn create_queue(
        &self,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput>;

    fn delete_queue(&self, name: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput>;

    fn query_server(&self, context: &ModuleContext) -> ModuleResult<ServerInfo>;

    fn set_server_attributes(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput>;
}

// ---------------------------------------------------------------------------
// Shared helpers (reused by scheduler backends)
// ---------------------------------------------------------------------------

pub(crate) fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
    let mut options = ExecuteOptions::new();
    if context.r#become {
        options = options.with_escalation(context.become_user.clone());
        if let Some(ref method) = context.become_method {
            options.escalate_method = Some(method.clone());
        }
        if let Some(ref password) = context.become_password {
            options.escalate_password = Some(password.clone());
        }
    }
    options
}

pub(crate) fn run_cmd(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<(bool, String, String)> {
    let options = get_exec_options(context);
    let result = Handle::current()
        .block_on(async { connection.execute(cmd, Some(options)).await })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;
    Ok((result.success, result.stdout, result.stderr))
}

pub(crate) fn run_cmd_ok(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
    if !success {
        return Err(ModuleError::ExecutionFailed(format!(
            "Command failed: {}",
            stderr.trim()
        )));
    }
    Ok(stdout)
}

// ---------------------------------------------------------------------------
// State mapping helpers
// ---------------------------------------------------------------------------

/// Map a Slurm state string to a [`JobState`].
pub fn map_slurm_state(state: &str) -> JobState {
    match state.to_uppercase().as_str() {
        "PENDING" | "PD" => JobState::Queued,
        "RUNNING" | "R" => JobState::Running,
        "SUSPENDED" | "S" => JobState::Suspended,
        "COMPLETED" | "CD" => JobState::Completed,
        "FAILED" | "F" => JobState::Failed,
        "CANCELLED" | "CA" => JobState::Cancelled,
        "TIMEOUT" | "TO" => JobState::Failed,
        "NODE_FAIL" | "NF" => JobState::Failed,
        "PREEMPTED" | "PR" => JobState::Cancelled,
        // Slurm uses "HELD" as an informal state but the scontrol state
        // is actually "PENDING" with a hold reason.  When we see a hold
        // reason in `scontrol show job` we pass the literal "HELD".
        "HELD" => JobState::Held,
        other => JobState::Unknown(other.to_string()),
    }
}

/// Map a PBS one-character state to a [`JobState`].
pub fn map_pbs_state(state: &str) -> JobState {
    match state.trim() {
        "Q" | "W" => JobState::Queued,
        "R" | "E" | "B" => JobState::Running,
        "H" => JobState::Held,
        "S" | "U" | "T" => JobState::Suspended,
        "F" => JobState::Completed, // caller should refine to Failed if exit!=0
        "X" => JobState::Cancelled,
        other => JobState::Unknown(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Scheduler resolution
// ---------------------------------------------------------------------------

/// Detect the scheduler installed on the remote host by checking for
/// `scontrol` (Slurm) and `qstat` (PBS) on `$PATH`.
pub fn detect_scheduler(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<Box<dyn HpcScheduler>> {
    // Check for Slurm
    #[cfg(feature = "slurm")]
    {
        let (ok, _, _) = run_cmd(connection, "which scontrol 2>/dev/null", context)?;
        if ok {
            return Ok(Box::new(super::scheduler_slurm::SlurmScheduler));
        }
    }

    // Check for PBS
    #[cfg(feature = "pbs")]
    {
        let (ok, _, _) = run_cmd(connection, "which qstat 2>/dev/null", context)?;
        if ok {
            return Ok(Box::new(super::scheduler_pbs::PbsScheduler));
        }
    }

    #[cfg(not(any(feature = "slurm", feature = "pbs")))]
    {
        let _ = (connection, context);
    }

    Err(ModuleError::ExecutionFailed(
        "No supported scheduler detected on the remote host. \
         Ensure Slurm (scontrol) or PBS Pro (qstat) is installed and on $PATH, \
         and that rustible was compiled with the corresponding feature (slurm / pbs)."
            .to_string(),
    ))
}

/// Resolve a scheduler from the `scheduler` parameter.
///
/// Accepted values: `"slurm"`, `"pbs"`, `"auto"` (default).
pub fn resolve_scheduler(
    params: &ModuleParams,
    context: &ModuleContext,
) -> ModuleResult<Box<dyn HpcScheduler>> {
    use crate::modules::ParamExt;

    let scheduler_param = params
        .get_string("scheduler")?
        .unwrap_or_else(|| "auto".to_string());

    match scheduler_param.to_lowercase().as_str() {
        "auto" => {
            let connection = context
                .connection
                .as_ref()
                .ok_or_else(|| {
                    ModuleError::ExecutionFailed("No connection available".to_string())
                })?;
            detect_scheduler(connection, context)
        }
        #[cfg(feature = "slurm")]
        "slurm" => Ok(Box::new(super::scheduler_slurm::SlurmScheduler)),
        #[cfg(feature = "pbs")]
        "pbs" => Ok(Box::new(super::scheduler_pbs::PbsScheduler)),
        other => {
            // Handle cases where the feature is not compiled in
            #[cfg(not(feature = "slurm"))]
            if other == "slurm" {
                return Err(ModuleError::ExecutionFailed(
                    "Slurm support not compiled in. Rebuild with --features slurm".to_string(),
                ));
            }
            #[cfg(not(feature = "pbs"))]
            if other == "pbs" {
                return Err(ModuleError::ExecutionFailed(
                    "PBS support not compiled in. Rebuild with --features pbs".to_string(),
                ));
            }
            Err(ModuleError::InvalidParameter(format!(
                "Unknown scheduler '{}'. Must be 'slurm', 'pbs', or 'auto'",
                other
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- map_slurm_state tests --

    #[test]
    fn test_map_slurm_state_pending() {
        assert_eq!(map_slurm_state("PENDING"), JobState::Queued);
        assert_eq!(map_slurm_state("PD"), JobState::Queued);
    }

    #[test]
    fn test_map_slurm_state_running() {
        assert_eq!(map_slurm_state("RUNNING"), JobState::Running);
        assert_eq!(map_slurm_state("R"), JobState::Running);
    }

    #[test]
    fn test_map_slurm_state_completed() {
        assert_eq!(map_slurm_state("COMPLETED"), JobState::Completed);
        assert_eq!(map_slurm_state("CD"), JobState::Completed);
    }

    #[test]
    fn test_map_slurm_state_failed() {
        assert_eq!(map_slurm_state("FAILED"), JobState::Failed);
        assert_eq!(map_slurm_state("F"), JobState::Failed);
        assert_eq!(map_slurm_state("TIMEOUT"), JobState::Failed);
        assert_eq!(map_slurm_state("NODE_FAIL"), JobState::Failed);
    }

    #[test]
    fn test_map_slurm_state_cancelled() {
        assert_eq!(map_slurm_state("CANCELLED"), JobState::Cancelled);
        assert_eq!(map_slurm_state("PREEMPTED"), JobState::Cancelled);
    }

    #[test]
    fn test_map_slurm_state_held() {
        assert_eq!(map_slurm_state("HELD"), JobState::Held);
    }

    #[test]
    fn test_map_slurm_state_suspended() {
        assert_eq!(map_slurm_state("SUSPENDED"), JobState::Suspended);
    }

    #[test]
    fn test_map_slurm_state_unknown() {
        assert_eq!(
            map_slurm_state("CONFIGURING"),
            JobState::Unknown("CONFIGURING".to_string())
        );
    }

    // -- map_pbs_state tests --

    #[test]
    fn test_map_pbs_state_queued() {
        assert_eq!(map_pbs_state("Q"), JobState::Queued);
        assert_eq!(map_pbs_state("W"), JobState::Queued);
    }

    #[test]
    fn test_map_pbs_state_running() {
        assert_eq!(map_pbs_state("R"), JobState::Running);
        assert_eq!(map_pbs_state("E"), JobState::Running);
        assert_eq!(map_pbs_state("B"), JobState::Running);
    }

    #[test]
    fn test_map_pbs_state_held() {
        assert_eq!(map_pbs_state("H"), JobState::Held);
    }

    #[test]
    fn test_map_pbs_state_suspended() {
        assert_eq!(map_pbs_state("S"), JobState::Suspended);
        assert_eq!(map_pbs_state("U"), JobState::Suspended);
        assert_eq!(map_pbs_state("T"), JobState::Suspended);
    }

    #[test]
    fn test_map_pbs_state_finished() {
        assert_eq!(map_pbs_state("F"), JobState::Completed);
    }

    #[test]
    fn test_map_pbs_state_cancelled() {
        assert_eq!(map_pbs_state("X"), JobState::Cancelled);
    }

    #[test]
    fn test_map_pbs_state_unknown() {
        assert_eq!(
            map_pbs_state("Z"),
            JobState::Unknown("Z".to_string())
        );
    }

    // -- JobState Display + serde roundtrip --

    #[test]
    fn test_job_state_display() {
        assert_eq!(JobState::Queued.to_string(), "queued");
        assert_eq!(JobState::Running.to_string(), "running");
        assert_eq!(JobState::Held.to_string(), "held");
        assert_eq!(JobState::Suspended.to_string(), "suspended");
        assert_eq!(JobState::Completed.to_string(), "completed");
        assert_eq!(JobState::Failed.to_string(), "failed");
        assert_eq!(JobState::Cancelled.to_string(), "cancelled");
        assert_eq!(
            JobState::Unknown("X".to_string()).to_string(),
            "unknown(X)"
        );
    }

    #[test]
    fn test_job_state_serde_roundtrip() {
        let states = vec![
            JobState::Queued,
            JobState::Running,
            JobState::Held,
            JobState::Suspended,
            JobState::Completed,
            JobState::Failed,
            JobState::Cancelled,
            JobState::Unknown("CONFIGURING".to_string()),
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: JobState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }

    #[test]
    fn test_job_info_serde() {
        let info = JobInfo {
            id: "12345".to_string(),
            name: Some("test_job".to_string()),
            state: JobState::Running,
            queue: Some("batch".to_string()),
            owner: Some("alice".to_string()),
            nodes: Some(4),
            cpus: Some(128),
            walltime_limit: Some("24:00:00".to_string()),
            walltime_used: Some("01:30:00".to_string()),
            raw: serde_json::json!({"extra": "data"}),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: JobInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "12345");
        assert_eq!(deserialized.state, JobState::Running);
    }

    #[test]
    fn test_queue_info_serde() {
        let info = QueueInfo {
            name: "batch".to_string(),
            state: "active".to_string(),
            total_jobs: Some(42),
            raw: serde_json::json!({}),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: QueueInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "batch");
        assert_eq!(deserialized.state, "active");
    }

    #[test]
    fn test_server_info_serde() {
        let mut attrs = HashMap::new();
        attrs.insert("scheduling".to_string(), "True".to_string());
        let info = ServerInfo {
            scheduler: "pbs".to_string(),
            attributes: attrs,
            raw: serde_json::json!({}),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ServerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.scheduler, "pbs");
        assert_eq!(
            deserialized.attributes.get("scheduling"),
            Some(&"True".to_string())
        );
    }
}
