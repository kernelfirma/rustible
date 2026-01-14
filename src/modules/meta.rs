//! Meta module - Execute Ansible meta actions
//!
//! This module is used for meta-level operations that control playbook execution
//! rather than modifying target hosts. It supports various actions like flushing
//! handlers, ending play execution, and clearing facts.
//!
//! Unlike most modules, meta actions run entirely on the control node and affect
//! the playbook execution flow rather than target system state.
//!
//! # Supported Actions
//!
//! - `flush_handlers` - Run pending handlers immediately
//! - `end_host` - End play for the current host (continue on other hosts)
//! - `end_play` - End the current play entirely
//! - `end_batch` - End the current batch of hosts
//! - `clear_facts` - Clear all gathered facts for the current host
//! - `clear_host_errors` - Clear failure status for the current host
//! - `refresh_inventory` - Refresh the inventory from sources
//! - `reset_connection` - Reset/reconnect the SSH/WinRM connection
//! - `noop` - No operation (placeholder)
//!
//! # Example
//!
//! ```yaml
//! - name: Flush handlers now
//!   meta: flush_handlers
//!
//! - name: End play for this host
//!   meta: end_host
//!   when: skip_condition
//!
//! - name: Clear gathered facts
//!   meta: clear_facts
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};

/// Supported meta actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaAction {
    /// Run pending handlers immediately
    FlushHandlers,
    /// End play for the current host (continue with other hosts)
    EndHost,
    /// End the current play entirely
    EndPlay,
    /// End the current batch of hosts
    EndBatch,
    /// Clear all gathered facts for the current host
    ClearFacts,
    /// Clear failure status for the current host
    ClearHostErrors,
    /// Refresh the inventory from sources
    RefreshInventory,
    /// Reset/reconnect the connection
    ResetConnection,
    /// No operation (placeholder action)
    Noop,
}

impl MetaAction {
    /// Parse a string into a MetaAction
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "flush_handlers" => Some(MetaAction::FlushHandlers),
            "end_host" => Some(MetaAction::EndHost),
            "end_play" => Some(MetaAction::EndPlay),
            "end_batch" => Some(MetaAction::EndBatch),
            "clear_facts" => Some(MetaAction::ClearFacts),
            "clear_host_errors" => Some(MetaAction::ClearHostErrors),
            "refresh_inventory" => Some(MetaAction::RefreshInventory),
            "reset_connection" => Some(MetaAction::ResetConnection),
            "noop" => Some(MetaAction::Noop),
            _ => None,
        }
    }

    /// Get the action name
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaAction::FlushHandlers => "flush_handlers",
            MetaAction::EndHost => "end_host",
            MetaAction::EndPlay => "end_play",
            MetaAction::EndBatch => "end_batch",
            MetaAction::ClearFacts => "clear_facts",
            MetaAction::ClearHostErrors => "clear_host_errors",
            MetaAction::RefreshInventory => "refresh_inventory",
            MetaAction::ResetConnection => "reset_connection",
            MetaAction::Noop => "noop",
        }
    }

    /// Check if this action ends execution in some way
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            MetaAction::EndHost | MetaAction::EndPlay | MetaAction::EndBatch
        )
    }
}

impl std::fmt::Display for MetaAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Module for executing meta actions
pub struct MetaModule;

impl MetaModule {
    /// Get the action from parameters
    fn get_action(&self, params: &ModuleParams) -> ModuleResult<MetaAction> {
        // The meta module in Ansible can be specified as either:
        // - meta: action_name (direct value)
        // - meta:
        //     action: action_name (free_form parameter)

        // Try "meta" key first (for free_form style)
        if let Some(action_str) = params.get_string("meta")? {
            return MetaAction::from_str(&action_str).ok_or_else(|| {
                ModuleError::InvalidParameter(format!(
                    "Unknown meta action: '{}'. Valid actions are: flush_handlers, end_host, \
                     end_play, end_batch, clear_facts, clear_host_errors, refresh_inventory, \
                     reset_connection, noop",
                    action_str
                ))
            });
        }

        // Try "action" key (alternative specification)
        if let Some(action_str) = params.get_string("action")? {
            return MetaAction::from_str(&action_str).ok_or_else(|| {
                ModuleError::InvalidParameter(format!(
                    "Unknown meta action: '{}'. Valid actions are: flush_handlers, end_host, \
                     end_play, end_batch, clear_facts, clear_host_errors, refresh_inventory, \
                     reset_connection, noop",
                    action_str
                ))
            });
        }

        // Try "free_form" key (another Ansible convention)
        if let Some(action_str) = params.get_string("free_form")? {
            return MetaAction::from_str(&action_str).ok_or_else(|| {
                ModuleError::InvalidParameter(format!(
                    "Unknown meta action: '{}'. Valid actions are: flush_handlers, end_host, \
                     end_play, end_batch, clear_facts, clear_host_errors, refresh_inventory, \
                     reset_connection, noop",
                    action_str
                ))
            });
        }

        Err(ModuleError::MissingParameter(
            "meta action is required. Use one of: flush_handlers, end_host, end_play, \
             end_batch, clear_facts, clear_host_errors, refresh_inventory, reset_connection, noop"
                .to_string(),
        ))
    }

    /// Execute the meta action
    fn execute_action(&self, action: MetaAction, context: &ModuleContext) -> ModuleOutput {
        match action {
            MetaAction::FlushHandlers => {
                // Signal to the executor to run pending handlers
                ModuleOutput::ok("Handlers flushed")
                    .with_data("meta_action", serde_json::json!("flush_handlers"))
                    .with_data("flush_handlers", serde_json::json!(true))
            }
            MetaAction::EndHost => {
                // Signal to end play for this host
                ModuleOutput::ok("Ending play for this host")
                    .with_data("meta_action", serde_json::json!("end_host"))
                    .with_data("end_host", serde_json::json!(true))
            }
            MetaAction::EndPlay => {
                // Signal to end the entire play
                ModuleOutput::ok("Ending play")
                    .with_data("meta_action", serde_json::json!("end_play"))
                    .with_data("end_play", serde_json::json!(true))
            }
            MetaAction::EndBatch => {
                // Signal to end the current batch
                ModuleOutput::ok("Ending batch")
                    .with_data("meta_action", serde_json::json!("end_batch"))
                    .with_data("end_batch", serde_json::json!(true))
            }
            MetaAction::ClearFacts => {
                // Signal to clear gathered facts
                ModuleOutput::changed("Facts cleared")
                    .with_data("meta_action", serde_json::json!("clear_facts"))
                    .with_data("clear_facts", serde_json::json!(true))
            }
            MetaAction::ClearHostErrors => {
                // Signal to clear host failure status
                ModuleOutput::ok("Host errors cleared")
                    .with_data("meta_action", serde_json::json!("clear_host_errors"))
                    .with_data("clear_host_errors", serde_json::json!(true))
            }
            MetaAction::RefreshInventory => {
                // Signal to refresh inventory
                ModuleOutput::ok("Inventory refresh requested")
                    .with_data("meta_action", serde_json::json!("refresh_inventory"))
                    .with_data("refresh_inventory", serde_json::json!(true))
            }
            MetaAction::ResetConnection => {
                // Signal to reset the connection
                ModuleOutput::ok("Connection reset requested")
                    .with_data("meta_action", serde_json::json!("reset_connection"))
                    .with_data("reset_connection", serde_json::json!(true))
            }
            MetaAction::Noop => {
                // Do nothing
                ModuleOutput::ok("No operation").with_data("meta_action", serde_json::json!("noop"))
            }
        }
    }
}

impl Module for MetaModule {
    fn name(&self) -> &'static str {
        "meta"
    }

    fn description(&self) -> &'static str {
        "Execute Ansible meta actions for playbook flow control"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because meta actions run on the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Some meta actions (like end_play) affect global state
        // but we handle this at the executor level
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // Action is required but can come from different keys
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate that we can parse the action
        self.get_action(params)?;
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let action = self.get_action(params)?;
        Ok(self.execute_action(action, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ModuleStatus;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_meta_flush_handlers() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "meta".to_string(),
            Value::String("flush_handlers".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, ModuleStatus::Ok);
        assert_eq!(result.data.get("flush_handlers"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_meta_end_host() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("meta".to_string(), Value::String("end_host".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.data.get("end_host"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_meta_end_play() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("meta".to_string(), Value::String("end_play".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.data.get("end_play"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_meta_clear_facts() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("meta".to_string(), Value::String("clear_facts".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert_eq!(result.status, ModuleStatus::Changed);
        assert_eq!(result.data.get("clear_facts"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_meta_noop() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("meta".to_string(), Value::String("noop".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, ModuleStatus::Ok);
    }

    #[test]
    fn test_meta_invalid_action() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "meta".to_string(),
            Value::String("invalid_action".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown meta action"));
    }

    #[test]
    fn test_meta_missing_action() {
        let module = MetaModule;
        let params: ModuleParams = HashMap::new();

        let result = module.validate_params(&params);

        assert!(result.is_err());
    }

    #[test]
    fn test_meta_action_via_action_key() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "action".to_string(),
            Value::String("flush_handlers".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.data.get("flush_handlers"), Some(&Value::Bool(true)));
    }

    #[test]
    fn test_meta_action_is_terminal() {
        assert!(MetaAction::EndHost.is_terminal());
        assert!(MetaAction::EndPlay.is_terminal());
        assert!(MetaAction::EndBatch.is_terminal());
        assert!(!MetaAction::FlushHandlers.is_terminal());
        assert!(!MetaAction::Noop.is_terminal());
    }

    #[test]
    fn test_meta_classification() {
        let module = MetaModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_meta_check_mode_clear_facts() {
        let module = MetaModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("meta".to_string(), Value::String("clear_facts".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        // In check mode, clear_facts should report what would happen
        assert!(result.msg.contains("check mode") || result.msg.contains("clear"));
    }
}
