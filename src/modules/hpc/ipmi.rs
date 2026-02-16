//! Dedicated IPMI power and boot management modules
//!
//! Provides full `ipmitool` option coverage for power control and boot device
//! configuration. Distinct from `HpcPowerModule` which is a generic power wrapper.
//!
//! # Modules
//!
//! - `ipmi_power`: Power on/off/reset/cycle/status via ipmitool
//! - `ipmi_boot`: Set boot device (pxe/disk/cdrom/bios) via ipmitool

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

fn build_ipmi_base(host: &str, user: &str, password: &str, interface: &str) -> String {
    format!(
        "ipmitool -I {} -H {} -U {} -P '{}'",
        interface,
        host,
        user,
        password.replace('\'', "'\\''"),
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

        let valid_actions = ["on", "off", "reset", "cycle", "status"];
        if !valid_actions.contains(&action.as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be one of: {}",
                action,
                valid_actions.join(", ")
            )));
        }

        let base = build_ipmi_base(&host, &user, &password, &interface);

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
                .with_data("host", serde_json::json!(host)));
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
            .with_data("host", serde_json::json!(host)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute power {} on {} (current: {})",
                action, host, current_state
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("action", serde_json::json!(action)));
        }

        let cmd = format!("{} chassis power {}", base, action);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Power {} executed on {} (was {})",
            action, host, current_state
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("action", serde_json::json!(action))
        .with_data("previous_state", serde_json::json!(current_state)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["host", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!("admin"));
        m.insert("password", serde_json::json!(""));
        m.insert("interface", serde_json::json!("lanplus"));
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

        let valid_devices = ["pxe", "disk", "cdrom", "bios"];
        if !valid_devices.contains(&device.as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid boot device '{}'. Must be one of: {}",
                device,
                valid_devices.join(", ")
            )));
        }

        let base = build_ipmi_base(&host, &user, &password, &interface);

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
                    .with_data("host", serde_json::json!(host)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set boot device to {} on {} (current: {})",
                device, host, current_device
            ))
            .with_data("host", serde_json::json!(host))
            .with_data("device", serde_json::json!(device)));
        }

        let mut cmd = format!("{} chassis bootdev {}", base, device);
        if persistent {
            cmd.push_str(" options=persistent");
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Set boot device to {} on {} (was {})",
            device, host, current_device
        ))
        .with_data("host", serde_json::json!(host))
        .with_data("device", serde_json::json!(device))
        .with_data("persistent", serde_json::json!(persistent))
        .with_data("previous_device", serde_json::json!(current_device)))
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
    fn test_ipmi_power_optional_params() {
        let module = IpmiPowerModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("interface"));
    }

    #[test]
    fn test_ipmi_boot_optional_params() {
        let module = IpmiBootModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("user"));
        assert!(optional.contains_key("password"));
        assert!(optional.contains_key("interface"));
        assert!(optional.contains_key("persistent"));
    }
}
