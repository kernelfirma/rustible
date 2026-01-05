//! Fail module - Fail playbook execution with a custom message
//!
//! This module is used to intentionally fail playbook execution with a
//! custom error message. It is useful for enforcing preconditions, validation,
//! and error handling in playbooks.
//!
//! Unlike most modules, it runs entirely on the control node and does not
//! require an SSH connection.
//!
//! # Example
//!
//! ```yaml
//! - name: Fail if variable is not defined
//!   fail:
//!     msg: "The required variable 'db_password' is not defined"
//!   when: db_password is not defined
//!
//! - name: Fail with default message
//!   fail:
//!   when: some_condition
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Default failure message when no custom message is provided
const DEFAULT_FAIL_MSG: &str = "Failed as requested from task";

/// Module for intentionally failing playbook execution
pub struct FailModule;

impl Module for FailModule {
    fn name(&self) -> &'static str {
        "fail"
    }

    fn description(&self) -> &'static str {
        "Fail playbook execution with a custom message"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs entirely on the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel since it's just setting a failure status
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // No required parameters - msg is optional
        &[]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Get the custom message or use the default
        let message = params
            .get_string("msg")?
            .unwrap_or_else(|| DEFAULT_FAIL_MSG.to_string());

        // Return a failed output with the message
        Ok(ModuleOutput::failed(message))
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // Fail module behaves the same in check mode - it still fails
        self.execute(params, context)
    }

    fn diff(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<Option<super::Diff>> {
        // Fail module never produces diffs
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ModuleStatus;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_fail_with_custom_message() {
        let module = FailModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "msg".to_string(),
            Value::String("Custom failure message".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, ModuleStatus::Failed);
        assert_eq!(result.msg, "Custom failure message");
    }

    #[test]
    fn test_fail_with_default_message() {
        let module = FailModule;
        let params: ModuleParams = HashMap::new();

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, ModuleStatus::Failed);
        assert_eq!(result.msg, DEFAULT_FAIL_MSG);
    }

    #[test]
    fn test_fail_check_mode_still_fails() {
        let module = FailModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), Value::String("Check mode fail".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert_eq!(result.status, ModuleStatus::Failed);
        assert_eq!(result.msg, "Check mode fail");
    }

    #[test]
    fn test_fail_classification() {
        let module = FailModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_fail_no_required_params() {
        let module = FailModule;
        assert!(module.required_params().is_empty());
    }
}
