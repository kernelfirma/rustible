//! Lustre OST (Object Storage Target) management module
//!
//! Manage Lustre OST lifecycle operations via lctl.
//!
//! # Parameters
//!
//! - `ost_index` (required): OST index number
//! - `target` (required): Target device or filesystem
//! - `action` (required): Operation - "activate", "deactivate", "add", "remove"
//! - `mdt_index` (optional): MDT index for coordinated operations

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
    let mut options = ExecuteOptions::new();
    if context.r#become {
        options = options.with_escalation(context.become_user.clone());
        if let Some(ref method) = context.become_method {
            options.escalate_method = Some(method.clone());
        }
        if let Some(ref password) = context.become_password {
            options.escalate_password = Some(password.clone());
        }
    }
    options
}

fn run_cmd(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<(bool, String, String)> {
    let options = get_exec_options(context);
    let result = Handle::current()
        .block_on(async { connection.execute(cmd, Some(options)).await })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;
    Ok((result.success, result.stdout, result.stderr))
}

fn run_cmd_ok(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
    if !success {
        return Err(ModuleError::ExecutionFailed(format!(
            "Command failed: {}",
            stderr.trim()
        )));
    }
    Ok(stdout)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum OstAction {
    Activate,
    Deactivate,
    Add,
    Remove,
}

impl OstAction {
    fn from_str(s: &str) -> Option<OstAction> {
        match s.to_lowercase().as_str() {
            "activate" => Some(OstAction::Activate),
            "deactivate" => Some(OstAction::Deactivate),
            "add" => Some(OstAction::Add),
            "remove" => Some(OstAction::Remove),
            _ => None,
        }
    }

    fn to_lctl_cmd(&self) -> &'static str {
        match self {
            OstAction::Activate => "activate",
            OstAction::Deactivate => "deactivate",
            OstAction::Add => "add_osc_to_group",
            OstAction::Remove => "del_osc_from_group",
        }
    }
}

pub struct LustreOstModule;

impl Module for LustreOstModule {
    fn name(&self) -> &'static str {
        "lustre_ost"
    }

    fn description(&self) -> &'static str {
        "Manage Lustre OST lifecycle operations via lctl"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let ost_index = params.get_string_required("ost_index")?;
        let target = params.get_string_required("target")?;
        let action_str = params.get_string_required("action")?;
        let mdt_index = params.get_string("mdt_index")?;

        let action = OstAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'activate', 'deactivate', 'add', or 'remove'",
                action_str
            ))
        })?;

        // Check if lctl is available
        let (lctl_ok, _, _) = run_cmd(connection, "which lctl", context)?;
        if !lctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "lctl command not found. Ensure Lustre client utilities are installed.".to_string(),
            ));
        }

        // Query current OST state
        let ost_name = format!(
            "{}-OST{:04x}",
            target,
            ost_index.parse::<u32>().unwrap_or(0)
        );
        let (state_ok, state_output, _) = run_cmd(
            connection,
            &format!(
                "lctl get_param obdfilter.{}.state 2>/dev/null || echo 'unknown'",
                ost_name
            ),
            context,
        )?;

        let current_state = if state_ok {
            if state_output.contains("active") {
                "active"
            } else if state_output.contains("inactive") {
                "inactive"
            } else {
                "unknown"
            }
        } else {
            "unknown"
        };

        // Idempotency check
        let needs_change = match action {
            OstAction::Activate => current_state != "active",
            OstAction::Deactivate => current_state != "inactive",
            OstAction::Add | OstAction::Remove => true, // Always execute for add/remove
        };

        if !needs_change {
            return Ok(
                ModuleOutput::ok(format!("OST {} is already in desired state", ost_name))
                    .with_data("ost", serde_json::json!(ost_name))
                    .with_data("state", serde_json::json!(current_state)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute {} on OST {} (current state: {})",
                action_str, ost_name, current_state
            ))
            .with_data("ost", serde_json::json!(ost_name))
            .with_data("action", serde_json::json!(action)));
        }

        // Build lctl command
        let lctl_cmd = match action {
            OstAction::Activate | OstAction::Deactivate => {
                format!(
                    "lctl set_param obdfilter.{}.state={}",
                    ost_name,
                    action.to_lctl_cmd()
                )
            }
            OstAction::Add | OstAction::Remove => {
                let mdt_suffix = if let Some(ref mdt) = mdt_index {
                    format!(" --mdt-index {}", mdt)
                } else {
                    String::new()
                };
                format!(
                    "lctl {} {} {}{}",
                    action.to_lctl_cmd(),
                    target,
                    ost_index,
                    mdt_suffix
                )
            }
        };

        run_cmd_ok(connection, &lctl_cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Executed {} on OST {} (was {})",
            action_str, ost_name, current_state
        ))
        .with_data("ost", serde_json::json!(ost_name))
        .with_data("action", serde_json::json!(action))
        .with_data("previous_state", serde_json::json!(current_state)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["ost_index", "target", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("mdt_index", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = LustreOstModule;
        assert_eq!(module.name(), "lustre_ost");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = LustreOstModule;
        let required = module.required_params();
        assert!(required.contains(&"ost_index"));
        assert!(required.contains(&"target"));
        assert!(required.contains(&"action"));
    }

    #[test]
    fn test_optional_params() {
        let module = LustreOstModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("mdt_index"));
    }

    #[test]
    fn test_ost_action_from_str() {
        assert_eq!(OstAction::from_str("activate"), Some(OstAction::Activate));
        assert_eq!(
            OstAction::from_str("DEACTIVATE"),
            Some(OstAction::Deactivate)
        );
        assert_eq!(OstAction::from_str("add"), Some(OstAction::Add));
        assert_eq!(OstAction::from_str("remove"), Some(OstAction::Remove));
        assert_eq!(OstAction::from_str("invalid"), None);
    }

    #[test]
    fn test_ost_action_to_lctl_cmd() {
        assert_eq!(OstAction::Activate.to_lctl_cmd(), "activate");
        assert_eq!(OstAction::Deactivate.to_lctl_cmd(), "deactivate");
        assert_eq!(OstAction::Add.to_lctl_cmd(), "add_osc_to_group");
        assert_eq!(OstAction::Remove.to_lctl_cmd(), "del_osc_from_group");
    }
}
