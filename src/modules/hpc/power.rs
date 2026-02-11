//! HPC bare-metal power management module
//!
//! Manages server power state via IPMI (ipmitool) or Redfish REST API.
//! Supports power on, off, reset, cycle, and status queries.
//!
//! # Parameters
//!
//! - `action` (required): Power action - "on", "off", "reset", "cycle", "status"
//! - `host` (required): BMC/IPMI host address
//! - `user` (optional): IPMI/Redfish username (default: "admin")
//! - `password` (optional): IPMI/Redfish password
//! - `interface` (optional): IPMI interface - "lanplus" (default), "lan", "open"
//! - `provider` (optional): Power provider - "ipmi" (default), "redfish"

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
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

/// Power actions supported by the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerAction {
    On,
    Off,
    Reset,
    Cycle,
    Status,
}

impl PowerAction {
    /// Parse a string into a PowerAction.
    pub fn from_str(s: &str) -> Option<PowerAction> {
        match s.to_lowercase().as_str() {
            "on" => Some(PowerAction::On),
            "off" => Some(PowerAction::Off),
            "reset" => Some(PowerAction::Reset),
            "cycle" => Some(PowerAction::Cycle),
            "status" => Some(PowerAction::Status),
            _ => None,
        }
    }

    /// Convert to the ipmitool chassis power subcommand.
    pub fn to_ipmi_cmd(&self) -> &'static str {
        match self {
            PowerAction::On => "on",
            PowerAction::Off => "off",
            PowerAction::Reset => "reset",
            PowerAction::Cycle => "cycle",
            PowerAction::Status => "status",
        }
    }

    /// Convert to the Redfish reset type value.
    pub fn to_redfish_reset_type(&self) -> Option<&'static str> {
        match self {
            PowerAction::On => Some("On"),
            PowerAction::Off => Some("ForceOff"),
            PowerAction::Reset => Some("ForceRestart"),
            PowerAction::Cycle => Some("PowerCycle"),
            PowerAction::Status => None,
        }
    }
}

/// Current power state of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    On,
    Off,
    Unknown,
}

impl PowerState {
    /// Parse power state from ipmitool output.
    pub fn from_ipmi_output(output: &str) -> PowerState {
        let lower = output.to_lowercase();
        if lower.contains("chassis power is on") {
            PowerState::On
        } else if lower.contains("chassis power is off") {
            PowerState::Off
        } else {
            PowerState::Unknown
        }
    }

    /// Parse power state from a Redfish PowerState field value.
    pub fn from_redfish_value(value: &str) -> PowerState {
        match value.to_lowercase().as_str() {
            "on" => PowerState::On,
            "off" => PowerState::Off,
            "poweredoff" | "powered off" => PowerState::Off,
            "poweredon" | "powered on" => PowerState::On,
            _ => PowerState::Unknown,
        }
    }
}

/// Provider for power management operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerProvider {
    Ipmi,
    Redfish,
}

impl PowerProvider {
    pub fn from_str(s: &str) -> Option<PowerProvider> {
        match s.to_lowercase().as_str() {
            "ipmi" => Some(PowerProvider::Ipmi),
            "redfish" => Some(PowerProvider::Redfish),
            _ => None,
        }
    }
}

pub struct HpcPowerModule;

impl HpcPowerModule {
    /// Build the ipmitool command for a given action.
    fn build_ipmi_command(
        host: &str,
        user: &str,
        password: &str,
        interface: &str,
        action: PowerAction,
    ) -> String {
        format!(
            "ipmitool -I {} -H {} -U {} -P '{}' chassis power {}",
            interface,
            host,
            user,
            password.replace('\'', "'\\''"),
            action.to_ipmi_cmd(),
        )
    }

    /// Build the curl command for a Redfish power status query.
    fn build_redfish_status_command(host: &str, user: &str, password: &str) -> String {
        format!(
            "curl -sk -u '{}:{}' https://{}/redfish/v1/Systems/1 2>/dev/null",
            user.replace('\'', "'\\''"),
            password.replace('\'', "'\\''"),
            host,
        )
    }

    /// Build the curl command for a Redfish reset action.
    fn build_redfish_reset_command(
        host: &str,
        user: &str,
        password: &str,
        reset_type: &str,
    ) -> String {
        format!(
            "curl -sk -u '{}:{}' -X POST \
             -H 'Content-Type: application/json' \
             -d '{{\"ResetType\": \"{}\"}}' \
             https://{}/redfish/v1/Systems/1/Actions/ComputerSystem.Reset 2>/dev/null",
            user.replace('\'', "'\\''"),
            password.replace('\'', "'\\''"),
            reset_type,
            host,
        )
    }

    /// Determine whether an action would change state given the current power state.
    fn action_would_change(action: PowerAction, current: PowerState) -> bool {
        match action {
            PowerAction::On => current != PowerState::On,
            PowerAction::Off => current != PowerState::Off,
            PowerAction::Reset | PowerAction::Cycle => true,
            PowerAction::Status => false,
        }
    }
}

impl Module for HpcPowerModule {
    fn name(&self) -> &'static str {
        "hpc_power"
    }

    fn description(&self) -> &'static str {
        "Manage bare-metal server power state via IPMI or Redfish"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
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

        let action_str = params.get_string_required("action")?;
        let action = PowerAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid power action '{}'. Must be 'on', 'off', 'reset', 'cycle', or 'status'",
                action_str
            ))
        })?;

        let host = params.get_string_required("host")?;
        let user = params
            .get_string("user")?
            .unwrap_or_else(|| "admin".to_string());
        let password = params
            .get_string("password")?
            .unwrap_or_default();
        let interface = params
            .get_string("interface")?
            .unwrap_or_else(|| "lanplus".to_string());
        let provider_str = params
            .get_string("provider")?
            .unwrap_or_else(|| "ipmi".to_string());
        let provider = PowerProvider::from_str(&provider_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid provider '{}'. Must be 'ipmi' or 'redfish'",
                provider_str
            ))
        })?;

        match provider {
            PowerProvider::Ipmi => {
                self.execute_ipmi(connection, context, action, &host, &user, &password, &interface)
            }
            PowerProvider::Redfish => {
                self.execute_redfish(connection, context, action, &host, &user, &password)
            }
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action", "host"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("interface", serde_json::json!("lanplus"));
        m.insert("provider", serde_json::json!("ipmi"));
        m
    }
}

impl HpcPowerModule {
    fn execute_ipmi(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        action: PowerAction,
        host: &str,
        user: &str,
        password: &str,
        interface: &str,
    ) -> ModuleResult<ModuleOutput> {
        // Always query current state first for idempotency
        let status_cmd =
            Self::build_ipmi_command(host, user, password, interface, PowerAction::Status);
        let (ok, stdout, _stderr) = run_cmd(connection, &status_cmd, context)?;
        let current_state = if ok {
            PowerState::from_ipmi_output(&stdout)
        } else {
            PowerState::Unknown
        };

        // For status action, just return current state
        if action == PowerAction::Status {
            return Ok(ModuleOutput::ok(format!(
                "Power state: {:?}",
                current_state
            ))
            .with_data("power_state", serde_json::json!(current_state))
            .with_data("host", serde_json::json!(host))
            .with_data("provider", serde_json::json!("ipmi")));
        }

        // Check idempotency
        if !Self::action_would_change(action, current_state) {
            return Ok(ModuleOutput::ok(format!(
                "Server is already {:?}, no action needed",
                current_state
            ))
            .with_data("power_state", serde_json::json!(current_state))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute power {} on {} (current state: {:?})",
                action.to_ipmi_cmd(),
                host,
                current_state
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action))
            .with_data("current_state", serde_json::json!(current_state)));
        }

        let cmd = Self::build_ipmi_command(host, user, password, interface, action);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Power {} executed on {} (was {:?})",
            action.to_ipmi_cmd(),
            host,
            current_state,
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("previous_state", serde_json::json!(current_state))
        .with_data("provider", serde_json::json!("ipmi")))
    }

    fn execute_redfish(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        action: PowerAction,
        host: &str,
        user: &str,
        password: &str,
    ) -> ModuleResult<ModuleOutput> {
        // Query current state via Redfish
        let status_cmd = Self::build_redfish_status_command(host, user, password);
        let (ok, stdout, _) = run_cmd(connection, &status_cmd, context)?;

        let current_state = if ok {
            // Parse JSON response for PowerState field
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                json.get("PowerState")
                    .and_then(|v| v.as_str())
                    .map(PowerState::from_redfish_value)
                    .unwrap_or(PowerState::Unknown)
            } else {
                PowerState::Unknown
            }
        } else {
            PowerState::Unknown
        };

        if action == PowerAction::Status {
            return Ok(ModuleOutput::ok(format!(
                "Power state: {:?}",
                current_state
            ))
            .with_data("power_state", serde_json::json!(current_state))
            .with_data("host", serde_json::json!(host))
            .with_data("provider", serde_json::json!("redfish")));
        }

        if !Self::action_would_change(action, current_state) {
            return Ok(ModuleOutput::ok(format!(
                "Server is already {:?}, no action needed",
                current_state
            ))
            .with_data("power_state", serde_json::json!(current_state))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action)));
        }

        let reset_type = action.to_redfish_reset_type().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "Cannot determine Redfish reset type for action".to_string(),
            )
        })?;

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute Redfish {} on {} (current state: {:?})",
                reset_type, host, current_state
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action))
            .with_data("reset_type", serde_json::json!(reset_type))
            .with_data("current_state", serde_json::json!(current_state)));
        }

        let cmd = Self::build_redfish_reset_command(host, user, password, reset_type);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Redfish {} executed on {} (was {:?})",
            reset_type, host, current_state,
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("reset_type", serde_json::json!(reset_type))
        .with_data("previous_state", serde_json::json!(current_state))
        .with_data("provider", serde_json::json!("redfish")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_action_from_str() {
        assert_eq!(PowerAction::from_str("on"), Some(PowerAction::On));
        assert_eq!(PowerAction::from_str("OFF"), Some(PowerAction::Off));
        assert_eq!(PowerAction::from_str("Reset"), Some(PowerAction::Reset));
        assert_eq!(PowerAction::from_str("cycle"), Some(PowerAction::Cycle));
        assert_eq!(PowerAction::from_str("status"), Some(PowerAction::Status));
        assert_eq!(PowerAction::from_str("invalid"), None);
        assert_eq!(PowerAction::from_str(""), None);
    }

    #[test]
    fn test_power_action_to_ipmi_cmd() {
        assert_eq!(PowerAction::On.to_ipmi_cmd(), "on");
        assert_eq!(PowerAction::Off.to_ipmi_cmd(), "off");
        assert_eq!(PowerAction::Reset.to_ipmi_cmd(), "reset");
        assert_eq!(PowerAction::Cycle.to_ipmi_cmd(), "cycle");
        assert_eq!(PowerAction::Status.to_ipmi_cmd(), "status");
    }

    #[test]
    fn test_power_action_to_redfish_reset_type() {
        assert_eq!(PowerAction::On.to_redfish_reset_type(), Some("On"));
        assert_eq!(PowerAction::Off.to_redfish_reset_type(), Some("ForceOff"));
        assert_eq!(
            PowerAction::Reset.to_redfish_reset_type(),
            Some("ForceRestart")
        );
        assert_eq!(
            PowerAction::Cycle.to_redfish_reset_type(),
            Some("PowerCycle")
        );
        assert_eq!(PowerAction::Status.to_redfish_reset_type(), None);
    }

    #[test]
    fn test_power_state_from_ipmi_output() {
        assert_eq!(
            PowerState::from_ipmi_output("Chassis Power is on"),
            PowerState::On
        );
        assert_eq!(
            PowerState::from_ipmi_output("Chassis Power is off"),
            PowerState::Off
        );
        assert_eq!(
            PowerState::from_ipmi_output("CHASSIS POWER IS ON\n"),
            PowerState::On
        );
        assert_eq!(
            PowerState::from_ipmi_output("some random text"),
            PowerState::Unknown
        );
        assert_eq!(PowerState::from_ipmi_output(""), PowerState::Unknown);
    }

    #[test]
    fn test_power_state_from_redfish_value() {
        assert_eq!(PowerState::from_redfish_value("On"), PowerState::On);
        assert_eq!(PowerState::from_redfish_value("Off"), PowerState::Off);
        assert_eq!(
            PowerState::from_redfish_value("PoweredOff"),
            PowerState::Off
        );
        assert_eq!(
            PowerState::from_redfish_value("PoweredOn"),
            PowerState::On
        );
        assert_eq!(
            PowerState::from_redfish_value("Something"),
            PowerState::Unknown
        );
    }

    #[test]
    fn test_power_provider_from_str() {
        assert_eq!(PowerProvider::from_str("ipmi"), Some(PowerProvider::Ipmi));
        assert_eq!(
            PowerProvider::from_str("redfish"),
            Some(PowerProvider::Redfish)
        );
        assert_eq!(
            PowerProvider::from_str("REDFISH"),
            Some(PowerProvider::Redfish)
        );
        assert_eq!(PowerProvider::from_str("invalid"), None);
    }

    #[test]
    fn test_action_would_change() {
        // Turning on a server that is already on should not change
        assert!(!HpcPowerModule::action_would_change(
            PowerAction::On,
            PowerState::On
        ));
        // Turning on a server that is off should change
        assert!(HpcPowerModule::action_would_change(
            PowerAction::On,
            PowerState::Off
        ));
        // Turning off a server that is already off should not change
        assert!(!HpcPowerModule::action_would_change(
            PowerAction::Off,
            PowerState::Off
        ));
        // Turning off a server that is on should change
        assert!(HpcPowerModule::action_would_change(
            PowerAction::Off,
            PowerState::On
        ));
        // Reset always changes
        assert!(HpcPowerModule::action_would_change(
            PowerAction::Reset,
            PowerState::On
        ));
        assert!(HpcPowerModule::action_would_change(
            PowerAction::Reset,
            PowerState::Off
        ));
        // Cycle always changes
        assert!(HpcPowerModule::action_would_change(
            PowerAction::Cycle,
            PowerState::On
        ));
        // Status never changes
        assert!(!HpcPowerModule::action_would_change(
            PowerAction::Status,
            PowerState::On
        ));
        assert!(!HpcPowerModule::action_would_change(
            PowerAction::Status,
            PowerState::Off
        ));
        // Unknown state should trigger change for On/Off
        assert!(HpcPowerModule::action_would_change(
            PowerAction::On,
            PowerState::Unknown
        ));
        assert!(HpcPowerModule::action_would_change(
            PowerAction::Off,
            PowerState::Unknown
        ));
    }

    #[test]
    fn test_build_ipmi_command() {
        let cmd = HpcPowerModule::build_ipmi_command(
            "10.0.0.1",
            "admin",
            "secret",
            "lanplus",
            PowerAction::On,
        );
        assert!(cmd.contains("ipmitool"));
        assert!(cmd.contains("-I lanplus"));
        assert!(cmd.contains("-H 10.0.0.1"));
        assert!(cmd.contains("-U admin"));
        assert!(cmd.contains("chassis power on"));
    }

    #[test]
    fn test_build_ipmi_command_password_escaping() {
        let cmd = HpcPowerModule::build_ipmi_command(
            "10.0.0.1",
            "admin",
            "p'ass",
            "lanplus",
            PowerAction::Status,
        );
        // Ensure the single quote in the password is escaped
        assert!(cmd.contains("p'\\''ass"));
        assert!(cmd.contains("chassis power status"));
    }

    #[test]
    fn test_build_redfish_status_command() {
        let cmd =
            HpcPowerModule::build_redfish_status_command("bmc.example.com", "admin", "pass123");
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-sk"));
        assert!(cmd.contains("admin:pass123"));
        assert!(cmd.contains("https://bmc.example.com/redfish/v1/Systems/1"));
    }

    #[test]
    fn test_build_redfish_reset_command() {
        let cmd = HpcPowerModule::build_redfish_reset_command(
            "bmc.example.com",
            "admin",
            "pass123",
            "ForceRestart",
        );
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-X POST"));
        assert!(cmd.contains("ForceRestart"));
        assert!(cmd.contains("ComputerSystem.Reset"));
    }

    #[test]
    fn test_module_name_and_description() {
        let module = HpcPowerModule;
        assert_eq!(module.name(), "hpc_power");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_required_params() {
        let module = HpcPowerModule;
        let required = module.required_params();
        assert!(required.contains(&"action"));
        assert!(required.contains(&"host"));
    }

    #[test]
    fn test_module_optional_params() {
        let module = HpcPowerModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("interface"));
        assert!(optional.contains_key("provider"));
    }

    #[test]
    fn test_power_action_serde() {
        let action = PowerAction::On;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, "\"on\"");

        let parsed: PowerAction = serde_json::from_str("\"reset\"").unwrap();
        assert_eq!(parsed, PowerAction::Reset);
    }

    #[test]
    fn test_power_state_serde() {
        let state = PowerState::Off;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"off\"");

        let parsed: PowerState = serde_json::from_str("\"on\"").unwrap();
        assert_eq!(parsed, PowerState::On);
    }
}
