//! MPI configuration module
//!
//! Manages MPI library installation and configuration (OpenMPI, Intel MPI).

use crate::modules::{Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult, ParamExt};

pub struct MpiModule;

impl Module for MpiModule {
    fn name(&self) -> &'static str {
        "mpi_config"
    }

    fn description(&self) -> &'static str {
        "Configure MPI libraries (OpenMPI, Intel MPI)"
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let flavor = params
            .get_string("flavor")?
            .unwrap_or_else(|| "openmpi".to_string());

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would configure MPI ({})",
                flavor
            )));
        }

        Ok(ModuleOutput::ok(format!(
            "MPI config ({}): stub - not yet implemented",
            flavor
        ))
        .with_data("status", serde_json::json!("stub"))
        .with_data("flavor", serde_json::json!(flavor)))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}
