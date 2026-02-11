//! HPC common baseline module
//!
//! Validates and reports HPC cluster baseline configuration including
//! system limits, sysctl parameters, required directories, and time sync.

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleOutput, ModuleParams, ModuleResult,
};

pub struct HpcBaselineModule;

impl Module for HpcBaselineModule {
    fn name(&self) -> &'static str {
        "hpc_baseline"
    }

    fn description(&self) -> &'static str {
        "Validate and report HPC cluster baseline configuration"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok(
                "Would validate HPC baseline configuration",
            ));
        }

        Ok(
            ModuleOutput::ok("HPC baseline validation: stub - not yet implemented")
                .with_data("status", serde_json::json!("stub"))
                .with_data(
                    "supported_distros",
                    serde_json::json!(["rocky-9", "alma-9", "ubuntu-22.04"]),
                ),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }
}
