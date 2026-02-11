//! Parallel filesystem client modules
//!
//! Manages Lustre and BeeGFS client installation and mount configuration.

use crate::modules::{Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult};

pub struct LustreClientModule;

impl Module for LustreClientModule {
    fn name(&self) -> &'static str {
        "lustre_client"
    }

    fn description(&self) -> &'static str {
        "Manage Lustre filesystem client installation and mounts"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would configure Lustre client"));
        }

        Ok(
            ModuleOutput::ok("Lustre client: stub - not yet implemented")
                .with_data("status", serde_json::json!("stub")),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}

pub struct BeegfsClientModule;

impl Module for BeegfsClientModule {
    fn name(&self) -> &'static str {
        "beegfs_client"
    }

    fn description(&self) -> &'static str {
        "Manage BeeGFS filesystem client installation and mounts"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would configure BeeGFS client"));
        }

        Ok(
            ModuleOutput::ok("BeeGFS client: stub - not yet implemented")
                .with_data("status", serde_json::json!("stub")),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}
