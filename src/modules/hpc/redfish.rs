//! HPC Redfish REST API modules
//!
//! Provides two modules for interacting with Redfish-compliant BMCs:
//! - `RedfishPowerModule`: Power management operations (on/off/reset/cycle/status)
//! - `RedfishInfoModule`: System information queries (inventory, sensors, thermal, power)
//!
//! Both modules use curl for Redfish REST API calls and support SSL verification control.
//!
//! # Feature Gate
//!
//! These modules are only available when the `redfish` feature is enabled.
//!
//! # RedfishPowerModule Parameters
//!
//! - `host` (required): BMC hostname or IP address
//! - `user` (optional): Redfish username (default: "admin")
//! - `password` (optional): Redfish password (default: "")
//! - `action` (required): Power action - "on", "off", "reset", "cycle", "status"
//! - `verify_ssl` (optional): Verify SSL certificates (default: false)
//!
//! # RedfishInfoModule Parameters
//!
//! - `host` (required): BMC hostname or IP address
//! - `user` (optional): Redfish username (default: "admin")
//! - `password` (optional): Redfish password (default: "")
//! - `query_type` (required): Type of query - "system", "chassis", "thermal", "power", "storage"
//! - `verify_ssl` (optional): Verify SSL certificates (default: false)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

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

/// Build base curl command with authentication and SSL options
fn build_curl_base(host: &str, user: &str, password: &str, verify_ssl: bool) -> String {
    let ssl_flag = if verify_ssl { "-s" } else { "-sk" };
    format!(
        "curl {} -u '{}:{}' ",
        ssl_flag,
        user.replace('\'', "'\\''"),
        password.replace('\'', "'\\''")
    )
}

/// Power actions supported by Redfish
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RedfishPowerAction {
    On,
    Off,
    Reset,
    Cycle,
    Status,
}

impl RedfishPowerAction {
    pub fn from_str(s: &str) -> Option<RedfishPowerAction> {
        match s.to_lowercase().as_str() {
            "on" => Some(RedfishPowerAction::On),
            "off" => Some(RedfishPowerAction::Off),
            "reset" => Some(RedfishPowerAction::Reset),
            "cycle" => Some(RedfishPowerAction::Cycle),
            "status" => Some(RedfishPowerAction::Status),
            _ => None,
        }
    }

    pub fn to_reset_type(&self) -> Option<&'static str> {
        match self {
            RedfishPowerAction::On => Some("On"),
            RedfishPowerAction::Off => Some("ForceOff"),
            RedfishPowerAction::Reset => Some("ForceRestart"),
            RedfishPowerAction::Cycle => Some("PowerCycle"),
            RedfishPowerAction::Status => None,
        }
    }
}

/// Current power state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    On,
    Off,
    Unknown,
}

impl PowerState {
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

/// Query types supported by RedfishInfoModule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RedfishQueryType {
    System,
    Chassis,
    Thermal,
    Power,
    Storage,
}

impl RedfishQueryType {
    pub fn from_str(s: &str) -> Option<RedfishQueryType> {
        match s.to_lowercase().as_str() {
            "system" => Some(RedfishQueryType::System),
            "chassis" => Some(RedfishQueryType::Chassis),
            "thermal" => Some(RedfishQueryType::Thermal),
            "power" => Some(RedfishQueryType::Power),
            "storage" => Some(RedfishQueryType::Storage),
            _ => None,
        }
    }

    pub fn to_endpoint(&self) -> &'static str {
        match self {
            RedfishQueryType::System => "/redfish/v1/Systems/1",
            RedfishQueryType::Chassis => "/redfish/v1/Chassis/1",
            RedfishQueryType::Thermal => "/redfish/v1/Chassis/1/Thermal",
            RedfishQueryType::Power => "/redfish/v1/Chassis/1/Power",
            RedfishQueryType::Storage => "/redfish/v1/Systems/1/Storage",
        }
    }
}

pub struct RedfishPowerModule;

impl RedfishPowerModule {
    /// Build curl command for power status query
    fn build_status_command(host: &str, user: &str, password: &str, verify_ssl: bool) -> String {
        format!(
            "{}https://{}/redfish/v1/Systems/1 2>/dev/null",
            build_curl_base(host, user, password, verify_ssl),
            host
        )
    }

    /// Build curl command for power action (reset)
    fn build_reset_command(
        host: &str,
        user: &str,
        password: &str,
        reset_type: &str,
        verify_ssl: bool,
    ) -> String {
        format!(
            "{}-X POST -H 'Content-Type: application/json' \
             -d '{{\"ResetType\": \"{}\"}}' \
             https://{}/redfish/v1/Systems/1/Actions/ComputerSystem.Reset 2>/dev/null",
            build_curl_base(host, user, password, verify_ssl),
            reset_type,
            host
        )
    }

    /// Check if action would change current state
    fn action_would_change(action: RedfishPowerAction, current: PowerState) -> bool {
        match action {
            RedfishPowerAction::On => current != PowerState::On,
            RedfishPowerAction::Off => current != PowerState::Off,
            RedfishPowerAction::Reset | RedfishPowerAction::Cycle => true,
            RedfishPowerAction::Status => false,
        }
    }

    /// Query current power state
    fn query_power_state(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        host: &str,
        user: &str,
        password: &str,
        verify_ssl: bool,
    ) -> PowerState {
        let cmd = Self::build_status_command(host, user, password, verify_ssl);
        let (ok, stdout, _) = match run_cmd(connection, &cmd, context) {
            Ok(result) => result,
            Err(_) => return PowerState::Unknown,
        };

        if ok {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                return json
                    .get("PowerState")
                    .and_then(|v| v.as_str())
                    .map(PowerState::from_redfish_value)
                    .unwrap_or(PowerState::Unknown);
            }
        }
        PowerState::Unknown
    }
}

impl Module for RedfishPowerModule {
    fn name(&self) -> &'static str {
        "redfish_power"
    }

    fn description(&self) -> &'static str {
        "Manage server power state via Redfish REST API"
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

        let host = params.get_string_required("host")?;
        let user = params
            .get_string("user")?
            .unwrap_or_else(|| "admin".to_string());
        let password = params.get_string("password")?.unwrap_or_default();
        let action_str = params.get_string_required("action")?;
        let verify_ssl = params.get_bool("verify_ssl")?.unwrap_or(false);

        let action = RedfishPowerAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'on', 'off', 'reset', 'cycle', or 'status'",
                action_str
            ))
        })?;

        // Always query current state for idempotency
        let current_state =
            Self::query_power_state(connection, context, &host, &user, &password, verify_ssl);

        // For status action, just return current state
        if action == RedfishPowerAction::Status {
            return Ok(
                ModuleOutput::ok(format!("Power state: {:?}", current_state))
                    .with_data("power_state", serde_json::json!(current_state))
                    .with_data("host", serde_json::json!(host)),
            );
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

        let reset_type = action.to_reset_type().ok_or_else(|| {
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

        let cmd = Self::build_reset_command(&host, &user, &password, reset_type, verify_ssl);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Redfish {} executed on {} (was {:?})",
            reset_type, host, current_state,
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("reset_type", serde_json::json!(reset_type))
        .with_data("previous_state", serde_json::json!(current_state)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("verify_ssl", serde_json::json!(false));
        m
    }
}

pub struct RedfishInfoModule;

impl RedfishInfoModule {
    /// Build curl command for Redfish query
    fn build_query_command(
        host: &str,
        user: &str,
        password: &str,
        query_type: RedfishQueryType,
        verify_ssl: bool,
    ) -> String {
        format!(
            "{}https://{}{} 2>/dev/null",
            build_curl_base(host, user, password, verify_ssl),
            host,
            query_type.to_endpoint()
        )
    }
}

impl Module for RedfishInfoModule {
    fn name(&self) -> &'static str {
        "redfish_info"
    }

    fn description(&self) -> &'static str {
        "Query system information via Redfish REST API"
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

        let host = params.get_string_required("host")?;
        let user = params
            .get_string("user")?
            .unwrap_or_else(|| "admin".to_string());
        let password = params.get_string("password")?.unwrap_or_default();
        let query_type_str = params.get_string_required("query_type")?;
        let verify_ssl = params.get_bool("verify_ssl")?.unwrap_or(false);

        let query_type = RedfishQueryType::from_str(&query_type_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid query_type '{}'. Must be 'system', 'chassis', 'thermal', 'power', or 'storage'",
                query_type_str
            ))
        })?;

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would query Redfish {} endpoint on {}",
                query_type_str, host
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("query_type", serde_json::json!(query_type))
            .with_data("endpoint", serde_json::json!(query_type.to_endpoint())));
        }

        let cmd = Self::build_query_command(&host, &user, &password, query_type, verify_ssl);
        let output = run_cmd_ok(connection, &cmd, context)?;

        // Try to parse JSON response
        let json_data = serde_json::from_str::<serde_json::Value>(&output).ok();

        Ok(ModuleOutput::ok(format!(
            "Successfully queried {} information from {}",
            query_type_str, host
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("query_type", serde_json::json!(query_type))
        .with_data("endpoint", serde_json::json!(query_type.to_endpoint()))
        .with_data(
            "response",
            json_data.unwrap_or_else(|| serde_json::json!(output)),
        ))
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "query_type"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("verify_ssl", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redfish_power_action_from_str() {
        assert_eq!(
            RedfishPowerAction::from_str("on"),
            Some(RedfishPowerAction::On)
        );
        assert_eq!(
            RedfishPowerAction::from_str("OFF"),
            Some(RedfishPowerAction::Off)
        );
        assert_eq!(
            RedfishPowerAction::from_str("Reset"),
            Some(RedfishPowerAction::Reset)
        );
        assert_eq!(
            RedfishPowerAction::from_str("cycle"),
            Some(RedfishPowerAction::Cycle)
        );
        assert_eq!(
            RedfishPowerAction::from_str("status"),
            Some(RedfishPowerAction::Status)
        );
        assert_eq!(RedfishPowerAction::from_str("invalid"), None);
    }

    #[test]
    fn test_redfish_power_action_to_reset_type() {
        assert_eq!(RedfishPowerAction::On.to_reset_type(), Some("On"));
        assert_eq!(RedfishPowerAction::Off.to_reset_type(), Some("ForceOff"));
        assert_eq!(
            RedfishPowerAction::Reset.to_reset_type(),
            Some("ForceRestart")
        );
        assert_eq!(
            RedfishPowerAction::Cycle.to_reset_type(),
            Some("PowerCycle")
        );
        assert_eq!(RedfishPowerAction::Status.to_reset_type(), None);
    }

    #[test]
    fn test_power_state_from_redfish_value() {
        assert_eq!(PowerState::from_redfish_value("On"), PowerState::On);
        assert_eq!(PowerState::from_redfish_value("Off"), PowerState::Off);
        assert_eq!(
            PowerState::from_redfish_value("PoweredOff"),
            PowerState::Off
        );
        assert_eq!(PowerState::from_redfish_value("PoweredOn"), PowerState::On);
        assert_eq!(
            PowerState::from_redfish_value("Something"),
            PowerState::Unknown
        );
    }

    #[test]
    fn test_redfish_query_type_from_str() {
        assert_eq!(
            RedfishQueryType::from_str("system"),
            Some(RedfishQueryType::System)
        );
        assert_eq!(
            RedfishQueryType::from_str("CHASSIS"),
            Some(RedfishQueryType::Chassis)
        );
        assert_eq!(
            RedfishQueryType::from_str("Thermal"),
            Some(RedfishQueryType::Thermal)
        );
        assert_eq!(
            RedfishQueryType::from_str("power"),
            Some(RedfishQueryType::Power)
        );
        assert_eq!(
            RedfishQueryType::from_str("storage"),
            Some(RedfishQueryType::Storage)
        );
        assert_eq!(RedfishQueryType::from_str("invalid"), None);
    }

    #[test]
    fn test_redfish_query_type_to_endpoint() {
        assert_eq!(
            RedfishQueryType::System.to_endpoint(),
            "/redfish/v1/Systems/1"
        );
        assert_eq!(
            RedfishQueryType::Chassis.to_endpoint(),
            "/redfish/v1/Chassis/1"
        );
        assert_eq!(
            RedfishQueryType::Thermal.to_endpoint(),
            "/redfish/v1/Chassis/1/Thermal"
        );
        assert_eq!(
            RedfishQueryType::Power.to_endpoint(),
            "/redfish/v1/Chassis/1/Power"
        );
        assert_eq!(
            RedfishQueryType::Storage.to_endpoint(),
            "/redfish/v1/Systems/1/Storage"
        );
    }

    #[test]
    fn test_build_curl_base() {
        let cmd = build_curl_base("bmc.example.com", "admin", "pass123", false);
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-sk"));
        assert!(cmd.contains("admin:pass123"));

        let cmd_verify = build_curl_base("bmc.example.com", "admin", "pass123", true);
        assert!(cmd_verify.contains("curl"));
        assert!(!cmd_verify.contains("-k"));
        assert!(cmd_verify.contains("admin:pass123"));
    }

    #[test]
    fn test_build_curl_base_password_escaping() {
        let cmd = build_curl_base("host", "user", "p'ass", false);
        assert!(cmd.contains("p'\\''ass"));
    }

    #[test]
    fn test_build_status_command() {
        let cmd =
            RedfishPowerModule::build_status_command("bmc.example.com", "admin", "pass123", false);
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-sk"));
        assert!(cmd.contains("admin:pass123"));
        assert!(cmd.contains("https://bmc.example.com/redfish/v1/Systems/1"));
    }

    #[test]
    fn test_build_reset_command() {
        let cmd = RedfishPowerModule::build_reset_command(
            "bmc.example.com",
            "admin",
            "pass123",
            "ForceRestart",
            false,
        );
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-X POST"));
        assert!(cmd.contains("ForceRestart"));
        assert!(cmd.contains("ComputerSystem.Reset"));
    }

    #[test]
    fn test_build_query_command() {
        let cmd = RedfishInfoModule::build_query_command(
            "bmc.example.com",
            "admin",
            "pass123",
            RedfishQueryType::Thermal,
            false,
        );
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("admin:pass123"));
        assert!(cmd.contains("https://bmc.example.com/redfish/v1/Chassis/1/Thermal"));
    }

    #[test]
    fn test_action_would_change() {
        assert!(!RedfishPowerModule::action_would_change(
            RedfishPowerAction::On,
            PowerState::On
        ));
        assert!(RedfishPowerModule::action_would_change(
            RedfishPowerAction::On,
            PowerState::Off
        ));
        assert!(!RedfishPowerModule::action_would_change(
            RedfishPowerAction::Off,
            PowerState::Off
        ));
        assert!(RedfishPowerModule::action_would_change(
            RedfishPowerAction::Off,
            PowerState::On
        ));
        assert!(RedfishPowerModule::action_would_change(
            RedfishPowerAction::Reset,
            PowerState::On
        ));
        assert!(RedfishPowerModule::action_would_change(
            RedfishPowerAction::Cycle,
            PowerState::Off
        ));
        assert!(!RedfishPowerModule::action_would_change(
            RedfishPowerAction::Status,
            PowerState::On
        ));
    }

    #[test]
    fn test_redfish_power_module_metadata() {
        let module = RedfishPowerModule;
        assert_eq!(module.name(), "redfish_power");
        assert!(!module.description().is_empty());

        let required = module.required_params();
        assert!(required.contains(&"host"));
        assert!(required.contains(&"action"));

        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("verify_ssl"));
    }

    #[test]
    fn test_redfish_info_module_metadata() {
        let module = RedfishInfoModule;
        assert_eq!(module.name(), "redfish_info");
        assert!(!module.description().is_empty());

        let required = module.required_params();
        assert!(required.contains(&"host"));
        assert!(required.contains(&"query_type"));

        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("verify_ssl"));
    }

    #[test]
    fn test_redfish_power_action_serde() {
        let action = RedfishPowerAction::On;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, "\"on\"");

        let parsed: RedfishPowerAction = serde_json::from_str("\"reset\"").unwrap();
        assert_eq!(parsed, RedfishPowerAction::Reset);
    }

    #[test]
    fn test_power_state_serde() {
        let state = PowerState::Off;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"off\"");

        let parsed: PowerState = serde_json::from_str("\"on\"").unwrap();
        assert_eq!(parsed, PowerState::On);
    }

    #[test]
    fn test_redfish_query_type_serde() {
        let query = RedfishQueryType::Thermal;
        let json = serde_json::to_string(&query).unwrap();
        assert_eq!(json, "\"thermal\"");

        let parsed: RedfishQueryType = serde_json::from_str("\"chassis\"").unwrap();
        assert_eq!(parsed, RedfishQueryType::Chassis);
    }
}
