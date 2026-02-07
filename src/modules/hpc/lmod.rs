//! Lmod / Environment Modules support
//!
//! Manages Lmod installation and module path configuration.

use crate::modules::{
    Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult,
};

pub struct LmodModule;

impl Module for LmodModule {
    fn name(&self) -> &'static str {
        "lmod"
    }

    fn description(&self) -> &'static str {
        "Manage Lmod / Environment Modules installation and configuration"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would configure Lmod"));
        }

        Ok(ModuleOutput::ok("Lmod configuration: stub - not yet implemented")
            .with_data("status", serde_json::json!("stub")))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}
