//! Unified HPC queue management module
//!
//! Scheduler-agnostic queue/partition operations that work with Slurm or PBS Pro.
//!
//! # Parameters
//!
//! - `action` (required): `"list"`, `"create"`, or `"delete"`
//! - `name` (required for create/delete): Queue / partition name
//! - `scheduler` (optional, default `"auto"`): Which scheduler backend to use
//! - `queue_type` (optional): Queue type (PBS: `"execution"` / `"route"`)
//! - `enabled` (optional): Whether the queue accepts jobs
//! - `started` (optional): Whether the queue routes/runs jobs
//! - `attributes` (optional): JSON object of additional attributes

use std::collections::HashMap;

use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
};

use super::scheduler::resolve_scheduler;

pub struct HpcQueueModule;

impl Module for HpcQueueModule {
    fn name(&self) -> &'static str {
        "hpc_queue"
    }

    fn description(&self) -> &'static str {
        "Unified HPC queue management (list, create, delete) for Slurm and PBS Pro"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::GlobalExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let action = params.get_string_required("action")?;
        let scheduler = resolve_scheduler(params, context)?;

        let mut output = match action.as_str() {
            "list" => {
                let queues = scheduler.list_queues(context)?;
                ModuleOutput::ok(format!("Listed {} queue(s)", queues.len()))
                    .with_data("queues", serde_json::to_value(&queues).unwrap_or_default())
                    .with_data("count", serde_json::json!(queues.len()))
            }
            "create" => {
                let name = params.get_string_required("name")?;
                scheduler.create_queue(&name, params, context)?
            }
            "delete" => {
                let name = params.get_string_required("name")?;
                scheduler.delete_queue(&name, context)?
            }
            _ => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid action '{}'. Must be 'list', 'create', or 'delete'",
                    action
                )));
            }
        };

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
        m.insert("name", serde_json::json!(null));
        m.insert("queue_type", serde_json::json!(null));
        m.insert("enabled", serde_json::json!(null));
        m.insert("started", serde_json::json!(null));
        m.insert("max_run", serde_json::json!(null));
        m.insert("max_queued", serde_json::json!(null));
        m.insert("attributes", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Module;

    #[test]
    fn test_hpc_queue_module_name() {
        let m = HpcQueueModule;
        assert_eq!(m.name(), "hpc_queue");
    }

    #[test]
    fn test_hpc_queue_required_params() {
        let m = HpcQueueModule;
        assert_eq!(m.required_params(), &["action"]);
    }

    #[test]
    fn test_hpc_queue_parallelization_hint() {
        let m = HpcQueueModule;
        assert_eq!(
            m.parallelization_hint(),
            ParallelizationHint::GlobalExclusive
        );
    }
}
