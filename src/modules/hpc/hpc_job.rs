//! Unified HPC job management module
//!
//! Scheduler-agnostic job operations that work with Slurm or PBS Pro.
//! The scheduler is selected via the `scheduler` parameter (`"slurm"`,
//! `"pbs"`, or `"auto"` — the default).
//!
//! # Parameters
//!
//! - `action` (required): `"submit"`, `"cancel"`, `"status"`, `"hold"`, or `"release"`
//! - `scheduler` (optional, default `"auto"`): Which scheduler backend to use
//! - `script` (optional): Inline job script content (for submit)
//! - `script_path` (optional): Path to job script file (for submit)
//! - `job_name` (optional): Job name (used for idempotency on submit)
//! - `queue` (optional): Target queue / partition
//! - `nodes` (optional): Number of nodes
//! - `cpus` (optional): Number of CPUs / tasks
//! - `walltime` (optional): Wall time limit
//! - `output_path` (optional): stdout file path
//! - `error_path` (optional): stderr file path
//! - `extra_args` (optional): Additional scheduler-specific arguments
//! - `job_id` (required for cancel/status/hold/release): Job ID to operate on

use std::collections::HashMap;

use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
};

use super::scheduler::resolve_scheduler;

pub struct HpcJobModule;

impl Module for HpcJobModule {
    fn name(&self) -> &'static str {
        "hpc_job"
    }

    fn description(&self) -> &'static str {
        "Unified HPC job management (submit, cancel, status, hold, release) for Slurm and PBS Pro"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let action = params.get_string_required("action")?;
        let scheduler = resolve_scheduler(params, context)?;

        let mut output = match action.as_str() {
            "submit" => scheduler.submit_job(params, context)?,
            "cancel" => {
                let job_id = params.get_string_required("job_id")?;
                scheduler.cancel_job(&job_id, context)?
            }
            "status" => {
                let job_id = params.get_string_required("job_id")?;
                let info = scheduler.job_status(&job_id, context)?;
                ModuleOutput::ok(format!(
                    "Job {} is {}",
                    info.id,
                    info.state
                ))
                .with_data("job", serde_json::to_value(&info).unwrap_or_default())
            }
            "hold" => {
                let job_id = params.get_string_required("job_id")?;
                scheduler.hold_job(&job_id, context)?
            }
            "release" => {
                let job_id = params.get_string_required("job_id")?;
                scheduler.release_job(&job_id, context)?
            }
            _ => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid action '{}'. Must be 'submit', 'cancel', 'status', 'hold', or 'release'",
                    action
                )));
            }
        };

        // Tag the output with the scheduler name
        output.data.insert(
            "scheduler".to_string(),
            serde_json::json!(scheduler.scheduler_name()),
        );

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("scheduler", serde_json::json!("auto"));
        m.insert("script", serde_json::json!(null));
        m.insert("script_path", serde_json::json!(null));
        m.insert("job_name", serde_json::json!(null));
        m.insert("queue", serde_json::json!(null));
        m.insert("nodes", serde_json::json!(null));
        m.insert("cpus", serde_json::json!(null));
        m.insert("walltime", serde_json::json!(null));
        m.insert("output_path", serde_json::json!(null));
        m.insert("error_path", serde_json::json!(null));
        m.insert("extra_args", serde_json::json!(null));
        m.insert("job_id", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Module;

    #[test]
    fn test_hpc_job_module_name() {
        let m = HpcJobModule;
        assert_eq!(m.name(), "hpc_job");
    }

    #[test]
    fn test_hpc_job_description() {
        let m = HpcJobModule;
        assert!(m.description().contains("Unified"));
        assert!(m.description().contains("Slurm"));
        assert!(m.description().contains("PBS"));
    }

    #[test]
    fn test_hpc_job_required_params() {
        let m = HpcJobModule;
        assert_eq!(m.required_params(), &["action"]);
    }

    #[test]
    fn test_hpc_job_optional_params() {
        let m = HpcJobModule;
        let opts = m.optional_params();
        assert!(opts.contains_key("scheduler"));
        assert!(opts.contains_key("script"));
        assert!(opts.contains_key("script_path"));
        assert!(opts.contains_key("job_name"));
        assert!(opts.contains_key("queue"));
        assert!(opts.contains_key("nodes"));
        assert!(opts.contains_key("cpus"));
        assert!(opts.contains_key("walltime"));
        assert!(opts.contains_key("job_id"));
        assert_eq!(opts["scheduler"], serde_json::json!("auto"));
    }

    #[test]
    fn test_hpc_job_parallelization_hint() {
        let m = HpcJobModule;
        assert_eq!(m.parallelization_hint(), ParallelizationHint::FullyParallel);
    }
}
