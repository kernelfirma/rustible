//! Ping module - Test connectivity to a target host
//!
//! This module is a trivial test module that validates the connection to a
//! target host. It returns "pong" on success. It does not perform ICMP pings;
//! it simply verifies that the host is reachable and the module system is working.
//!
//! # Parameters
//!
//! - `data`: Data to return instead of "pong" (optional, default: "pong")
//!
//! # Example
//!
//! ```yaml
//! - name: Test connectivity
//!   ping:
//!
//! - name: Test with custom data
//!   ping:
//!     data: "alive"
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Module for testing connectivity to a target host
pub struct PingModule;

impl Module for PingModule {
    fn name(&self) -> &'static str {
        "ping"
    }

    fn description(&self) -> &'static str {
        "Test connectivity to a target host"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let data = params
            .get_string("data")?
            .unwrap_or_else(|| "pong".to_string());

        Ok(ModuleOutput::ok("ping: pong").with_data("ping", serde_json::json!(data)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_ping_default() {
        let module = PingModule;
        let params: ModuleParams = HashMap::new();
        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.msg, "ping: pong");
        assert_eq!(result.data["ping"], "pong");
        assert!(!result.changed);
    }

    #[test]
    fn test_ping_custom_data() {
        let module = PingModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("data".to_string(), serde_json::json!("alive"));
        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.data["ping"], "alive");
    }

    #[test]
    fn test_ping_metadata() {
        let module = PingModule;
        assert_eq!(module.name(), "ping");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert!(module.required_params().is_empty());
    }
}
