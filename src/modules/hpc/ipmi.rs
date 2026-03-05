//! Dedicated IPMI power and boot management modules
//!
//! Provides full `ipmitool` option coverage for power control and boot device
//! configuration. Distinct from `HpcPowerModule` which is a generic power wrapper.
//!
//! # Modules
//!
//! - `ipmi_power`: Power on/off/reset/cycle/status via ipmitool
//! - `ipmi_boot`: Set boot device (pxe/disk/cdrom/bios) via ipmitool
//!
//! # Robustness (BM-01)
//!
//! Both modules include BMC reachability preflight checks, configurable retry
//! logic with exponential backoff, and post-action power state verification.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};
use crate::utils::shell_escape;

// ---- Robustness result structs (BM-01) ----

/// Result of a BMC reachability preflight check.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// Result of a post-action power state verification.
#[derive(Debug, serde::Serialize)]
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

/// Execute a command with configurable retries and exponential backoff.
///
/// Returns `(success, stdout, stderr, retries_used)`. On each failed attempt the
/// function sleeps for `initial_delay_ms * 2^attempt` milliseconds before retrying.
/// A log of retry attempts is collected in the returned stderr when retries occur.
fn run_cmd_with_retry(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
    _context_label: &str,
    max_retries: u32,
    initial_delay_ms: u64,
) -> ModuleResult<(bool, String, String, u32)> {
    let mut retries_used: u32 = 0;
    let mut retry_log: Vec<String> = Vec::new();

    loop {
        let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
        if success {
            let combined_stderr = if retry_log.is_empty() {
                stderr
            } else {
                format!("{}\n[retry log] {}", stderr, retry_log.join("; "))
            };
            return Ok((true, stdout, combined_stderr, retries_used));
        }

        if retries_used >= max_retries {
            let combined_stderr = if retry_log.is_empty() {
                stderr
            } else {
                format!("{}\n[retry log] {}", stderr, retry_log.join("; "))
            };
            return Ok((false, stdout, combined_stderr, retries_used));
        }

        let delay = initial_delay_ms * 2u64.pow(retries_used);
        retry_log.push(format!(
            "attempt {} failed, retrying in {}ms",
            retries_used + 1,
            delay
        ));
        std::thread::sleep(Duration::from_millis(delay));
        retries_used += 1;
    }
}

/// Probe BMC reachability by running a quick `chassis status` command.
///
/// Classifies failures as transient (network unreachable) or terminal
/// (authentication failure) and returns a structured preflight result.
fn check_bmc_reachability(
    connection: &Arc<dyn Connection + Send + Sync>,
    base: &str,
    context: &ModuleContext,
) -> PreflightResult {
    let cmd = format!("{} chassis status", base);
    let result = run_cmd(connection, &cmd, context);

    match result {
        Ok((true, _stdout, _stderr)) => PreflightResult {
            passed: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        },
        Ok((false, _stdout, stderr)) => {
            let lower = stderr.to_lowercase();
            if lower.contains("unable to establish") || lower.contains("connection timed out") {
                PreflightResult {
                    passed: false,
                    warnings: Vec::new(),
                    errors: vec![format!(
                        "BMC network unreachable (transient): {}",
                        stderr.trim()
                    )],
                }
            } else if lower.contains("password") || lower.contains("auth") {
                PreflightResult {
                    passed: false,
                    warnings: Vec::new(),
                    errors: vec![format!(
                        "BMC authentication failure (terminal): {}",
                        stderr.trim()
                    )],
                }
            } else {
                PreflightResult {
                    passed: false,
                    warnings: vec![format!(
                        "BMC probe failed with unknown error: {}",
                        stderr.trim()
                    )],
                    errors: vec![format!("BMC unreachable: {}", stderr.trim())],
                }
            }
        }
        Err(e) => PreflightResult {
            passed: false,
            warnings: Vec::new(),
            errors: vec![format!("BMC probe connection error: {}", e)],
        },
    }
}

/// After a power action, wait briefly then re-query power state to verify the
/// action took effect. Returns a structured verification result.
fn post_verify_power_state(
    connection: &Arc<dyn Connection + Send + Sync>,
    base: &str,
    context: &ModuleContext,
    action: &str,
) -> VerifyResult {
    std::thread::sleep(Duration::from_secs(2));

    let cmd = format!("{} chassis power status", base);
    let result = run_cmd(connection, &cmd, context);

    let expected = expected_state_for_action(action);

    match result {
        Ok((true, stdout, _stderr)) => {
            let actual = parse_ipmi_power_state(&stdout);
            if actual == expected {
                VerifyResult {
                    verified: true,
                    details: vec![format!("Power state confirmed: {}", actual)],
                    warnings: Vec::new(),
                }
            } else {
                VerifyResult {
                    verified: false,
                    details: vec![format!(
                        "Expected power state '{}' after '{}', got '{}'",
                        expected, action, actual
                    )],
                    warnings: vec!["Post-action state does not match expected".to_string()],
                }
            }
        }
        Ok((false, _stdout, stderr)) => VerifyResult {
            verified: false,
            details: Vec::new(),
            warnings: vec![format!("Verification query failed: {}", stderr.trim())],
        },
        Err(e) => VerifyResult {
            verified: false,
            details: Vec::new(),
            warnings: vec![format!("Verification connection error: {}", e)],
        },
    }
}

/// Map a power action to the expected resulting power state.
fn expected_state_for_action(action: &str) -> &'static str {
    match action {
        "on" => "on",
        "off" => "off",
        "reset" | "cycle" => "on",
        _ => "unknown",
    }
}

fn build_ipmi_base(host: &str, user: &str, password: &str, interface: &str) -> String {
    format!(
        "ipmitool -I {} -H {} -U {} -P {}",
        shell_escape(interface),
        shell_escape(host),
        shell_escape(user),
        shell_escape(password),
    )
}

// ---- IPMI Power Module ----

pub struct IpmiPowerModule;

impl Module for IpmiPowerModule {
    fn name(&self) -> &'static str {
        "ipmi_power"
    }

    fn description(&self) -> &'static str {
        "Manage server power state via IPMI (ipmitool chassis power)"
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
        let interface = params
            .get_string("interface")?
            .unwrap_or_else(|| "lanplus".to_string());
        let action = params.get_string_required("action")?;
        let max_retries = params
            .get_i64("retries")?
            .map(|v| v.max(0) as u32)
            .unwrap_or(3);
        let retry_delay = params
            .get_i64("retry_delay")?
            .map(|v| v.max(1) as u64)
            .unwrap_or(2);
        let initial_delay_ms = retry_delay * 1000;

        let valid_actions = ["on", "off", "reset", "cycle", "status"];
        if !valid_actions.contains(&action.as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be one of: {}",
                action,
                valid_actions.join(", ")
            )));
        }

        let base = build_ipmi_base(&host, &user, &password, &interface);

        // Preflight: check BMC reachability
        let preflight = check_bmc_reachability(connection, &base, context);
        if !preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "BMC preflight failed: {}",
                preflight.errors.join("; ")
            )));
        }

        // Query current state
        let status_cmd = format!("{} chassis power status", base);
        let (ok, stdout, _) = run_cmd(connection, &status_cmd, context)?;
        let current_state = if ok {
            parse_ipmi_power_state(&stdout)
        } else {
            "unknown"
        };

        // Status action: just report
        if action == "status" {
            return Ok(ModuleOutput::ok(format!("Power state: {}", current_state))
                .with_data("power_state", serde_json::json!(current_state))
                .with_data("host", serde_json::json!(host))
                .with_data("preflight", serde_json::json!(preflight)));
        }

        // Idempotency
        let would_change = match action.as_str() {
            "on" => current_state != "on",
            "off" => current_state != "off",
            "reset" | "cycle" => true,
            _ => true,
        };

        if !would_change {
            return Ok(ModuleOutput::ok(format!(
                "Server is already {}, no action needed",
                current_state
            ))
            .with_data("power_state", serde_json::json!(current_state))
            .with_data("host", serde_json::json!(host))
            .with_data("preflight", serde_json::json!(preflight)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute power {} on {} (current: {})",
                action, host, current_state
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action))
            .with_data("preflight", serde_json::json!(preflight)));
        }

        // Execute power action with retry
        let cmd = format!("{} chassis power {}", base, action);
        let (success, _stdout, stderr, retries_needed) = run_cmd_with_retry(
            connection,
            &cmd,
            context,
            "power action",
            max_retries,
            initial_delay_ms,
        )?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Power {} failed after {} retries: {}",
                action,
                retries_needed,
                stderr.trim()
            )));
        }

        // Post-action verification
        let verification = post_verify_power_state(connection, &base, context, &action);

        Ok(ModuleOutput::changed(format!(
            "Power {} executed on {} (was {})",
            action, host, current_state
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("previous_state", serde_json::json!(current_state))
        .with_data("preflight", serde_json::json!(preflight))
        .with_data("verification", serde_json::json!(verification))
        .with_data("retries_needed", serde_json::json!(retries_needed)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("interface", serde_json::json!("lanplus"));
        m.insert("retries", serde_json::json!(3));
        m.insert("retry_delay", serde_json::json!(2));
        m
    }
}

// ---- IPMI Boot Module ----

pub struct IpmiBootModule;

impl Module for IpmiBootModule {
    fn name(&self) -> &'static str {
        "ipmi_boot"
    }

    fn description(&self) -> &'static str {
        "Set server boot device via IPMI (ipmitool chassis bootdev)"
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
        let interface = params
            .get_string("interface")?
            .unwrap_or_else(|| "lanplus".to_string());
        let device = params.get_string_required("device")?;
        let persistent = params.get_bool_or("persistent", false);
        let max_retries = params
            .get_i64("retries")?
            .map(|v| v.max(0) as u32)
            .unwrap_or(3);
        let retry_delay = params
            .get_i64("retry_delay")?
            .map(|v| v.max(1) as u64)
            .unwrap_or(2);
        let initial_delay_ms = retry_delay * 1000;

        let valid_devices = ["pxe", "disk", "cdrom", "bios"];
        if !valid_devices.contains(&device.as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid boot device '{}'. Must be one of: {}",
                device,
                valid_devices.join(", ")
            )));
        }

        let base = build_ipmi_base(&host, &user, &password, &interface);

        // Preflight: check BMC reachability
        let preflight = check_bmc_reachability(connection, &base, context);
        if !preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "BMC preflight failed: {}",
                preflight.errors.join("; ")
            )));
        }

        // Query current boot device
        let bootparam_cmd = format!("{} chassis bootparam get 5", base);
        let (ok, stdout, _) = run_cmd(connection, &bootparam_cmd, context)?;
        let current_device = if ok {
            parse_ipmi_boot_device(&stdout)
        } else {
            "unknown".to_string()
        };

        // Map device names for comparison
        let target_device_normalized = match device.as_str() {
            "pxe" => "pxe",
            "disk" => "disk",
            "cdrom" => "cdrom",
            "bios" => "bios",
            _ => "unknown",
        };

        if current_device == target_device_normalized {
            return Ok(
                ModuleOutput::ok(format!("Boot device is already set to {}", device))
                    .with_data("boot_device", serde_json::json!(device))
                    .with_data("host", serde_json::json!(host))
                    .with_data("preflight", serde_json::json!(preflight)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set boot device to {} on {} (current: {})",
                device, host, current_device
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("device", serde_json::json!(device))
            .with_data("preflight", serde_json::json!(preflight)));
        }

        let mut cmd = format!("{} chassis bootdev {}", base, device);
        if persistent {
            cmd.push_str(" options=persistent");
        }

        // Execute bootdev command with retry
        let (success, _stdout, stderr, retries_needed) = run_cmd_with_retry(
            connection,
            &cmd,
            context,
            "bootdev",
            max_retries,
            initial_delay_ms,
        )?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Boot device set failed after {} retries: {}",
                retries_needed,
                stderr.trim()
            )));
        }

        Ok(ModuleOutput::changed(format!(
            "Set boot device to {} on {} (was {})",
            device, host, current_device
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("device", serde_json::json!(device))
        .with_data("persistent", serde_json::json!(persistent))
        .with_data("previous_device", serde_json::json!(current_device))
        .with_data("preflight", serde_json::json!(preflight))
        .with_data("retries_needed", serde_json::json!(retries_needed)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "device"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("interface", serde_json::json!("lanplus"));
        m.insert("persistent", serde_json::json!(false));
        m.insert("retries", serde_json::json!(3));
        m.insert("retry_delay", serde_json::json!(2));
        m
    }
}

fn parse_ipmi_power_state(output: &str) -> &str {
    let lower = output.to_lowercase();
    if lower.contains("chassis power is on") {
        "on"
    } else if lower.contains("chassis power is off") {
        "off"
    } else {
        "unknown"
    }
}

fn parse_ipmi_boot_device(output: &str) -> String {
    let lower = output.to_lowercase();
    if lower.contains("force pxe") || lower.contains("network") {
        "pxe".to_string()
    } else if lower.contains("force boot from default hard-drive")
        || lower.contains("disk")
        || lower.contains("hard-drive")
    {
        "disk".to_string()
    } else if lower.contains("force boot from cd/dvd") || lower.contains("cdrom") {
        "cdrom".to_string()
    } else if lower.contains("force boot into bios") || lower.contains("bios setup") {
        "bios".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipmi_power_module_metadata() {
        let module = IpmiPowerModule;
        assert_eq!(module.name(), "ipmi_power");
        assert!(!module.description().is_empty());
        assert!(module.required_params().contains(&"host"));
        assert!(module.required_params().contains(&"action"));
    }

    #[test]
    fn test_ipmi_boot_module_metadata() {
        let module = IpmiBootModule;
        assert_eq!(module.name(), "ipmi_boot");
        assert!(!module.description().is_empty());
        assert!(module.required_params().contains(&"host"));
        assert!(module.required_params().contains(&"device"));
    }

    #[test]
    fn test_parse_ipmi_power_state() {
        assert_eq!(parse_ipmi_power_state("Chassis Power is on"), "on");
        assert_eq!(parse_ipmi_power_state("Chassis Power is off"), "off");
        assert_eq!(parse_ipmi_power_state("CHASSIS POWER IS ON\n"), "on");
        assert_eq!(parse_ipmi_power_state("random text"), "unknown");
        assert_eq!(parse_ipmi_power_state(""), "unknown");
    }

    #[test]
    fn test_parse_ipmi_boot_device() {
        assert_eq!(parse_ipmi_boot_device("Force PXE"), "pxe");
        assert_eq!(
            parse_ipmi_boot_device("Force Boot from default Hard-Drive"),
            "disk"
        );
        assert_eq!(parse_ipmi_boot_device("Force Boot from CD/DVD"), "cdrom");
        assert_eq!(parse_ipmi_boot_device("Force Boot into BIOS Setup"), "bios");
        assert_eq!(parse_ipmi_boot_device("No override"), "unknown");
    }

    #[test]
    fn test_build_ipmi_base() {
        let base = build_ipmi_base("10.0.0.1", "admin", "secret", "lanplus");
        assert!(base.contains("ipmitool"));
        assert!(base.contains("-I lanplus"));
        assert!(base.contains("-H 10.0.0.1"));
        assert!(base.contains("-U admin"));
    }

    #[test]
    fn test_build_ipmi_base_password_escaping() {
        let base = build_ipmi_base("10.0.0.1", "admin", "p'ass", "lanplus");
        assert!(base.contains("p'\\''ass"));
    }

    #[test]
    fn test_build_ipmi_base_escapes_host_and_user() {
        let base = build_ipmi_base("10.0.0.1;touch /tmp/pwn", "admin user", "secret", "lanplus");
        assert!(base.contains("-H '10.0.0.1;touch /tmp/pwn'"));
        assert!(base.contains("-U 'admin user'"));
    }

    #[test]
    fn test_ipmi_power_optional_params() {
        let module = IpmiPowerModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("interface"));
        assert!(optional.contains_key("retries"));
        assert!(optional.contains_key("retry_delay"));
    }

    #[test]
    fn test_ipmi_boot_optional_params() {
        let module = IpmiBootModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("interface"));
        assert!(optional.contains_key("persistent"));
        assert!(optional.contains_key("retries"));
        assert!(optional.contains_key("retry_delay"));
    }

    #[test]
    fn test_bmc_error_classification() {
        // Network unreachable errors (transient)
        let network_errors = [
            "Unable to establish IPMI v2 / RMCP+ session",
            "Error: Connection timed out",
        ];
        for msg in &network_errors {
            let lower = msg.to_lowercase();
            let is_transient =
                lower.contains("unable to establish") || lower.contains("connection timed out");
            assert!(
                is_transient,
                "Expected transient classification for: {}",
                msg
            );
        }

        // Authentication errors (terminal)
        let auth_errors = [
            "Error in open session response message: invalid password",
            "IPMI auth error: insufficient privilege",
        ];
        for msg in &auth_errors {
            let lower = msg.to_lowercase();
            let is_auth = lower.contains("password") || lower.contains("auth");
            assert!(
                is_auth,
                "Expected auth/terminal classification for: {}",
                msg
            );
        }

        // Unknown errors (neither transient nor auth)
        let unknown_msg = "some random failure";
        let lower = unknown_msg.to_lowercase();
        let is_transient =
            lower.contains("unable to establish") || lower.contains("connection timed out");
        let is_auth = lower.contains("password") || lower.contains("auth");
        assert!(!is_transient && !is_auth, "Should be classified as unknown");
    }

    #[test]
    fn test_retry_param_defaults() {
        // Simulate default param parsing logic: no params set -> defaults
        let params: ModuleParams = HashMap::new();

        let max_retries = params
            .get_i64("retries")
            .ok()
            .flatten()
            .map(|v| v.max(0) as u32)
            .unwrap_or(3);
        let retry_delay = params
            .get_i64("retry_delay")
            .ok()
            .flatten()
            .map(|v| v.max(1) as u64)
            .unwrap_or(2);

        assert_eq!(max_retries, 3, "Default retries should be 3");
        assert_eq!(retry_delay, 2, "Default retry_delay should be 2 seconds");

        // Custom values
        let mut params_custom: ModuleParams = HashMap::new();
        params_custom.insert("retries".to_string(), serde_json::json!(5));
        params_custom.insert("retry_delay".to_string(), serde_json::json!(4));

        let max_retries = params_custom
            .get_i64("retries")
            .ok()
            .flatten()
            .map(|v| v.max(0) as u32)
            .unwrap_or(3);
        let retry_delay = params_custom
            .get_i64("retry_delay")
            .ok()
            .flatten()
            .map(|v| v.max(1) as u64)
            .unwrap_or(2);

        assert_eq!(max_retries, 5, "Custom retries should be 5");
        assert_eq!(retry_delay, 4, "Custom retry_delay should be 4 seconds");

        // Negative values should be clamped
        let mut params_neg: ModuleParams = HashMap::new();
        params_neg.insert("retries".to_string(), serde_json::json!(-2));
        params_neg.insert("retry_delay".to_string(), serde_json::json!(-1));

        let max_retries = params_neg
            .get_i64("retries")
            .ok()
            .flatten()
            .map(|v| v.max(0) as u32)
            .unwrap_or(3);
        let retry_delay = params_neg
            .get_i64("retry_delay")
            .ok()
            .flatten()
            .map(|v| v.max(1) as u64)
            .unwrap_or(2);

        assert_eq!(max_retries, 0, "Negative retries should clamp to 0");
        assert_eq!(retry_delay, 1, "Negative retry_delay should clamp to 1");
    }

    #[test]
    fn test_verify_expected_state() {
        assert_eq!(expected_state_for_action("on"), "on");
        assert_eq!(expected_state_for_action("off"), "off");
        assert_eq!(
            expected_state_for_action("reset"),
            "on",
            "Reset should expect 'on' (server restarts)"
        );
        assert_eq!(
            expected_state_for_action("cycle"),
            "on",
            "Cycle should expect 'on' (power cycles back)"
        );
        assert_eq!(
            expected_state_for_action("unknown_action"),
            "unknown",
            "Unknown actions should map to 'unknown'"
        );
    }
}
