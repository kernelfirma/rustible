//! OFED / RDMA / InfiniBand stack module
//!
//! Manages RDMA userland packages and kernel module configuration.

use crate::modules::{
    Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult,
};

pub struct RdmaStackModule;

impl Module for RdmaStackModule {
    fn name(&self) -> &'static str {
        "rdma_stack"
    }

    fn description(&self) -> &'static str {
        "Manage RDMA / InfiniBand / OFED userland stack"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would configure RDMA stack"));
        }

        Ok(ModuleOutput::ok("RDMA stack configuration: stub - not yet implemented")
            .with_data("status", serde_json::json!("stub")))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }
}
