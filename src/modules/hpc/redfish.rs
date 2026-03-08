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
//! - `timeout` (optional): Timeout in seconds for task polling (default: 120)
//! - `maintenance_window` (optional): ISO8601 time range for maintenance window
//!
//! # RedfishInfoModule Parameters
//!
//! - `host` (required): BMC hostname or IP address
//! - `user` (optional): Redfish username (default: "admin")
//! - `password` (optional): Redfish password (default: "")
//! - `query_type` (required): Type of query - "system", "chassis", "thermal", "power", "storage"
//! - `verify_ssl` (optional): Verify SSL certificates (default: false)
//! - `timeout` (optional): Timeout in seconds for task polling (default: 120)
//! - `maintenance_window` (optional): ISO8601 time range for maintenance window

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};
use crate::utils::shell_escape;

/// Result of a preflight check before performing Redfish operations.
#[derive(Debug, Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single configuration drift item describing a mismatch between desired and actual state.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Result of a verification or task-polling operation.
#[derive(Debug, Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

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
fn build_curl_base(user: &str, password: &str, verify_ssl: bool) -> String {
    let ssl_flag = if verify_ssl { "-s" } else { "-sk" };
    let auth = format!("{}:{}", user, password);
    format!("curl {} -u {} ", ssl_flag, shell_escape(&auth))
}

fn redfish_url(host: &str, path: &str) -> String {
    shell_escape(&format!("https://{}{}", host, path)).into_owned()
}

/// Parse a JSON string into a `serde_json::Value`, returning a `ModuleError` on failure.
fn parse_redfish_json(raw: &str) -> Result<serde_json::Value, ModuleError> {
    serde_json::from_str(raw).map_err(|e| {
        ModuleError::ExecutionFailed(format!("Failed to parse Redfish JSON response: {}", e))
    })
}

/// Detect the BMC vendor from a `/redfish/v1/Systems/1` JSON response string.
///
/// Extracts the `"Manufacturer"` field and normalizes common vendor names to
/// a short lowercase identifier. Unknown vendors are lowercased as-is.
fn detect_vendor(system_json: &str) -> String {
    let json = match parse_redfish_json(system_json) {
        Ok(v) => v,
        Err(_) => return "unknown".to_string(),
    };

    let manufacturer = json
        .get("Manufacturer")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    normalize_vendor(manufacturer)
}

/// Normalize a raw manufacturer string into a short vendor identifier.
fn normalize_vendor(manufacturer: &str) -> String {
    let lower = manufacturer.to_lowercase();
    if lower.contains("dell") {
        "dell".to_string()
    } else if lower == "hpe" || lower.contains("hewlett") {
        "hpe".to_string()
    } else if lower.contains("lenovo") {
        "lenovo".to_string()
    } else if lower.contains("supermicro") {
        "supermicro".to_string()
    } else if lower == "unknown" || manufacturer.is_empty() {
        "unknown".to_string()
    } else {
        lower
    }
}

/// Poll a Redfish task URI until the task reaches a terminal state or the timeout expires.
///
/// Uses exponential back-off between polls (1s, 2s, 4s, ... capped at 16s).
/// Terminal task states: `Completed`, `Exception`, `Killed`.
#[allow(clippy::too_many_arguments)]
fn poll_task_status(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    host: &str,
    user: &str,
    password: &str,
    verify_ssl: bool,
    task_uri: &str,
    timeout_secs: u32,
) -> VerifyResult {
    let start = Instant::now();
    let timeout = Duration::from_secs(u64::from(timeout_secs));
    let mut backoff_secs: u64 = 1;
    let max_backoff: u64 = 16;

    let mut details: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    loop {
        if start.elapsed() >= timeout {
            warnings.push(format!(
                "Timeout after {}s waiting for task {}",
                timeout_secs, task_uri
            ));
            return VerifyResult {
                verified: false,
                details,
                warnings,
            };
        }

        let cmd = format!(
            "{}{} 2>/dev/null",
            build_curl_base(user, password, verify_ssl),
            redfish_url(host, task_uri)
        );

        let stdout = match run_cmd_ok(connection, &cmd, context) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!("Error polling task: {}", e));
                return VerifyResult {
                    verified: false,
                    details,
                    warnings,
                };
            }
        };

        let task_state = match parse_redfish_json(&stdout) {
            Ok(json) => {
                let state = json
                    .get("TaskState")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();

                if let Some(pct) = json.get("PercentComplete").and_then(|v| v.as_u64()) {
                    details.push(format!("Task progress: {}%", pct));
                }

                state
            }
            Err(e) => {
                warnings.push(format!("Failed to parse task response: {}", e));
                return VerifyResult {
                    verified: false,
                    details,
                    warnings,
                };
            }
        };

        match task_state.as_str() {
            "Completed" => {
                details.push("Task completed successfully".to_string());
                return VerifyResult {
                    verified: true,
                    details,
                    warnings,
                };
            }
            "Exception" => {
                details.push("Task failed with exception".to_string());
                return VerifyResult {
                    verified: false,
                    details,
                    warnings,
                };
            }
            "Killed" => {
                details.push("Task was killed".to_string());
                return VerifyResult {
                    verified: false,
                    details,
                    warnings,
                };
            }
            _ => {
                details.push(format!("Task state: {}", task_state));
            }
        }

        // Sleep with exponential back-off
        std::thread::sleep(Duration::from_secs(backoff_secs));
        backoff_secs = (backoff_secs * 2).min(max_backoff);
    }
}

/// Perform a firmware preflight check by querying the Redfish FirmwareInventory.
///
/// Returns a `PreflightResult` with the BMC firmware version and warnings about
/// known-bad firmware versions.
fn firmware_precheck(firmware_json: &str) -> PreflightResult {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let json = match parse_redfish_json(firmware_json) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("Cannot parse firmware inventory: {}", e));
            return PreflightResult {
                passed: false,
                warnings,
                errors,
            };
        }
    };

    // Look for Members array containing firmware entries
    let members = json
        .get("Members")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut bmc_version: Option<String> = None;

    for member in &members {
        let name = member.get("Name").and_then(|v| v.as_str()).unwrap_or("");
        let version = member.get("Version").and_then(|v| v.as_str()).unwrap_or("");

        let name_lower = name.to_lowercase();
        if name_lower.contains("bmc")
            || name_lower.contains("idrac")
            || name_lower.contains("ilo")
            || name_lower.contains("remote access controller")
        {
            bmc_version = Some(version.to_string());

            // Warn about known-bad firmware versions
            if version.starts_with("1.0.") || version.starts_with("0.") {
                warnings.push(format!(
                    "BMC firmware {} version {} is known to have issues; consider upgrading",
                    name, version
                ));
            }
        }
    }

    if let Some(ref ver) = bmc_version {
        if ver.is_empty() {
            warnings.push("BMC firmware version is empty".to_string());
        }
    }

    PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    }
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
            "{}{} 2>/dev/null",
            build_curl_base(user, password, verify_ssl),
            redfish_url(host, "/redfish/v1/Systems/1")
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
        let payload = format!(r#"{{"ResetType": "{}"}}"#, reset_type);
        format!(
            "{}-X POST -H 'Content-Type: application/json' \
             -d {} \
             {} 2>/dev/null",
            build_curl_base(user, password, verify_ssl),
            shell_escape(&payload),
            redfish_url(host, "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset")
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

    /// Query system endpoint and return the raw JSON response for vendor detection and
    /// other enrichment. Returns an empty string on failure.
    fn query_system_json(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        host: &str,
        user: &str,
        password: &str,
        verify_ssl: bool,
    ) -> String {
        let cmd = Self::build_status_command(host, user, password, verify_ssl);
        match run_cmd(connection, &cmd, context) {
            Ok((true, stdout, _)) => stdout,
            _ => String::new(),
        }
    }

    /// Query firmware inventory endpoint and return the raw JSON response.
    fn query_firmware_inventory(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        host: &str,
        user: &str,
        password: &str,
        verify_ssl: bool,
    ) -> String {
        let cmd = format!(
            "{}{} 2>/dev/null",
            build_curl_base(user, password, verify_ssl),
            redfish_url(host, "/redfish/v1/UpdateService/FirmwareInventory")
        );
        match run_cmd(connection, &cmd, context) {
            Ok((true, stdout, _)) => stdout,
            _ => String::new(),
        }
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
        let timeout_secs = params.get_u32("timeout")?.unwrap_or(120);
        let _maintenance_window = params.get_string("maintenance_window")?;

        let action = RedfishPowerAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'on', 'off', 'reset', 'cycle', or 'status'",
                action_str
            ))
        })?;

        // Vendor detection: query system endpoint for manufacturer info
        let system_json =
            Self::query_system_json(connection, context, &host, &user, &password, verify_ssl);
        let vendor = detect_vendor(&system_json);

        // Firmware precheck: query firmware inventory and check for known-bad versions
        let firmware_json = Self::query_firmware_inventory(
            connection, context, &host, &user, &password, verify_ssl,
        );
        let preflight = firmware_precheck(&firmware_json);

        // Extract BMC firmware version from preflight data (best-effort)
        let firmware_version = {
            let fw_json = parse_redfish_json(&firmware_json).ok();
            fw_json
                .as_ref()
                .and_then(|j| j.get("Members"))
                .and_then(|v| v.as_array())
                .and_then(|members| {
                    members.iter().find_map(|m| {
                        let name = m.get("Name").and_then(|v| v.as_str()).unwrap_or("");
                        let name_lower = name.to_lowercase();
                        if name_lower.contains("bmc")
                            || name_lower.contains("idrac")
                            || name_lower.contains("ilo")
                        {
                            m.get("Version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| "unknown".to_string())
        };

        // Always query current state for idempotency
        let current_state =
            Self::query_power_state(connection, context, &host, &user, &password, verify_ssl);

        // For status action, just return current state with enriched data
        if action == RedfishPowerAction::Status {
            return Ok(
                ModuleOutput::ok(format!("Power state: {:?}", current_state))
                    .with_data("power_state", serde_json::json!(current_state))
                    .with_data("host", serde_json::json!(host))
                    .with_data("vendor", serde_json::json!(vendor))
                    .with_data("firmware_version", serde_json::json!(firmware_version))
                    .with_data("preflight", serde_json::json!(preflight)),
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
            .with_data("vendor", serde_json::json!(vendor))
            .with_data("firmware_version", serde_json::json!(firmware_version))
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
            .with_data("current_state", serde_json::json!(current_state))
            .with_data("vendor", serde_json::json!(vendor))
            .with_data("firmware_version", serde_json::json!(firmware_version)));
        }

        let cmd = Self::build_reset_command(&host, &user, &password, reset_type, verify_ssl);
        let reset_output = run_cmd_ok(connection, &cmd, context)?;

        // Check if the response contains a task URI for async tracking
        let task_status = parse_redfish_json(&reset_output)
            .ok()
            .and_then(|json| {
                json.get("@odata.id")
                    .and_then(|v| v.as_str())
                    .map(|uri| uri.to_string())
            })
            .map(|task_uri| {
                let normalized_task_uri =
                    if task_uri.starts_with("http://") || task_uri.starts_with("https://") {
                        task_uri
                            .split_once("/redfish/")
                            .map(|(_, suffix)| format!("/redfish/{}", suffix))
                            .unwrap_or_else(|| "/redfish/v1/TaskService/Tasks".to_string())
                    } else if task_uri.starts_with('/') {
                        task_uri
                    } else {
                        format!("/{}", task_uri)
                    };
                poll_task_status(
                    connection,
                    context,
                    &host,
                    &user,
                    &password,
                    verify_ssl,
                    &normalized_task_uri,
                    timeout_secs,
                )
            });

        let mut output = ModuleOutput::changed(format!(
            "Redfish {} executed on {} (was {:?})",
            reset_type, host, current_state,
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("reset_type", serde_json::json!(reset_type))
        .with_data("previous_state", serde_json::json!(current_state))
        .with_data("vendor", serde_json::json!(vendor))
        .with_data("firmware_version", serde_json::json!(firmware_version));

        if let Some(ref status) = task_status {
            output = output.with_data("task_status", serde_json::json!(status));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("verify_ssl", serde_json::json!(false));
        m.insert("timeout", serde_json::json!(120));
        m.insert("maintenance_window", serde_json::Value::Null);
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
            "{}{} 2>/dev/null",
            build_curl_base(user, password, verify_ssl),
            redfish_url(host, query_type.to_endpoint())
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
        let _timeout_secs = params.get_u32("timeout")?.unwrap_or(120);
        let _maintenance_window = params.get_string("maintenance_window")?;

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

        // Vendor detection when querying system endpoint
        let vendor = if query_type == RedfishQueryType::System {
            detect_vendor(&output)
        } else {
            "unknown".to_string()
        };

        // Extract firmware version from system response if available
        let firmware_version = json_data
            .as_ref()
            .and_then(|j| j.get("BiosVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(ModuleOutput::ok(format!(
            "Successfully queried {} information from {}",
            query_type_str, host
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("query_type", serde_json::json!(query_type))
        .with_data("endpoint", serde_json::json!(query_type.to_endpoint()))
        .with_data("vendor", serde_json::json!(vendor))
        .with_data("firmware_version", serde_json::json!(firmware_version))
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
        m.insert("timeout", serde_json::json!(120));
        m.insert("maintenance_window", serde_json::Value::Null);
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
        let cmd = build_curl_base("admin", "pass123", false);
        assert!(cmd.contains("curl"));
        assert!(cmd.contains("-sk"));
        assert!(cmd.contains("admin:pass123"));

        let cmd_verify = build_curl_base("admin", "pass123", true);
        assert!(cmd_verify.contains("curl"));
        assert!(!cmd_verify.contains("-k"));
        assert!(cmd_verify.contains("admin:pass123"));
    }

    #[test]
    fn test_build_curl_base_password_escaping() {
        let cmd = build_curl_base("user", "p'ass", false);
        assert!(cmd.contains("p'\\''ass"));
    }

    #[test]
    fn test_redfish_url_escapes_host_and_path() {
        let url = redfish_url("bmc.example.com;touch /tmp/pwn", "/redfish/v1/Systems/1");
        assert_eq!(
            url,
            "'https://bmc.example.com;touch /tmp/pwn/redfish/v1/Systems/1'"
        );
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
        assert!(optional.contains_key("timeout"));
        assert!(optional.contains_key("maintenance_window"));
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
        assert!(optional.contains_key("timeout"));
        assert!(optional.contains_key("maintenance_window"));
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

    #[test]
    fn test_vendor_normalization() {
        // Dell variants
        assert_eq!(detect_vendor(r#"{"Manufacturer": "Dell Inc."}"#), "dell");
        assert_eq!(detect_vendor(r#"{"Manufacturer": "DELL"}"#), "dell");
        assert_eq!(
            detect_vendor(r#"{"Manufacturer": "Dell Technologies"}"#),
            "dell"
        );

        // HPE variants
        assert_eq!(detect_vendor(r#"{"Manufacturer": "HPE"}"#), "hpe");
        assert_eq!(
            detect_vendor(r#"{"Manufacturer": "Hewlett Packard Enterprise"}"#),
            "hpe"
        );

        // Lenovo
        assert_eq!(detect_vendor(r#"{"Manufacturer": "Lenovo"}"#), "lenovo");
        assert_eq!(
            detect_vendor(r#"{"Manufacturer": "Lenovo Group Ltd."}"#),
            "lenovo"
        );

        // Supermicro
        assert_eq!(
            detect_vendor(r#"{"Manufacturer": "Supermicro"}"#),
            "supermicro"
        );

        // Unknown / missing
        assert_eq!(detect_vendor(r#"{"Manufacturer": "unknown"}"#), "unknown");
        assert_eq!(detect_vendor(r#"{"Model": "no-manufacturer"}"#), "unknown");
        assert_eq!(detect_vendor("not json at all"), "unknown");
        assert_eq!(detect_vendor(r#"{"Manufacturer": ""}"#), "unknown");

        // Other vendor falls back to lowercased name
        assert_eq!(detect_vendor(r#"{"Manufacturer": "Fujitsu"}"#), "fujitsu");
    }

    #[test]
    fn test_task_status_parsing() {
        // Completed task
        let completed_json = r#"{
            "TaskState": "Completed",
            "PercentComplete": 100,
            "@odata.id": "/redfish/v1/TaskService/Tasks/1"
        }"#;
        let parsed = parse_redfish_json(completed_json).unwrap();
        assert_eq!(
            parsed.get("TaskState").and_then(|v| v.as_str()),
            Some("Completed")
        );
        assert_eq!(
            parsed.get("PercentComplete").and_then(|v| v.as_u64()),
            Some(100)
        );

        // Running task
        let running_json = r#"{
            "TaskState": "Running",
            "PercentComplete": 45
        }"#;
        let parsed = parse_redfish_json(running_json).unwrap();
        assert_eq!(
            parsed.get("TaskState").and_then(|v| v.as_str()),
            Some("Running")
        );
        assert_eq!(
            parsed.get("PercentComplete").and_then(|v| v.as_u64()),
            Some(45)
        );

        // Exception task
        let exception_json = r#"{"TaskState": "Exception"}"#;
        let parsed = parse_redfish_json(exception_json).unwrap();
        assert_eq!(
            parsed.get("TaskState").and_then(|v| v.as_str()),
            Some("Exception")
        );

        // Killed task
        let killed_json = r#"{"TaskState": "Killed"}"#;
        let parsed = parse_redfish_json(killed_json).unwrap();
        assert_eq!(
            parsed.get("TaskState").and_then(|v| v.as_str()),
            Some("Killed")
        );

        // Missing TaskState field
        let no_state_json = r#"{"SomeOtherField": "value"}"#;
        let parsed = parse_redfish_json(no_state_json).unwrap();
        assert!(parsed.get("TaskState").is_none());
    }

    #[test]
    fn test_redfish_json_parsing() {
        // Valid JSON
        let valid = r#"{"PowerState": "On", "Model": "PowerEdge R640"}"#;
        let result = parse_redfish_json(valid);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert_eq!(json.get("PowerState").and_then(|v| v.as_str()), Some("On"));
        assert_eq!(
            json.get("Model").and_then(|v| v.as_str()),
            Some("PowerEdge R640")
        );

        // Invalid JSON
        let invalid = "this is not json {{{";
        let result = parse_redfish_json(invalid);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            ModuleError::ExecutionFailed(msg) => {
                assert!(msg.contains("Failed to parse Redfish JSON response"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }

        // Empty string
        let empty = "";
        let result = parse_redfish_json(empty);
        assert!(result.is_err());

        // Valid nested JSON
        let nested = r#"{"Members": [{"Name": "BMC Firmware", "Version": "2.5.1"}]}"#;
        let result = parse_redfish_json(nested);
        assert!(result.is_ok());
        let json = result.unwrap();
        let members = json.get("Members").and_then(|v| v.as_array()).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(
            members[0].get("Name").and_then(|v| v.as_str()),
            Some("BMC Firmware")
        );
    }

    #[test]
    fn test_firmware_precheck() {
        // Normal firmware inventory with good version
        let good_fw = r#"{
            "Members": [
                {"Name": "BMC Firmware", "Version": "2.5.1"},
                {"Name": "BIOS", "Version": "1.2.3"}
            ]
        }"#;
        let result = firmware_precheck(good_fw);
        assert!(result.passed);
        assert!(result.warnings.is_empty());
        assert!(result.errors.is_empty());

        // Firmware with known-bad version
        let bad_fw = r#"{
            "Members": [
                {"Name": "BMC Firmware", "Version": "1.0.2"},
                {"Name": "BIOS", "Version": "1.2.3"}
            ]
        }"#;
        let result = firmware_precheck(bad_fw);
        assert!(result.passed);
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("known to have issues"));

        // iDRAC firmware
        let idrac_fw = r#"{
            "Members": [
                {"Name": "Integrated Dell Remote Access Controller", "Version": "0.9.0"}
            ]
        }"#;
        let result = firmware_precheck(idrac_fw);
        assert!(result.passed);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("known to have issues")));

        // iLO firmware
        let ilo_fw = r#"{
            "Members": [
                {"Name": "iLO 5", "Version": "3.0.1"}
            ]
        }"#;
        let result = firmware_precheck(ilo_fw);
        assert!(result.passed);
        assert!(result.warnings.is_empty());

        // Invalid JSON
        let invalid = "not json";
        let result = firmware_precheck(invalid);
        assert!(!result.passed);
        assert!(!result.errors.is_empty());

        // Empty Members array
        let empty_members = r#"{"Members": []}"#;
        let result = firmware_precheck(empty_members);
        assert!(result.passed);
        assert!(result.warnings.is_empty());
    }
}
