//! Slurm workload manager modules
//!
//! Provides configuration and operations modules for Slurm:
//! - `slurm_config`: Manage slurm.conf, cgroup.conf, gres.conf
//! - `slurm_ops`: Cluster operations (reconfigure, drain, resume)

use crate::modules::{
    Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};

pub struct SlurmConfigModule;

impl Module for SlurmConfigModule {
    fn name(&self) -> &'static str {
        "slurm_config"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm configuration files (slurm.conf, cgroup.conf, gres.conf)"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would configure Slurm"));
        }

        Ok(ModuleOutput::ok("Slurm configuration: stub - not yet implemented")
            .with_data("status", serde_json::json!("stub")))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}

pub struct SlurmOpsModule;

impl Module for SlurmOpsModule {
    fn name(&self) -> &'static str {
        "slurm_ops"
    }

    fn description(&self) -> &'static str {
        "Slurm cluster operations (reconfigure, drain/resume nodes)"
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let action = params.get_string("action")?.unwrap_or_default();

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!("Would perform Slurm action: {}", action)));
        }

        Ok(ModuleOutput::ok(format!("Slurm ops '{}': stub - not yet implemented", action))
            .with_data("status", serde_json::json!("stub"))
            .with_data("action", serde_json::json!(action)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }
}
