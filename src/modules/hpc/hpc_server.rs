//! Unified HPC server/cluster configuration module
//!
//! Scheduler-agnostic server query and configuration that works with Slurm or PBS Pro.
//!
//! # Parameters
//!
//! - `action` (required): `"query"` or `"set_attributes"`
//! - `scheduler` (optional, default `"auto"`): Which scheduler backend to use
//! - `attributes` (optional): JSON object of server attributes to set

use std::collections::HashMap;

use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
};

use super::scheduler::resolve_scheduler;

pub struct HpcServerModule;

impl Module for HpcServerModule {
    fn name(&self) -> &'static str {
        "hpc_server"
    }

    fn description(&self) -> &'static str {
        "Unified HPC server configuration (query, set_attributes) for Slurm and PBS Pro"
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
            "query" => {
                let info = scheduler.query_server(context)?;
                ModuleOutput::ok(format!(
                    "Retrieved {} server attribute(s) from {}",
                    info.attributes.len(),
                    info.scheduler
                ))
                .with_data("server", serde_json::to_value(&info).unwrap_or_default())
            }
            "set_attributes" => scheduler.set_server_attributes(params, context)?,
            _ => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid action '{}'. Must be 'query' or 'set_attributes'",
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
        m.insert("attributes", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Module;

    #[test]
    fn test_hpc_server_module_name() {
        let m = HpcServerModule;
        assert_eq!(m.name(), "hpc_server");
    }

    #[test]
    fn test_hpc_server_required_params() {
        let m = HpcServerModule;
        assert_eq!(m.required_params(), &["action"]);
    }

    #[test]
    fn test_hpc_server_parallelization_hint() {
        let m = HpcServerModule;
        assert_eq!(
            m.parallelization_hint(),
            ParallelizationHint::GlobalExclusive
        );
    }
}
